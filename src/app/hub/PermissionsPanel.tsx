import { Check, CircleAlert, TriangleAlert } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button, Card } from "@/components/ui/kit";
import { commands, type PermissionKind } from "@/lib/ipc";
import { type Permissions, permissionIssues, refreshPermissions } from "@/lib/permissions";

/** True when dictation can't work yet. */
export function missingPermissions(p: Permissions): boolean {
  return permissionIssues(p).length > 0;
}

type Row = { kind: PermissionKind; granted: boolean };

function rowsFor(p: Permissions): Row[] {
  const rows: Row[] = [{ kind: "microphone", granted: p.microphone === "granted" }];
  if (p.platform === "macos") {
    rows.push({ kind: "accessibility", granted: p.accessibility === "granted" });
    // Only shown when Accessibility alone didn't start the hotkey.
    if (p.accessibility === "granted" && !p.hotkeysActive) {
      rows.push({ kind: "inputMonitoring", granted: false });
    }
  }
  return rows;
}

/**
 * Every permission dictation needs, with its state and the most direct fix: the OS prompt while
 * it still has one, otherwise the exact privacy page.
 */
export function PermissionsPanel({ permissions: p }: { permissions: Permissions }) {
  const { t } = useTranslation();
  // After a first attempt the button also opens Settings, and "Fix" is offered.
  const [prompted, setPrompted] = useState<Partial<Record<PermissionKind, boolean>>>({});
  const settingsLabel =
    p.platform === "windows" ? t("permissions.openWindows") : t("permissions.openMac");

  const ask = async (kind: PermissionKind, repair = false) => {
    const r = await (repair ? commands.resetPermission(kind) : commands.requestPermission(kind));
    if (r.status === "ok") setPrompted((s) => ({ ...s, [kind]: true }));
    refreshPermissions();
  };

  const hint = (row: Row): string | null => {
    if (row.granted) return null;
    if (row.kind === "microphone") {
      if (p.platform === "windows" && p.microphoneBlock) {
        return t(`permissions.microphone.block.${p.microphoneBlock}`);
      }
      if (p.microphone === "denied") return t("permissions.microphone.deniedMac");
      return null;
    }
    if (prompted[row.kind]) return t("permissions.stillOff");
    return null;
  };

  const actionLabel = (row: Row): string => {
    const canPrompt =
      p.platform === "macos" &&
      !prompted[row.kind] &&
      (row.kind === "microphone" ? p.microphone === "notAsked" : true);
    return canPrompt ? t("permissions.allow") : settingsLabel;
  };

  return (
    <Card className="flex flex-col gap-4">
      {rowsFor(p).map((row) => {
        const extra = hint(row);
        return (
          <div key={row.kind} className="flex items-center gap-4">
            <div className="min-w-0 flex-1">
              <p className="flex items-center gap-2 font-extrabold">
                {t(`permissions.${row.kind}.title`)}
                {!row.granted && (
                  <span className="rounded-pill border-2 border-warn px-2 text-[11px] font-bold text-warn">
                    {t("permissions.pending")}
                  </span>
                )}
              </p>
              <p className="text-text-3">{t(`permissions.${row.kind}.body`)}</p>
              {extra && <p className="mt-1 text-warn">{extra}</p>}
            </div>
            {row.granted ? (
              <span className="flex items-center gap-1 font-bold text-ok">
                <Check size={16} aria-hidden="true" /> {t("permissions.granted")}
              </span>
            ) : (
              <div className="flex shrink-0 flex-col items-end gap-2">
                <Button variant="primary" onClick={() => void ask(row.kind)}>
                  {actionLabel(row)}
                </Button>
                {/* macOS keeps approvals per build: an update can leave the switch on but void. */}
                {p.platform === "macos" &&
                  (prompted[row.kind] ||
                    (row.kind === "microphone" && p.microphone === "denied")) && (
                    <Button size="sm" onClick={() => void ask(row.kind, true)}>
                      {t("permissions.repair")}
                    </Button>
                  )}
              </div>
            )}
          </div>
        );
      })}

      {!p.hasMicrophone && (
        <div className="flex items-start gap-3 text-warn" role="status">
          <CircleAlert size={18} className="mt-0.5 shrink-0" aria-hidden="true" />
          <div>
            <p className="font-extrabold">{t("permissions.noMicrophone.title")}</p>
            <p>{t("permissions.noMicrophone.body")}</p>
          </div>
        </div>
      )}

      {p.platform === "macos" && p.accessibility === "granted" && !p.hotkeysActive && (
        <div className="flex items-center gap-4 border-t-2 border-line pt-4">
          <p className="min-w-0 flex-1 text-text-3">{t("permissions.restartHint")}</p>
          <Button onClick={() => void commands.restartApp()}>{t("permissions.restart")}</Button>
        </div>
      )}
    </Card>
  );
}

/** One line on every Hub page while `count` things are missing, linking to the fix. */
export function PermissionsBanner({ count, onFix }: { count: number; onFix: () => void }) {
  const { t } = useTranslation();
  return (
    <div
      role="alert"
      className="flex items-center gap-4 rounded-md border-2 border-warn bg-surface-1 p-4 shadow-brut-sm"
    >
      <TriangleAlert size={20} className="shrink-0 text-warn" aria-hidden="true" />
      <div className="min-w-0 flex-1">
        <p className="font-extrabold">{t("permissions.banner.title", { count })}</p>
        <p className="text-text-3">{t("permissions.banner.body")}</p>
      </div>
      <Button variant="primary" onClick={onFix}>
        {t("permissions.banner.action")}
      </Button>
    </div>
  );
}
