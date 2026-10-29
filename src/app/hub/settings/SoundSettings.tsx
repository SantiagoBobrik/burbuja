import { Play } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button, Row, Switch } from "@/components/ui/kit";
import { commands, type SoundKind } from "@/lib/ipc";
import type { SettingsProps } from "./SettingsPage";

export function SoundSettings({ settings, update }: SettingsProps) {
  const { t, i18n } = useTranslation();
  const volume = settings.sound_volume ?? 0.6;
  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col divide-y-2 divide-line">
        <Row label={t("sounds.enabled")}>
          {() => (
            <Switch
              label={t("sounds.enabled")}
              checked={settings.sounds_enabled}
              onChange={(v) => void update({ ...settings, sounds_enabled: v })}
            />
          )}
        </Row>
        <Row label={t("sounds.volume")}>
          {(id) => (
            <div className="flex items-center gap-3">
              <input
                id={id}
                type="range"
                min={0}
                max={1}
                step={0.05}
                value={volume}
                disabled={!settings.sounds_enabled}
                onChange={(e) => void update({ ...settings, sound_volume: Number(e.target.value) })}
                className="w-44 accent-accent"
              />
              <span className="w-10 text-right tabular-nums text-text-2">
                {new Intl.NumberFormat(i18n.language, { style: "percent" }).format(volume)}
              </span>
            </div>
          )}
        </Row>
        {/* Previews are labelled as such: a bare "Cancel" button reads as an action. */}
        <Row stack label={t("sounds.preview")} hint={t("sounds.previewHint")}>
          {() => (
            <div className="flex gap-2">
              {(["start", "stop", "cancel"] as SoundKind[]).map((k) => (
                <Button
                  key={k}
                  size="sm"
                  aria-label={t("sounds.play", { sound: t(`sounds.${k}`) })}
                  onClick={() => void commands.playSound(k)}
                  disabled={!settings.sounds_enabled}
                >
                  <Play size={12} aria-hidden="true" />
                  {t(`sounds.${k}`)}
                </Button>
              ))}
            </div>
          )}
        </Row>
      </div>
    </div>
  );
}
