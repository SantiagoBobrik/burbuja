import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import en from "./en.json";
import es from "./es.json";

export type UiLanguage = "system" | "en" | "es";

/** Resolves "system" to the OS/browser language, falling back to English. */
export function resolveLanguage(pref: UiLanguage, systemLang = navigator.language): "en" | "es" {
  if (pref !== "system") return pref;
  return systemLang.toLowerCase().startsWith("es") ? "es" : "en";
}

void i18n.use(initReactI18next).init({
  resources: { en: { translation: en }, es: { translation: es } },
  lng: resolveLanguage("system"),
  fallbackLng: "en",
  interpolation: { escapeValue: false },
});

export default i18n;
