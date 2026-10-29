import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button, Card, Row, SectionTitle, Segmented, Switch } from "@/components/ui/kit";
import { commands, events, type DictationTimings, type InsertMethod } from "@/lib/ipc";
import type { SettingsProps } from "./SettingsPage";

/** Pipeline stages in the order they run. `total` (release → inserted) closes the list. */
const STAGES = [
  ["hotkeyToRecord", "hotkeyToRecordMs"],
  ["releaseToStop", "releaseToStopMs"],
  ["vad", "vadMs"],
  ["transcribe", "transcribeMs"],
  ["cleanup", "cleanupMs"],
  ["insert", "insertMs"],
] as const;

/** "412 ms" below a second, "1.17 s" above; decimals only where they carry information. */
function duration(ms: number, lang: string): string {
  if (ms >= 1000) {
    return new Intl.NumberFormat(lang, {
      style: "unit",
      unit: "second",
      maximumFractionDigits: ms >= 10_000 ? 1 : 2,
    }).format(ms / 1000);
  }
  return new Intl.NumberFormat(lang, {
    style: "unit",
    unit: "millisecond",
    maximumFractionDigits: ms < 10 ? 1 : 0,
  }).format(ms);
}

/** Where the time of the last dictation went. Read-only diagnostics. */
function LastTimings() {
  const { t, i18n } = useTranslation();
  const [timings, setTimings] = useState<DictationTimings | null>(null);

  useEffect(() => {
    void commands.getLastTimings().then(setTimings);
    const off = events.dictationTimings.listen(({ payload }) => setTimings(payload));
    return () => void off.then((f) => f());
  }, []);

  const rows = timings
    ? STAGES.flatMap(([key, field]) => {
        const v = timings[field];
        return v === null || v === undefined ? [] : [{ key, ms: v }];
      })
    : [];

  return (
    <section aria-labelledby="timings-title" className="flex flex-col gap-3">
      <div>
        <SectionTitle id="timings-title">{t("advanced.timings")}</SectionTitle>
        {timings && (
          <p className="text-text-3" translate="no">
            {timings.engine}
            {timings.audioMs !== null &&
              ` · ${t("advanced.timing.audio")} ${duration(timings.audioMs, i18n.language)}`}
          </p>
        )}
      </div>
      {!timings ? (
        <Card flat>
          <p className="text-text-2">{t("advanced.timingsEmpty")}</p>
        </Card>
      ) : (
        <dl className="overflow-hidden rounded-md border-2 border-line-strong bg-surface-1">
          {rows.map((r, i) => (
            <div
              key={r.key}
              className={`flex items-baseline justify-between gap-4 px-4 py-2 ${i > 0 ? "border-t-2 border-line" : ""}`}
            >
              <dt className="text-text-2">{t(`advanced.timing.${r.key}`)}</dt>
              <dd className="font-mono text-[12px] tabular-nums">
                {duration(r.ms, i18n.language)}
              </dd>
            </div>
          ))}
          {timings.totalMs !== null && (
            <div
              className={`flex items-baseline justify-between gap-4 bg-surface-2 px-4 py-2.5 ${rows.length > 0 ? "border-t-2 border-line-strong" : ""}`}
            >
              <dt className="font-bold">{t("advanced.timing.total")}</dt>
              <dd className="font-mono text-[15px] font-semibold tabular-nums">
                {duration(timings.totalMs, i18n.language)}
              </dd>
            </div>
          )}
        </dl>
      )}
    </section>
  );
}

export function AdvancedSettings({ settings, update }: SettingsProps) {
  const { t } = useTranslation();
  const [confirmReset, setConfirmReset] = useState(false);

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col divide-y-2 divide-line">
        <Row label={t("providers.insertMethod")} hint={t("advanced.insertHint")}>
          {() => (
            <Segmented<InsertMethod>
              label={t("providers.insertMethod")}
              value={settings.insert_method}
              onChange={(v) => void update({ ...settings, insert_method: v })}
              options={[
                { value: "paste", label: t("providers.paste") },
                { value: "type", label: t("providers.type") },
              ]}
            />
          )}
        </Row>
        <Row label={t("providers.restoreClipboard")} hint={t("advanced.restoreHint")}>
          {() => (
            <Switch
              label={t("providers.restoreClipboard")}
              checked={settings.restore_clipboard}
              onChange={(v) => void update({ ...settings, restore_clipboard: v })}
            />
          )}
        </Row>
        <Row label={t("advanced.keepAudio")} hint={t("advanced.keepAudioHint")}>
          {() => (
            <Switch
              label={t("advanced.keepAudio")}
              checked={settings.keep_audio}
              onChange={(v) => void update({ ...settings, keep_audio: v })}
            />
          )}
        </Row>
      </div>

      <LastTimings />

      <div className="flex flex-wrap gap-3">
        <Button onClick={() => void commands.openLogsFolder()}>{t("advanced.logs")}</Button>
        <Button variant="danger" onClick={() => setConfirmReset(true)}>
          {t("advanced.reset")}
        </Button>
      </div>

      {confirmReset && (
        <Card flat className="flex items-center justify-between gap-4">
          <p role="alert">{t("advanced.confirmReset")}</p>
          <div className="flex gap-2">
            <Button size="sm" onClick={() => setConfirmReset(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              size="sm"
              variant="danger"
              onClick={() => {
                setConfirmReset(false);
                void commands.resetSettings();
              }}
            >
              {t("advanced.reset")}
            </Button>
          </div>
        </Card>
      )}
    </div>
  );
}
