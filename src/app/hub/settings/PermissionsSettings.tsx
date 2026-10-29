import { Check } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button, Row } from "@/components/ui/kit";
import { permissionIssues, usePermissions } from "@/lib/permissions";
import { PermissionsPanel } from "../PermissionsPanel";
import type { SettingsProps } from "./SettingsPage";

/** What the OS lets Burbuja do, with a one-click fix for anything still pending. */
export function PermissionsSettings({ settings, update }: SettingsProps) {
  const { t } = useTranslation();
  const permissions = usePermissions();
  if (!permissions) return null;
  const allSet = permissionIssues(permissions).length === 0;

  return (
    <div className="flex flex-col gap-5">
      <p className="text-text-2">
        {t(permissions.platform === "windows" ? "permissions.bodyWindows" : "permissions.body")}
      </p>
      {allSet && (
        <p role="status" className="flex items-center gap-2 font-bold text-ok">
          <Check size={16} aria-hidden="true" /> {t("permissions.allSet")}
        </p>
      )}
      <PermissionsPanel permissions={permissions} />
      <Row label={t("permissions.rerun")} hint={t("permissions.rerunHint")}>
        {(id) => (
          <Button id={id} onClick={() => void update({ ...settings, onboarding_completed: false })}>
            {t("permissions.rerunAction")}
          </Button>
        )}
      </Row>
    </div>
  );
}
