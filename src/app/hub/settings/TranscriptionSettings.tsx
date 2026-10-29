import { MoreHorizontal } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button, IconButton, Input, Row, SectionTitle, Select, Switch } from "@/components/ui/kit";
import { commands, events, type ModelInfo } from "@/lib/ipc";
import type { SettingsProps } from "./SettingsPage";

const LANGUAGES = ["auto", "es", "en", "pt", "fr", "de", "it"];

export function TranscriptionSettings({ settings, update }: SettingsProps) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-col gap-6">
      <section aria-labelledby="models-title" className="flex flex-col gap-3">
        <div>
          <SectionTitle id="models-title">{t("models.title")}</SectionTitle>
          <p className="text-text-3">{t("transcription.engineHint")}</p>
        </div>
        <ModelCards />
      </section>
      <div className="flex flex-col divide-y-2 divide-line">
        <Row label={t("transcription.gpu")} hint={t("transcription.gpuHint")}>
          {() => (
            <Switch
              label={t("transcription.gpu")}
              checked={settings.use_gpu}
              onChange={(v) => void update({ ...settings, use_gpu: v })}
            />
          )}
        </Row>
        <Row label={t("transcription.language")}>
          {(id) => (
            <Select
              id={id}
              value={settings.language}
              onChange={(e) => void update({ ...settings, language: e.target.value })}
            >
              {LANGUAGES.map((l) => (
                <option key={l} value={l}>
                  {l === "auto"
                    ? t("transcription.autoDetect")
                    : new Intl.DisplayNames([navigator.language], { type: "language" }).of(l)}
                </option>
              ))}
            </Select>
          )}
        </Row>
        <Row label={t("transcription.unload")} hint={t("transcription.unloadHint")}>
          {(id) => (
            <Input
              id={id}
              type="number"
              inputMode="numeric"
              min={0}
              max={1440}
              value={settings.unload_after_idle_minutes}
              onChange={(e) => {
                const v = Number(e.target.value);
                if (v >= 0 && v <= 1440) void update({ ...settings, unload_after_idle_minutes: v });
              }}
              className="w-24 text-right tabular-nums"
            />
          )}
        </Row>
      </div>
    </div>
  );
}

const TAGS: Record<string, string> = {
  "whisper-large-v3-turbo": "transcription.tags.best",
  "parakeet-tdt-0.6b-v3": "transcription.tags.cpu",
  "whisper-base.en": "transcription.tags.light",
};

/** Model cards: name, size, tag, Download / Downloading n% / Active / Use, menu. */
function ModelCards() {
  const { t, i18n } = useTranslation();
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [progress, setProgress] = useState<Record<string, number>>({});
  const [menu, setMenu] = useState<string | null>(null);

  const refresh = useCallback(() => void commands.listModels().then(setModels), []);
  useEffect(() => {
    refresh();
    const off = events.modelDownloadProgress.listen(({ payload }) => {
      if (payload.status === "downloading") {
        setProgress((p) => ({ ...p, [payload.id]: (payload.bytes ?? 0) / (payload.total || 1) }));
      } else {
        setProgress((p) =>
          Object.fromEntries(Object.entries(p).filter(([id]) => id !== payload.id)),
        );
        refresh();
      }
    });
    return () => void off.then((f) => f());
  }, [refresh]);

  const mb = (b: number | null) =>
    new Intl.NumberFormat(i18n.language, { maximumFractionDigits: 0 }).format((b ?? 0) / 1e6);

  return (
    <ul className="flex flex-col gap-3" aria-label={t("models.title")}>
      {models.map((m) => {
        const pct = progress[m.id];
        const downloading = pct !== undefined || m.downloading;
        return (
          <li
            key={m.id}
            className={`relative flex items-center gap-4 rounded-md border-2 border-line-strong bg-surface-1 px-4 py-3 ${m.active ? "shadow-brut-md" : ""}`}
          >
            <div className="min-w-0 flex-1">
              <p className="font-extrabold">{m.name}</p>
              <p className="text-text-3 tabular-nums">
                {mb(m.sizeBytes)} MB · {t(TAGS[m.id] ?? "transcription.tags.light")}
              </p>
            </div>
            {downloading ? (
              <>
                <span className="w-14 text-right font-bold tabular-nums">
                  {Math.round((pct ?? 0) * 100)}%
                </span>
                <Button size="sm" onClick={() => void commands.cancelDownload(m.id)}>
                  {t("models.cancel")}
                </Button>
              </>
            ) : !m.downloaded ? (
              <Button
                size="sm"
                variant="primary"
                onClick={() =>
                  void commands
                    .downloadModel(m.id)
                    .then(() => setProgress((p) => ({ ...p, [m.id]: 0 })))
                }
              >
                {t("models.download")}
              </Button>
            ) : m.active ? (
              <span className="rounded-pill border-2 border-line-strong bg-teal px-3 py-0.5 text-[12px] font-bold text-on-accent">
                {t("models.active")}
              </span>
            ) : (
              <Button size="sm" onClick={() => void commands.setActiveModel(m.id).then(refresh)}>
                {t("models.use")}
              </Button>
            )}
            {m.downloaded && !downloading && (
              <div className="relative">
                <IconButton
                  aria-label={t("models.more", { name: m.name })}
                  aria-expanded={menu === m.id}
                  onClick={() => setMenu(menu === m.id ? null : m.id)}
                >
                  <MoreHorizontal size={16} aria-hidden="true" />
                </IconButton>
                {menu === m.id && (
                  <div className="absolute top-9 right-0 z-10 rounded-md border-2 border-line-strong bg-surface-3 p-1 shadow-brut-sm">
                    <button
                      type="button"
                      className="h-8 rounded-sm px-3 font-bold whitespace-nowrap text-coral hover:bg-surface-2"
                      onClick={() => {
                        setMenu(null);
                        void commands.deleteModel(m.id).then(refresh);
                      }}
                    >
                      {t("models.delete")}
                    </button>
                  </div>
                )}
              </div>
            )}
          </li>
        );
      })}
    </ul>
  );
}
