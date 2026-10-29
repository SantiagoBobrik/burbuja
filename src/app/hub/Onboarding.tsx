import { Check } from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button, Card, Keys, Star } from "@/components/ui/kit";
import { commands, events, type ModelInfo, type Settings_Serialize } from "@/lib/ipc";
import { displayHotkey } from "@/lib/micDiagnosis";
import { devParam } from "./App";
import { usePermissions } from "@/lib/permissions";
import { missingPermissions, PermissionsPanel } from "./PermissionsPanel";
import { MicrophoneSettings } from "./settings/MicrophoneSettings";
import type { UpdateSettings } from "./useSettings";

const STEPS = ["welcome", "access", "microphone", "model"] as const;

export function Onboarding({
  settings,
  update,
}: {
  settings: Settings_Serialize;
  update: UpdateSettings;
}) {
  const { t } = useTranslation();
  const [step, setStep] = useState(() => (import.meta.env.DEV ? Number(devParam("step") ?? 0) : 0));
  const reduced = useReducedMotion();
  const next = () => setStep((s) => Math.min(s + 1, STEPS.length - 1));
  const finish = () => void update({ ...settings, onboarding_completed: true });

  return (
    <div className="relative flex h-full flex-col items-center justify-center overflow-hidden px-10 pt-10 pb-20">
      <Star className="absolute top-10 left-14 size-8" />
      <Star className="absolute right-16 bottom-16 size-6" color="var(--iris)" />
      <Star className="absolute top-24 right-28 size-4" color="var(--pink)" />

      <AnimatePresence mode="wait">
        <motion.div
          key={step}
          initial={{ opacity: 0, y: reduced ? 0 : 8 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: reduced ? 0 : -8 }}
          transition={{ duration: reduced ? 0.1 : 0.18, ease: [0.22, 1, 0.36, 1] }}
          className="w-full max-w-[480px]"
        >
          {STEPS[step] === "welcome" && <Welcome onNext={next} />}
          {STEPS[step] === "access" && <Access onNext={next} />}
          {STEPS[step] === "microphone" && (
            <MicStep settings={settings} update={update} onNext={next} />
          )}
          {STEPS[step] === "model" && <ModelStep hotkey={settings.hotkey} onFinish={finish} />}
        </motion.div>
      </AnimatePresence>

      {/* Pinned, so the progress doesn't jump as steps change height. */}
      <ol
        className="absolute bottom-10 left-1/2 flex -translate-x-1/2 gap-2"
        aria-label={t("onboarding.progress", { step: step + 1, total: STEPS.length })}
      >
        {STEPS.map((s, i) => (
          <li
            key={s}
            aria-current={i === step ? "step" : undefined}
            className={`h-2.5 rounded-pill border-2 border-line-strong transition-[width,background-color] duration-200 ${i === step ? "w-7 bg-accent" : i < step ? "w-2.5 bg-text-1" : "w-2.5 bg-surface-2"}`}
          />
        ))}
      </ol>
    </div>
  );
}

function Welcome({ onNext }: { onNext: () => void }) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-col items-center gap-6 text-center">
      <span
        aria-hidden="true"
        className="relative size-20 rounded-pill border-2 border-line-strong bg-accent shadow-brut-lg"
      >
        <span className="absolute top-3 left-4 h-3 w-6 -rotate-[35deg] rounded-pill bg-text-1/90" />
      </span>
      <div>
        <h1 className="text-[34px] font-extrabold tracking-[-0.02em]" translate="no">
          burbuja
        </h1>
        <p className="text-[15px] text-text-2">{t("onboarding.welcome")}</p>
      </div>
      <Button variant="primary" onClick={onNext} className="h-11 px-6 text-[15px]">
        {t("onboarding.start")}
      </Button>
    </div>
  );
}

/**
 * macOS grants each permission per app (with its own prompt the first time); Windows has only the
 * privacy switches, which we read so the user sees whether the microphone is really allowed.
 */
function Access({ onNext }: { onNext: () => void }) {
  const { t } = useTranslation();
  const permissions = usePermissions();
  // Nothing to ask for on this OS: skip the step.
  const skip = permissions !== null && !permissions.required;
  useEffect(() => {
    if (skip) onNext();
  }, [skip, onNext]);
  if (!permissions || skip) return null;
  const missing = missingPermissions(permissions);
  const windows = permissions.platform === "windows";
  return (
    <div className="flex flex-col gap-5">
      <h1 className="text-[24px] font-extrabold">
        {t(windows ? "onboarding.accessTitle" : "permissions.title")}
      </h1>
      <p className="text-text-2">{t(windows ? "permissions.bodyWindows" : "permissions.body")}</p>
      <PermissionsPanel permissions={permissions} />
      <div className="flex items-center gap-4">
        <Button variant={missing ? "ghost" : "primary"} onClick={onNext}>
          {t(missing ? "onboarding.continueAnyway" : "onboarding.continue")}
        </Button>
        {missing && <p className="text-text-3">{t("onboarding.pendingLater")}</p>}
      </div>
    </div>
  );
}

function MicStep({
  settings,
  update,
  onNext,
}: {
  settings: Settings_Serialize;
  update: UpdateSettings;
  onNext: () => void;
}) {
  const { t } = useTranslation();
  const [heard, setHeard] = useState(false);
  const onHeard = useCallback(() => setHeard(true), []);
  return (
    <div className="flex flex-col gap-5">
      <h1 className="text-[24px] font-extrabold">{t("onboarding.micTitle")}</h1>
      <p className="text-text-2">{t("onboarding.micBody")}</p>
      <MicrophoneSettings
        key={settings.microphone}
        settings={settings}
        update={update}
        onHeard={onHeard}
      />
      <div className="flex items-center gap-4">
        <Button variant="primary" disabled={!heard} onClick={onNext}>
          {t("onboarding.continue")}
        </Button>
        <button
          type="button"
          className="text-text-3 underline-offset-4 hover:text-text-1 hover:underline"
          onClick={onNext}
        >
          {t("onboarding.skip")}
        </button>
      </div>
    </div>
  );
}

function ModelStep({ hotkey, onFinish }: { hotkey: string; onFinish: () => void }) {
  const { t, i18n } = useTranslation();
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [pct, setPct] = useState<number | null>(null);
  const [said, setSaid] = useState<string | null>(null);

  const refresh = useCallback(() => void commands.listModels().then(setModels), []);
  useEffect(() => {
    refresh();
    const offs = [
      events.modelDownloadProgress.listen(({ payload }) => {
        if (payload.status === "downloading") setPct((payload.bytes ?? 0) / (payload.total || 1));
        else {
          setPct(null);
          refresh();
        }
      }),
      events.dictationResult.listen(({ payload }) => setSaid(payload.text)),
    ];
    return () => offs.forEach((p) => void p.then((f) => f()));
  }, [refresh]);

  // Recommended: large-v3-turbo (GPU via Vulkan on Windows x64); Parakeet is the CPU fallback.
  const recommended = models.find((m) => m.id === "whisper-large-v3-turbo");
  const ready = recommended?.downloaded ?? false;
  const mb = (b: number | null) =>
    new Intl.NumberFormat(i18n.language, { maximumFractionDigits: 0 }).format((b ?? 0) / 1e6);

  return (
    <div className="flex flex-col gap-5">
      <h1 className="text-[24px] font-extrabold">{t("onboarding.modelTitle")}</h1>
      {recommended && (
        <Card className="flex items-center gap-4">
          <div className="min-w-0 flex-1">
            <p className="font-extrabold">{recommended.name}</p>
            <p className="text-text-3 tabular-nums">
              {mb(recommended.sizeBytes)} MB · {t("transcription.tags.best")}
            </p>
          </div>
          {ready ? (
            <span className="flex items-center gap-1 font-bold text-ok">
              <Check size={16} aria-hidden="true" /> {t("onboarding.ready")}
            </span>
          ) : pct !== null ? (
            <span className="font-bold tabular-nums">{Math.round(pct * 100)}%</span>
          ) : (
            <Button
              variant="primary"
              onClick={() => void commands.downloadModel(recommended.id).then(() => setPct(0))}
            >
              {t("models.download")}
            </Button>
          )}
        </Card>
      )}
      {pct !== null && (
        <div
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(pct * 100)}
          className="h-4 overflow-hidden rounded-pill border-2 border-line-strong bg-surface-2"
        >
          <div
            className="h-full origin-left bg-accent transition-transform duration-200"
            style={{ transform: `scaleX(${pct})` }}
          />
        </div>
      )}
      {ready && (
        <div className="flex flex-col gap-3">
          <p className="text-[15px] font-bold">
            {t("onboarding.tryBefore")} <Keys combo={displayHotkey(hotkey)} />{" "}
            {t("onboarding.tryAfter")}
          </p>
          <textarea
            autoFocus
            aria-label={t("home.tryLabel")}
            className="min-h-20 rounded-md border-2 border-line-strong bg-surface-2 p-3 text-[14px] text-text-1 shadow-brut-sm transition-[box-shadow,border-color] duration-100 focus:border-accent focus:shadow-[3px_3px_0_0_var(--accent)] focus:outline-none"
          />
          {said && (
            <p role="status" className="flex items-center gap-2 font-bold text-ok">
              <Check size={16} aria-hidden="true" /> {t("onboarding.itWorks")}
            </p>
          )}
          <p className="text-text-3">{t("onboarding.trayTip")}</p>
        </div>
      )}
      <Button variant={said ? "primary" : "ghost"} className="self-start" onClick={onFinish}>
        {said ? t("onboarding.finish") : t("onboarding.later")}
      </Button>
    </div>
  );
}
