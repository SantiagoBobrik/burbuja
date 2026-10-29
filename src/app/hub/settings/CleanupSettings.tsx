import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button, Card, Input, Row, Segmented, Select, TextArea } from "@/components/ui/kit";
import {
  commands,
  type CleanupProvider,
  type CleanupStyle,
  type LocalLlms,
  type ProviderTest,
} from "@/lib/ipc";
import type { SettingsProps } from "./SettingsPage";

/** Local servers only: Burbuja never sends text off this computer. */
const CLEANUPS: CleanupProvider[] = ["off", "ollama", "lmstudio"];
/** Presets change capitalization and punctuation only; "custom" is the user's own prompt. */
const STYLES: CleanupStyle[] = ["standard", "formal", "casual", "very_casual", "excited", "custom"];

export function CleanupSettings({ settings, update }: SettingsProps) {
  const { t } = useTranslation();
  const [test, setTest] = useState<ProviderTest | "running" | "failed" | null>(null);
  const [local, setLocal] = useState<LocalLlms | null>(null);
  const isLocal = settings.cleanup !== "off";

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col divide-y-2 divide-line">
        <Row stack label={t("cleanup.provider")} hint={t("cleanup.providerHint")}>
          {() => (
            <Segmented<CleanupProvider>
              label={t("cleanup.provider")}
              value={settings.cleanup}
              onChange={(v) => {
                setTest(null);
                setLocal(null);
                void update({ ...settings, cleanup: v });
              }}
              options={CLEANUPS.map((v) => ({ value: v, label: t(`providers.cleanups.${v}`) }))}
            />
          )}
        </Row>
        {isLocal && (
          <Row stack label={t("cleanup.model")} hint={t("cleanup.modelHint")}>
            {(id) => (
              <Input
                id={id}
                name="cleanup-model"
                autoComplete="off"
                spellCheck={false}
                placeholder={t("cleanup.modelPlaceholder")}
                value={settings.cleanup_model}
                onChange={(e) => void update({ ...settings, cleanup_model: e.target.value })}
                className="w-56"
              />
            )}
          </Row>
        )}
        {isLocal && (
          <Row stack label={t("cleanup.style")} hint={t("cleanup.styleHint")}>
            {(id) => (
              <div className="flex flex-col gap-3">
                <Select
                  id={id}
                  value={settings.cleanup_style}
                  onChange={(e) => {
                    setTest(null);
                    void update({ ...settings, cleanup_style: e.target.value as CleanupStyle });
                  }}
                  className="w-56"
                >
                  {STYLES.map((s) => (
                    <option key={s} value={s}>
                      {t(`cleanup.styles.${s}`)}
                    </option>
                  ))}
                </Select>
                {settings.cleanup_style === "custom" ? (
                  <>
                    <TextArea
                      aria-label={t("cleanup.customPrompt")}
                      name="cleanup-custom-prompt"
                      spellCheck={false}
                      maxLength={4000}
                      placeholder={t("cleanup.customPlaceholder")}
                      value={settings.cleanup_custom_prompt}
                      onChange={(e) =>
                        void update({ ...settings, cleanup_custom_prompt: e.target.value })
                      }
                    />
                    <p className="text-text-3">{t("cleanup.customHint")}</p>
                  </>
                ) : (
                  <p className="text-text-2">
                    <span className="text-text-3">{t("cleanup.example")} </span>
                    {t(`cleanup.examples.${settings.cleanup_style}`)}
                  </p>
                )}
              </div>
            )}
          </Row>
        )}
      </div>

      {isLocal && (
        <div className="flex flex-wrap items-center gap-3">
          <Button
            variant="primary"
            onClick={() => {
              setTest("running");
              void commands
                .testProvider()
                .then((r) => setTest(r.status === "ok" ? r.data : "failed"));
            }}
          >
            {t("providers.test")}
          </Button>
          <Button onClick={() => void commands.detectLocalLlms().then(setLocal)}>
            {t("cleanup.detect")}
          </Button>
        </div>
      )}
      <div aria-live="polite">
        {test === "running" && <p className="text-text-3">{t("providers.testing")}</p>}
        {test === "failed" && <p className="text-warn">{t("providers.testFailed")}</p>}
        {test && typeof test === "object" && (
          <Card flat className="flex flex-col gap-2">
            <p className="text-text-3 tabular-nums">
              {test.provider} · {test.ms} ms
            </p>
            <p className="text-text-2 line-through decoration-coral">“{test.before}”</p>
            <p className="font-bold">
              {test.after
                ? `“${test.after}”`
                : test.error
                  ? t(`pill.errors.${test.error}`)
                  : t("providers.testRaw")}
            </p>
          </Card>
        )}
        {local && (
          <p className="text-text-2">
            Ollama: {local.ollama ? local.ollama.join(", ") || "—" : t("providers.notRunning")} · LM
            Studio: {local.lmstudio ? local.lmstudio.join(", ") || "—" : t("providers.notRunning")}
          </p>
        )}
      </div>
    </div>
  );
}
