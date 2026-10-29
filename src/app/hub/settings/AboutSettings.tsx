import { getVersion } from "@tauri-apps/api/app";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button, Card } from "@/components/ui/kit";
import notices from "../../../../THIRD_PARTY_NOTICES.md?raw";

export function AboutSettings() {
  const { t } = useTranslation();
  const [version, setVersion] = useState("");
  const [showLicenses, setShowLicenses] = useState(false);

  useEffect(() => {
    void getVersion()
      .then(setVersion)
      .catch(() => {});
  }, []);

  return (
    <div className="flex flex-col gap-6">
      <Card className="flex items-center gap-5">
        {/* Brand mark: the same yellow bubble as the app icon and the welcome step. */}
        <span
          aria-hidden="true"
          className="relative size-14 shrink-0 rounded-pill border-2 border-line-strong bg-accent shadow-brut-sm"
        >
          <span className="absolute top-2 left-2.5 h-2 w-4 -rotate-[35deg] rounded-pill bg-text-1/80" />
        </span>
        <div>
          <p className="text-[24px] font-extrabold tracking-[-0.02em]" translate="no">
            burbuja
          </p>
          <p className="text-text-2 tabular-nums">{version && t("about.version", { version })}</p>
          <p className="text-text-3">{t("app.tagline")}</p>
        </div>
      </Card>
      <p className="text-text-1">{t("about.privacy")}</p>
      <div className="flex flex-col gap-3">
        <Button
          className="self-start"
          aria-expanded={showLicenses}
          onClick={() => setShowLicenses((v) => !v)}
        >
          {showLicenses ? t("about.hideLicenses") : t("about.licenses")}
        </Button>
        {showLicenses && (
          <pre className="scroll-area max-h-80 overflow-auto rounded-md border-2 border-line-strong bg-surface-2 p-4 font-mono text-[11px] whitespace-pre-wrap text-text-2 select-text">
            {notices}
          </pre>
        )}
      </div>
    </div>
  );
}
