import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X } from "lucide-react";
import { useTranslation } from "react-i18next";

/** Custom title bar: drag region + minimal window buttons. Close hides to tray. */
export function TitleBar() {
  const { t } = useTranslation();
  const win = () => getCurrentWindow();
  const button =
    "inline-flex h-9 w-11 items-center justify-center text-text-2 transition-colors duration-100 hover:bg-surface-2 hover:text-text-1";
  return (
    <header
      data-tauri-drag-region
      className="flex h-9 shrink-0 items-center justify-end border-b-2 border-line-strong bg-surface-1"
    >
      <button
        type="button"
        aria-label={t("titlebar.minimize")}
        className={button}
        onClick={() => void win().minimize()}
      >
        <Minus size={14} strokeWidth={2} aria-hidden="true" />
      </button>
      <button
        type="button"
        aria-label={t("titlebar.maximize")}
        className={button}
        onClick={() => void win().toggleMaximize()}
      >
        <Square size={12} strokeWidth={2} aria-hidden="true" />
      </button>
      <button
        type="button"
        aria-label={t("titlebar.close")}
        className={`${button} hover:bg-coral hover:text-on-accent`}
        onClick={() => void win().close()}
      >
        <X size={15} strokeWidth={2} aria-hidden="true" />
      </button>
    </header>
  );
}
