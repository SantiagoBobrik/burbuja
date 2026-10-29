import { getVersion } from "@tauri-apps/api/app";
import { BookText, History, House, Settings } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { PendingBadge } from "@/components/ui/kit";
import { commands, events, type EngineStatus } from "@/lib/ipc";
import { permissionIssues, usePermissions } from "@/lib/permissions";

export type Page = "home" | "history" | "dictionary" | "settings";

const NAV: { page: Page; icon: typeof House }[] = [
  { page: "home", icon: House },
  { page: "history", icon: History },
  { page: "dictionary", icon: BookText },
  { page: "settings", icon: Settings },
];

export function Sidebar({ page, onNavigate }: { page: Page; onNavigate: (p: Page) => void }) {
  const { t } = useTranslation();
  const [engine, setEngine] = useState<EngineStatus | null>(null);
  const [version, setVersion] = useState("");
  const permissions = usePermissions();
  const pending = permissions ? permissionIssues(permissions).length : 0;

  useEffect(() => {
    void commands.getEngineStatus().then(setEngine);
    void getVersion()
      .then(setVersion)
      .catch(() => {});
    const off = events.engineStatusChanged.listen(({ payload }) => setEngine(payload));
    return () => void off.then((f) => f());
  }, []);

  return (
    <nav
      aria-label={t("nav.label")}
      className="flex w-[200px] shrink-0 flex-col border-r-2 border-line-strong bg-surface-1 p-4"
    >
      <p className="mb-6 px-2 text-[22px] font-extrabold tracking-[-0.02em]" translate="no">
        burbuja
      </p>
      <ul className="flex flex-col gap-1.5">
        {NAV.map(({ page: p, icon: Icon }) => {
          const active = p === page;
          return (
            <li key={p}>
              <button
                type="button"
                aria-current={active ? "page" : undefined}
                onClick={() => onNavigate(p)}
                className={`flex h-9 w-full items-center gap-2.5 rounded-md border-2 px-2.5 text-left font-bold transition-[background-color,border-color] duration-100 ${
                  active
                    ? "border-line-strong bg-accent text-on-accent shadow-brut-sm"
                    : "border-transparent text-text-2 hover:border-line-strong hover:text-text-1"
                }`}
              >
                <Icon size={16} strokeWidth={2} aria-hidden="true" />
                <span className="flex-1">{t(`nav.${p}`)}</span>
                {p === "settings" && pending > 0 && (
                  <PendingBadge count={pending} label={t("permissions.pending")} />
                )}
              </button>
            </li>
          );
        })}
      </ul>
      <div className="mt-auto flex flex-col gap-1 px-2 text-[12px] text-text-3">
        {engine && (
          <p className="flex items-center gap-2" aria-live="polite">
            <span
              aria-hidden="true"
              className={`size-2 shrink-0 rounded-pill ${engine.state === "ready" ? "bg-ok" : engine.state === "failed" ? "bg-danger" : "bg-text-3"}`}
            />
            <span className="min-w-0 leading-tight">
              <span className="block truncate">{engine.model.replace("whisper-", "")}</span>
              <span className="block">
                {engine.onGpu ? "GPU" : "CPU"} · {t(`models.engine.${engine.state}`)}
              </span>
            </span>
          </p>
        )}
        {version && <p className="tabular-nums">v{version}</p>}
      </div>
    </nav>
  );
}
