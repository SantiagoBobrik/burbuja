import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button, Select } from "@/components/ui/kit";
import { commands, events, type MicInfo } from "@/lib/ipc";
import { diagnoseMic, type MicDiagnosis } from "@/lib/micDiagnosis";
import { refreshPermissions, usePermissions } from "@/lib/permissions";
import type { SettingsProps } from "./SettingsPage";

/** Mic picker + live meter + silent-mic diagnostic. Also used by onboarding (`onHeard`). */
export function MicrophoneSettings({
  settings,
  update,
  onHeard,
}: SettingsProps & { onHeard?: () => void }) {
  const { t } = useTranslation();
  const [mics, setMics] = useState<MicInfo[]>([]);
  const [level, setLevel] = useState(0);
  const [diagnosis, setDiagnosis] = useState<MicDiagnosis | "unavailable">("listening");
  const permissions = usePermissions();
  // A blocked microphone opens fine but stays silent: say why instead of "pick another one".
  const blocked = !!permissions?.required && permissions.microphone !== "granted";
  const noDevice = permissions?.hasMicrophone === false;
  const openedAt = useRef(0);
  const peak = useRef(0);

  useEffect(() => {
    void commands.listMicrophones().then((r) => r.status === "ok" && setMics(r.data));
  }, []);

  // Open the chosen mic for the meter while visible. The parent keys this component by
  // microphone, so a new choice starts from fresh state. Reopened when access is granted: a
  // stream opened while blocked stays silent.
  useEffect(() => {
    let alive = true;
    openedAt.current = performance.now();
    const unlisten = events.micLevel.listen(({ payload }) => {
      if (!alive) return;
      peak.current = Math.max(peak.current, payload.peak ?? 0);
      setLevel(Math.max(...payload.levels.map((l) => l ?? 0)));
      const d = diagnoseMic(peak.current, performance.now() - openedAt.current);
      setDiagnosis(d);
      if (d === "hearing") onHeard?.();
    });
    void commands.startMicPreview(settings.microphone).then((r) => {
      if (alive && r.status === "error") setDiagnosis("unavailable");
    });
    const timer = window.setTimeout(() => {
      if (alive && peak.current === 0) setDiagnosis((d) => (d === "listening" ? "noSignal" : d));
    }, 1600);
    return () => {
      alive = false;
      window.clearTimeout(timer);
      void unlisten.then((off) => off());
      void commands.stopMicPreview();
    };
  }, [settings.microphone, onHeard, blocked]);

  const tone =
    diagnosis === "hearing" ? "text-ok" : diagnosis === "listening" ? "text-text-3" : "text-warn";
  const pct = Math.round(Math.min(1, Math.max(0, level)) * 100);

  return (
    <div className="flex flex-col gap-4">
      <label className="flex flex-col gap-2">
        <span className="font-medium">{t("mic.device")}</span>
        <Select
          name="microphone"
          value={settings.microphone}
          onChange={(e) => void update({ ...settings, microphone: e.target.value })}
        >
          <option value="">{t("mic.systemDefault")}</option>
          {mics.map((m) => (
            <option key={m.name} value={m.name}>
              {m.isDefault ? `${m.name} (${t("mic.defaultSuffix")})` : m.name}
            </option>
          ))}
        </Select>
      </label>
      <div
        role="meter"
        aria-label={t("mic.title")}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct}
        className="h-4 overflow-hidden rounded-pill border-2 border-line-strong bg-surface-2"
      >
        <div
          className="h-full w-full origin-left bg-accent transition-transform duration-75 ease-out"
          style={{ transform: `scaleX(${pct / 100})` }}
        />
      </div>
      {blocked || noDevice ? (
        <div aria-live="polite" className="flex min-h-[2.9em] items-start gap-4 text-warn">
          <p className="min-w-0 flex-1">{t(blocked ? "mic.blocked" : "mic.noDevice")}</p>
          {blocked && (
            <Button
              variant="primary"
              onClick={() =>
                void commands.requestPermission("microphone").then(() => refreshPermissions())
              }
            >
              {t("mic.allowAccess")}
            </Button>
          )}
        </div>
      ) : (
        <p aria-live="polite" className={`min-h-[2.9em] ${tone}`}>
          {t(`mic.${diagnosis}`)}
        </p>
      )}
    </div>
  );
}
