import { motion, useReducedMotion } from "motion/react";
import { useTranslation } from "react-i18next";
import type { Settings_Serialize } from "@/lib/ipc";
import { PendingBadge } from "@/components/ui/kit";
import { permissionIssues, usePermissions } from "@/lib/permissions";
import type { UpdateSettings } from "../useSettings";
import { AboutSettings } from "./AboutSettings";
import { AdvancedSettings } from "./AdvancedSettings";
import { CleanupSettings } from "./CleanupSettings";
import { GeneralSettings } from "./GeneralSettings";
import { MicrophoneSettings } from "./MicrophoneSettings";
import { PermissionsSettings } from "./PermissionsSettings";
import { ShortcutSettings } from "./ShortcutSettings";
import { SoundSettings } from "./SoundSettings";
import { TranscriptionSettings } from "./TranscriptionSettings";

const SECTIONS = [
  "general",
  "permissions",
  "shortcut",
  "microphone",
  "transcription",
  "cleanup",
  "sounds",
  "advanced",
  "about",
] as const;
export type Section = (typeof SECTIONS)[number];

export type SettingsProps = { settings: Settings_Serialize; update: UpdateSettings };

export function SettingsPage({
  section,
  onSection,
  ...props
}: SettingsProps & { section: Section; onSection: (s: Section) => void }) {
  const { t } = useTranslation();
  const reduced = useReducedMotion();
  const permissions = usePermissions();
  const pending = permissions ? permissionIssues(permissions).length : 0;

  return (
    <div className="flex flex-col gap-5">
      <h1 className="text-[22px] font-extrabold tracking-[-0.02em]">{t("nav.settings")}</h1>
      <div className="flex gap-8">
        <nav aria-label={t("settings.label")} className="w-40 shrink-0">
          <ul className="flex flex-col gap-1">
            {SECTIONS.map((s) => (
              <li key={s}>
                <button
                  type="button"
                  aria-current={s === section ? "page" : undefined}
                  onClick={() => onSection(s)}
                  className={`flex h-8 w-full items-center justify-between gap-2 rounded-sm border-2 px-2.5 text-left font-bold transition-[background-color,border-color,color] duration-100 ${
                    s === section
                      ? "border-line-strong bg-surface-2 text-text-1"
                      : "border-transparent text-text-2 hover:border-line hover:text-text-1"
                  }`}
                >
                  {t(`settings.sections.${s}`)}
                  {s === "permissions" && pending > 0 && (
                    <PendingBadge count={pending} label={t("permissions.pending")} />
                  )}
                </button>
              </li>
            ))}
          </ul>
        </nav>
        <motion.section
          key={section}
          aria-label={t(`settings.sections.${section}`)}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: reduced ? 0.1 : 0.14 }}
          className="min-w-0 flex-1"
        >
          {section === "general" && <GeneralSettings {...props} />}
          {section === "permissions" && <PermissionsSettings {...props} />}
          {section === "shortcut" && <ShortcutSettings {...props} />}
          {section === "microphone" && (
            <MicrophoneSettings key={props.settings.microphone} {...props} />
          )}
          {section === "transcription" && <TranscriptionSettings {...props} />}
          {section === "cleanup" && <CleanupSettings {...props} />}
          {section === "sounds" && <SoundSettings {...props} />}
          {section === "advanced" && <AdvancedSettings {...props} />}
          {section === "about" && <AboutSettings />}
        </motion.section>
      </div>
    </div>
  );
}
