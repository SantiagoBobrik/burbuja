import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Row, Segmented, Switch } from "@/components/ui/kit";
import { commands, type UiLanguage } from "@/lib/ipc";
import type { SettingsProps } from "./SettingsPage";

export function GeneralSettings({ settings, update }: SettingsProps) {
  const { t } = useTranslation();
  // `null` until the OS answers, so the switch doesn't animate from "off" on every visit.
  const [autostart, setAutostart] = useState<boolean | null>(null);

  useEffect(() => {
    void commands.getAutostart().then(setAutostart);
  }, []);

  return (
    <div className="flex flex-col divide-y-2 divide-line">
      <Row label={t("general.launchAtLogin")} hint={t("general.launchAtLoginHint")}>
        {() =>
          autostart === null ? (
            <span aria-hidden="true" className="block h-6 w-11" />
          ) : (
            <Switch
              label={t("general.launchAtLogin")}
              checked={autostart}
              onChange={(v) =>
                void commands.setAutostart(v).then((r) => r.status === "ok" && setAutostart(r.data))
              }
            />
          )
        }
      </Row>
      <Row label={t("general.idleBubble")} hint={t("general.idleBubbleHint")}>
        {() => (
          <Switch
            label={t("general.idleBubble")}
            checked={settings.show_idle_bubble}
            onChange={(v) => void update({ ...settings, show_idle_bubble: v })}
          />
        )}
      </Row>
      <Row label={t("general.language")}>
        {() => (
          <Segmented<UiLanguage>
            label={t("general.language")}
            value={settings.ui_language}
            onChange={(v) => void update({ ...settings, ui_language: v })}
            options={[
              { value: "system", label: t("general.languages.system") },
              { value: "en", label: "English" },
              { value: "es", label: "Español" },
            ]}
          />
        )}
      </Row>
    </div>
  );
}
