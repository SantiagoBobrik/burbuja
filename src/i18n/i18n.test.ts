import { describe, expect, it } from "vitest";
import en from "./en.json";
import es from "./es.json";
import { resolveLanguage } from "./index";

function keys(obj: object, prefix = ""): string[] {
  return Object.entries(obj).flatMap(([k, v]) =>
    typeof v === "object" && v !== null ? keys(v, `${prefix}${k}.`) : [`${prefix}${k}`],
  );
}

describe("i18n", () => {
  it("en and es have the same keys", () => {
    expect(keys(es).sort()).toEqual(keys(en).sort());
  });

  it("resolves the system language", () => {
    expect(resolveLanguage("system", "es-AR")).toBe("es");
    expect(resolveLanguage("system", "en-US")).toBe("en");
    expect(resolveLanguage("system", "fr-FR")).toBe("en");
    expect(resolveLanguage("es", "en-US")).toBe("es");
  });
});
