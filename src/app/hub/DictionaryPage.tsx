import { ArrowRight, X } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button, IconButton, Input } from "@/components/ui/kit";
import type { Settings_Serialize } from "@/lib/ipc";
import type { UpdateSettings } from "./useSettings";

export function DictionaryPage({
  settings,
  update,
}: {
  settings: Settings_Serialize;
  update: UpdateSettings;
}) {
  const { t } = useTranslation();
  const [term, setTerm] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");

  const addTerm = () => {
    const v = term.trim();
    if (!v || settings.vocabulary.includes(v)) return;
    void update({ ...settings, vocabulary: [...settings.vocabulary, v] });
    setTerm("");
  };
  const addReplacement = () => {
    if (!from.trim() || !to.trim()) return;
    void update({
      ...settings,
      replacements: [...settings.replacements, { from: from.trim(), to: to.trim() }],
    });
    setFrom("");
    setTo("");
  };

  return (
    <div className="flex flex-col gap-8">
      <h1 className="text-[22px] font-extrabold tracking-[-0.02em]">{t("nav.dictionary")}</h1>

      <section aria-labelledby="vocab" className="flex flex-col gap-3">
        <div>
          <h2 id="vocab" className="text-[15px] font-extrabold">
            {t("dictionary.vocabulary")}
          </h2>
          <p className="text-text-3">{t("dictionary.vocabularyHint")}</p>
        </div>
        <div className="flex items-center gap-2">
          <Input
            name="vocabulary-term"
            autoComplete="off"
            spellCheck={false}
            aria-label={t("dictionary.addTerm")}
            placeholder={t("dictionary.addTermPlaceholder")}
            value={term}
            onChange={(e) => setTerm(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && addTerm()}
            className="flex-1"
          />
          <Button onClick={addTerm} disabled={!term.trim()}>
            {t("dictionary.add")}
          </Button>
        </div>
        {settings.vocabulary.length > 0 && (
          <ul className="flex flex-wrap gap-2" aria-label={t("dictionary.vocabulary")}>
            {settings.vocabulary.map((v) => (
              <li
                key={v}
                className="inline-flex items-center gap-1 rounded-pill border-2 border-line-strong bg-surface-2 py-0.5 pr-1 pl-3 font-bold"
                translate="no"
              >
                {v}
                <IconButton
                  aria-label={t("dictionary.remove", { term: v })}
                  className="size-6"
                  onClick={() =>
                    void update({
                      ...settings,
                      vocabulary: settings.vocabulary.filter((x) => x !== v),
                    })
                  }
                >
                  <X size={12} aria-hidden="true" />
                </IconButton>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section aria-labelledby="replacements" className="flex flex-col gap-3">
        <div>
          <h2 id="replacements" className="text-[15px] font-extrabold">
            {t("dictionary.replacements")}
          </h2>
          <p className="text-text-3">{t("dictionary.replacementsHint")}</p>
        </div>
        <div className="flex items-center gap-2">
          <Input
            name="replace-from"
            autoComplete="off"
            aria-label={t("dictionary.from")}
            placeholder={t("dictionary.fromPlaceholder")}
            value={from}
            onChange={(e) => setFrom(e.target.value)}
            className="flex-1"
          />
          <ArrowRight size={16} aria-hidden="true" className="shrink-0 text-text-3" />
          <Input
            name="replace-to"
            autoComplete="off"
            aria-label={t("dictionary.to")}
            placeholder={t("dictionary.toPlaceholder")}
            value={to}
            onChange={(e) => setTo(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && addReplacement()}
            className="flex-1"
          />
          <Button onClick={addReplacement} disabled={!from.trim() || !to.trim()}>
            {t("dictionary.add")}
          </Button>
        </div>
        {settings.replacements.length > 0 && (
          <div className="rounded-md border-2 border-line-strong bg-surface-1">
            <ul className="divide-y-2 divide-line">
              {settings.replacements.map((r, i) => (
                <li key={`${r.from}-${i}`} className="flex items-center gap-3 px-4 py-2.5">
                  <span className="min-w-0 flex-1 truncate text-text-2">{r.from}</span>
                  <ArrowRight size={14} aria-hidden="true" className="shrink-0 text-text-3" />
                  <span className="min-w-0 flex-1 truncate font-bold">{r.to}</span>
                  <IconButton
                    aria-label={t("dictionary.removeReplacement", { from: r.from })}
                    onClick={() =>
                      void update({
                        ...settings,
                        replacements: settings.replacements.filter((_, j) => j !== i),
                      })
                    }
                  >
                    <X size={14} aria-hidden="true" />
                  </IconButton>
                </li>
              ))}
            </ul>
          </div>
        )}
      </section>
    </div>
  );
}
