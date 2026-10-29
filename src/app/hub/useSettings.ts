import { useCallback, useEffect, useState } from "react";
import { commands, events, type Settings_Serialize } from "@/lib/ipc";
import i18n, { resolveLanguage, type UiLanguage } from "@/i18n";
import { untilOk } from "@/lib/retry";

/** Settings as Rust accepted them. Changes save instantly. */
export function useSettings() {
  const [settings, setSettings] = useState<Settings_Serialize | null>(null);

  useEffect(() => {
    // Retry: until settings arrive the Hub can only show its blank background.
    const load = () => void untilOk(commands.getSettings).then(setSettings);
    load();
    const off = events.settingsChanged.listen(load);
    return () => void off.then((f) => f());
  }, []);

  // The interface language follows the setting ("system" = the OS language).
  useEffect(() => {
    if (settings) void i18n.changeLanguage(resolveLanguage(settings.ui_language as UiLanguage));
  }, [settings]);

  const update = useCallback(async (next: Settings_Serialize) => {
    setSettings(next);
    const r = await commands.updateSettings(next);
    setSettings(r.status === "ok" ? r.data : await commands.getSettings());
    return r.status === "ok";
  }, []);

  return { settings, update, setSettings };
}

export type UpdateSettings = (next: Settings_Serialize) => Promise<boolean>;
