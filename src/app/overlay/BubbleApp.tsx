import { AnimatePresence } from "motion/react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, events, type Settings_Serialize } from "@/lib/ipc";
import { displayHotkey } from "@/lib/micDiagnosis";
import { untilOk } from "@/lib/retry";
import { Pill } from "./Pill";
import {
  errorMessage,
  INSERTED_MS,
  MESSAGE_MS,
  messageView,
  type PillKind,
  type PillView,
  stateView,
  view,
} from "./pillModel";

/** `overlay.html?demo=<kind>` renders one static state without Tauri (screenshots, fps). */
const demo = new URLSearchParams(location.search).get("demo") as PillKind | null;
const EXIT_MS = 300;

export function BubbleApp() {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<Settings_Serialize | null>(null);
  const [backend, setBackend] = useState({ state: "idle", startedAt: 0, limitMs: 300_000 });
  const [hovered, setHovered] = useState(false);
  const [transient, setTransient] = useState<PillView | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const levels = useRef<ReadonlyArray<number | null>>([]);
  const transientTimer = useRef<number | undefined>(undefined);

  const flash = useCallback((v: PillView, ms: number) => {
    window.clearTimeout(transientTimer.current);
    setTransient(v);
    transientTimer.current = window.setTimeout(() => setTransient(null), ms);
  }, []);

  // Backend wiring (skipped in demo mode).
  useEffect(() => {
    if (demo) return;
    const load = () => void untilOk(commands.getSettings).then(setSettings);
    load();
    const offs = [
      events.settingsChanged.listen(load),
      events.dictationState.listen(({ payload }) => {
        setBackend({
          state: payload.state,
          startedAt: payload.startedAt ?? 0,
          limitMs: payload.limitMs ?? 300_000,
        });
        if (payload.state !== "idle") {
          window.clearTimeout(transientTimer.current);
          setTransient(null);
        } else if (payload.reason === "cancelled") {
          flash(messageView("info", "pill.cancelled", "Cancelled"), MESSAGE_MS.info);
        }
      }),
      events.dictationError.listen(({ payload }) => {
        const v = errorMessage(payload.code, payload.message);
        flash(v, v.message?.tone === "error" ? MESSAGE_MS.error : MESSAGE_MS.info);
      }),
      events.dictationResult.listen(() => flash(view("inserted"), INSERTED_MS)),
      events.micLevel.listen(({ payload }) => {
        levels.current = payload.levels;
      }),
      events.overlayHover.listen(({ payload }) => setHovered(payload.hovered)),
    ];
    return () => offs.forEach((p) => void p.then((off) => off()));
  }, [flash]);

  // Demo levels: a gentle wave so the waveform moves in screenshots and fps runs.
  useEffect(() => {
    if (!demo) return;
    let frame = 0;
    const tick = (ts: number) => {
      levels.current = Array.from(
        { length: 16 },
        (_, i) => 0.35 + 0.35 * Math.sin(ts / 180 + i * 0.7),
      );
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, []);

  // The hands-free clock ticks once a second, only while recording hands-free.
  const recording = backend.state === "handsFree" || demo === "handsFree";
  useEffect(() => {
    if (!recording) return;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [recording]);

  const current: PillView = demo
    ? demo === "message"
      ? errorMessage("mic_silent", "No sound from the mic. Pick another one.")
      : view(demo as Exclude<PillKind, "message">)
    : (transient ?? stateView(backend.state, hovered));
  const resting = current.kind === "idle" || current.kind === "hover";
  const visible = demo !== null || !resting || (settings?.show_idle_bubble ?? true);

  // Presence: show at once; hide after the exit animation.
  useEffect(() => {
    if (demo || !settings) return;
    if (visible) {
      void commands.overlayPresence(true, resting);
      return;
    }
    const id = window.setTimeout(() => void commands.overlayPresence(false, true), EXIT_MS);
    return () => window.clearTimeout(id);
  }, [visible, resting, settings]);

  const reportBounds = useCallback((el: HTMLElement) => {
    if (demo) return;
    const r = el.getBoundingClientRect();
    void commands.setPillBounds({ x: r.x, y: r.y, width: r.width, height: r.height });
  }, []);

  const hotkey = displayHotkey(settings?.hotkey ?? "Ctrl+Super");
  const elapsedMs = backend.startedAt ? Math.max(0, now - backend.startedAt) : demo ? 42_000 : 0;

  return (
    // In demo mode a surface token stands in for the desktop behind the bubble.
    <main className={`flex h-full items-end justify-center pb-2 ${demo ? "bg-surface-3" : ""}`}>
      <span className="sr-only" aria-live="polite">
        {t(`pill.live.${current.kind}`, { defaultValue: "" })}
      </span>
      <AnimatePresence>
        {visible && (
          <Pill
            key="pill"
            view={current}
            hotkey={hotkey}
            levelsRef={levels}
            elapsedMs={elapsedMs}
            limitMs={backend.limitMs}
            onLayoutDone={reportBounds}
            onClick={() => void commands.startHandsFree()}
            onContextMenu={() => void commands.overlayMenu()}
          />
        )}
      </AnimatePresence>
    </main>
  );
}
