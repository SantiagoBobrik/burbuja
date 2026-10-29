import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { barHeights, clock, ENDING_SOON_MS, type PillView } from "./pillModel";

/** The pill spring for size changes. */
const SPRING = { type: "spring", stiffness: 420, damping: 34, mass: 0.8 } as const;
const BAR_MAX = 18;

type Props = {
  view: PillView;
  hotkey: string;
  /** Latest meter bands; read every animation frame, never re-renders the pill. */
  levelsRef: React.RefObject<ReadonlyArray<number | null>>;
  elapsedMs: number;
  limitMs: number;
  onLayoutDone: (el: HTMLElement) => void;
  onClick: () => void;
  onContextMenu: () => void;
};

export function Pill({
  view,
  hotkey,
  levelsRef,
  elapsedMs,
  limitMs,
  onLayoutDone,
  onClick,
  onContextMenu,
}: Props) {
  const { t } = useTranslation();
  const reduced = useReducedMotion();
  const ref = useRef<HTMLButtonElement>(null);
  const round = view.kind === "idle" || view.kind === "inserted";
  const fade = { duration: reduced ? 0.1 : 0.12 };
  const label = ariaLabel(view, t);

  return (
    <motion.button
      ref={ref}
      type="button"
      aria-label={label}
      onClick={onClick}
      onContextMenu={(e) => {
        e.preventDefault();
        onContextMenu();
      }}
      initial={reduced ? { opacity: 0 } : { opacity: 0, scale: 0.9, y: 6 }}
      animate={{
        opacity: view.kind === "idle" ? 0.6 : 1,
        scale: 1,
        y: 0,
        width: view.width,
        height: view.height,
        borderRadius: 999,
      }}
      exit={
        reduced
          ? { opacity: 0, transition: { duration: 0.1 } }
          : {
              opacity: 0,
              scale: 0.9,
              y: 6,
              transition: { duration: 0.14, ease: [0.22, 1, 0.36, 1] },
            }
      }
      transition={reduced ? { duration: 0.1 } : { ...SPRING, opacity: fade }}
      onAnimationComplete={() => ref.current && onLayoutDone(ref.current)}
      className={`relative flex cursor-default items-center justify-center overflow-hidden border-2 border-pill-ring bg-pill outline-none ${round ? "bubble-float shadow-brut-sm-hover" : "shadow-brut-sm"}`}
      style={{ maxWidth: 360 }}
    >
      <AnimatePresence mode="popLayout" initial={false}>
        <motion.span
          key={view.kind === "message" ? `msg-${view.message?.key}` : view.kind}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={fade}
          className="flex items-center gap-2 whitespace-nowrap px-3"
        >
          <Content
            view={view}
            hotkey={hotkey}
            levelsRef={levelsRef}
            elapsedMs={elapsedMs}
            limitMs={limitMs}
          />
        </motion.span>
      </AnimatePresence>
    </motion.button>
  );
}

function Content({
  view,
  hotkey,
  levelsRef,
  elapsedMs,
  limitMs,
}: Omit<Props, "onLayoutDone" | "onClick" | "onContextMenu">) {
  const { t } = useTranslation();
  switch (view.kind) {
    case "idle":
    case "inserted":
      return <Highlight />;
    case "hover":
      return <span className="text-[12px] text-text-3">{t("pill.hold", { hotkey })}</span>;
    case "listening":
      return (
        <>
          <RecDot />
          <Waveform levelsRef={levelsRef} />
        </>
      );
    case "handsFree": {
      const remaining = limitMs - elapsedMs;
      return (
        <>
          <RecDot />
          <Waveform levelsRef={levelsRef} />
          <span
            className={`font-mono text-[12px] tabular-nums ${remaining <= ENDING_SOON_MS ? "text-warn" : "text-text-2"}`}
          >
            {clock(elapsedMs)}
          </span>
        </>
      );
    }
    case "processing":
      return (
        <span className="flex items-center gap-1.5" aria-hidden="true">
          {[0, 150, 300].map((delay) => (
            <span
              key={delay}
              className="breathe size-1.5 rounded-pill bg-accent"
              style={{ animationDelay: `${delay}ms` }}
            />
          ))}
        </span>
      );
    case "message":
      return (
        <>
          <span
            aria-hidden="true"
            className={`size-2 shrink-0 rounded-pill ${view.message?.tone === "error" ? "bg-coral" : "bg-text-3"}`}
          />
          <span className="truncate text-[12px] text-text-1">
            {view.message ? t(view.message.key, { defaultValue: view.message.fallback }) : ""}
          </span>
        </>
      );
  }
}

/** The brand's specular highlight on the resting bubble. */
function Highlight() {
  return (
    <span
      aria-hidden="true"
      className="absolute left-[2px] top-[2px] h-[3px] w-[5px] rounded-pill bg-text-1/55"
      style={{ transform: "rotate(-35deg)" }}
    />
  );
}

function RecDot() {
  return <span aria-hidden="true" className="rec-pulse size-2 shrink-0 rounded-pill bg-rec" />;
}

/** 9 bars (2.5 px wide, 2.5 px gap, radius 2, 4–18 px). Levels arrive at ~30 Hz and are
 * interpolated every frame by writing `transform` directly (no React renders, GPU-only). */
function Waveform({ levelsRef }: { levelsRef: React.RefObject<ReadonlyArray<number | null>> }) {
  const bars = useRef<(HTMLSpanElement | null)[]>([]);
  const reduced = useReducedMotion();

  useEffect(() => {
    const current = new Array(9).fill(4);
    let frame = 0;
    const tick = () => {
      const target = barHeights(levelsRef.current ?? []);
      for (let i = 0; i < 9; i++) {
        // Critically-damped-ish follow: fast but smooth at 60 fps.
        current[i] = reduced ? target[i] : current[i] + (target[i] - current[i]) * 0.35;
        const el = bars.current[i];
        if (el) el.style.transform = `scaleY(${current[i] / BAR_MAX})`;
      }
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [levelsRef, reduced]);

  return (
    <span className="flex h-[18px] items-center gap-[2.5px]" aria-hidden="true">
      {Array.from({ length: 9 }, (_, i) => (
        <span
          key={i}
          ref={(el) => {
            bars.current[i] = el;
          }}
          className="h-[18px] w-[2.5px] rounded-[2px] bg-text-1 will-change-transform"
          style={{ transform: `scaleY(${4 / BAR_MAX})` }}
        />
      ))}
    </span>
  );
}

function ariaLabel(view: PillView, t: (k: string, o?: Record<string, unknown>) => string): string {
  switch (view.kind) {
    case "listening":
    case "handsFree":
      return t("pill.aria.recording");
    case "processing":
      return t("pill.aria.processing");
    case "message":
      return view.message ? t(view.message.key, { defaultValue: view.message.fallback }) : "";
    default:
      return t("pill.aria.idle");
  }
}
