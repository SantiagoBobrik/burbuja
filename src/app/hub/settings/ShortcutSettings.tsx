import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button, Card, Input, Keys, Row } from "@/components/ui/kit";
import { commands, events } from "@/lib/ipc";
import { displayHotkey } from "@/lib/micDiagnosis";
import type { SettingsProps } from "./SettingsPage";

export function ShortcutSettings({ settings, update }: SettingsProps) {
  const { t } = useTranslation();
  const [recording, setRecording] = useState(false);
  const [conflict, setConflict] = useState<string | null>(null);

  useEffect(() => {
    const off = events.hotkeyRecorded.listen(({ payload }) => {
      setRecording(false);
      setConflict(payload.conflict ?? null);
      if (payload.hotkey) void update({ ...settings, hotkey: payload.hotkey });
    });
    return () => void off.then((f) => f());
  }, [settings, update]);

  // Leaving the page mid-recording must not leave the hotkey service capturing.
  useEffect(() => () => void commands.cancelRecordHotkey(), []);

  const toggleRecording = () => {
    if (recording) {
      void commands.cancelRecordHotkey();
      setRecording(false);
    } else {
      setConflict(null);
      void commands.recordHotkey().then((r) => setRecording(r.status === "ok"));
    }
  };

  const altGr = settings.hotkey === "RightAlt";

  return (
    <div className="flex flex-col gap-6">
      <Card className="flex flex-col items-start gap-4">
        <p className="text-text-2">{t("shortcut.current")}</p>
        <Keys combo={displayHotkey(settings.hotkey)} large />
        <div className="flex items-center gap-3">
          <Button variant={recording ? "danger" : "primary"} onClick={toggleRecording}>
            {recording ? t("shortcut.cancelRecording") : t("shortcut.record")}
          </Button>
          <p aria-live="polite" className="text-text-2">
            {recording && t("shortcut.recordingHint")}
          </p>
        </div>
        {(conflict || altGr) && (
          <p role="status" className="text-warn">
            {conflict ? t("shortcut.conflict", { what: conflict }) : t("shortcut.altGr")}
          </p>
        )}
      </Card>
      <div className="flex flex-col divide-y-2 divide-line">
        <Row label={t("shortcut.doubleTap")} hint={t("shortcut.doubleTapHint")}>
          {(id) => (
            <Input
              id={id}
              type="number"
              inputMode="numeric"
              min={150}
              max={800}
              step={10}
              value={settings.double_tap_window_ms}
              onChange={(e) => {
                const v = Number(e.target.value);
                if (v >= 150 && v <= 800) void update({ ...settings, double_tap_window_ms: v });
              }}
              className="w-24 text-right tabular-nums"
            />
          )}
        </Row>
        <Row label={t("shortcut.autoStop")} hint={t("shortcut.autoStopHint")}>
          {(id) => (
            <Input
              id={id}
              type="number"
              inputMode="numeric"
              min={1}
              max={60}
              value={settings.max_session_minutes}
              onChange={(e) => {
                const v = Number(e.target.value);
                if (v >= 1 && v <= 60) void update({ ...settings, max_session_minutes: v });
              }}
              className="w-24 text-right tabular-nums"
            />
          )}
        </Row>
      </div>
    </div>
  );
}
