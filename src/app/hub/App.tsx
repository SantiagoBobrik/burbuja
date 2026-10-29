import { motion, useReducedMotion } from "motion/react";
import { useCallback, useEffect, useState } from "react";
import { events } from "@/lib/ipc";
import { permissionIssues, usePermissions } from "@/lib/permissions";
import { DictionaryPage } from "./DictionaryPage";
import { HistoryPage } from "./HistoryPage";
import { HomePage } from "./HomePage";
import { Onboarding } from "./Onboarding";
import { PermissionsBanner } from "./PermissionsPanel";
import { type Section, SettingsPage } from "./settings/SettingsPage";
import { type Page, Sidebar } from "./Sidebar";
import { TitleBar } from "./TitleBar";
import { useSettings } from "./useSettings";

export function devParam(name: string): string | null {
  return new URLSearchParams(location.search).get(name);
}

export default function App() {
  const { settings, update } = useSettings();
  // `?page=` deep link, used by development screenshots.
  const [page, setPage] = useState<Page>(
    () => (import.meta.env.DEV ? (devParam("page") as Page) : null) ?? "home",
  );
  const [section, setSection] = useState<Section>(
    () => (import.meta.env.DEV ? (devParam("section") as Section) : null) ?? "general",
  );
  const reduced = useReducedMotion();
  const permissions = usePermissions();
  const pending = permissions ? permissionIssues(permissions).length : 0;
  const showPermissions = useCallback(() => {
    setPage("settings");
    setSection("permissions");
  }, []);

  // A dictation hit a missing permission: the backend opened the Hub, show the fix.
  useEffect(() => {
    const off = events.permissionsNeeded.listen(showPermissions);
    return () => void off.then((f) => f());
  }, [showPermissions]);

  if (!settings) return <div className="h-full bg-bg" />;

  return (
    <div className="flex h-full flex-col bg-bg">
      <TitleBar />
      {!settings.onboarding_completed ? (
        <main className="min-h-0 flex-1">
          <Onboarding settings={settings} update={update} />
        </main>
      ) : (
        <div className="flex min-h-0 flex-1">
          <Sidebar page={page} onNavigate={setPage} />
          {/* The content scrolls, so the scrollbar sits at the window edge. */}
          <div className="scroll-area min-w-0 flex-1 overflow-y-auto">
            <motion.main
              key={page}
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ duration: reduced ? 0.1 : 0.14 }}
              className="mx-auto max-w-[760px] p-6 pb-12"
            >
              {pending > 0 && !(page === "settings" && section === "permissions") && (
                <div className="mb-6">
                  <PermissionsBanner count={pending} onFix={showPermissions} />
                </div>
              )}
              {page === "home" && <HomePage hotkey={settings.hotkey} />}
              {page === "history" && <HistoryPage settings={settings} update={update} />}
              {page === "dictionary" && <DictionaryPage settings={settings} update={update} />}
              {page === "settings" && (
                <SettingsPage
                  settings={settings}
                  update={update}
                  section={section}
                  onSection={setSection}
                />
              )}
            </motion.main>
          </div>
        </div>
      )}
    </div>
  );
}
