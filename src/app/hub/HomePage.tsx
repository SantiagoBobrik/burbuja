import { Copy } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button, Card, IconButton, Keys } from "@/components/ui/kit";
import { commands, events, type HistoryItem, type Stats } from "@/lib/ipc";
import { displayHotkey } from "@/lib/micDiagnosis";
import { relativeTime } from "./format";

export function HomePage({ hotkey }: { hotkey: string }) {
  const { t, i18n } = useTranslation();
  const [stats, setStats] = useState<Stats | null>(null);
  const [recent, setRecent] = useState<HistoryItem[]>([]);
  const [trying, setTrying] = useState(false);

  const load = useCallback(() => {
    void commands.getStats().then((r) => r.status === "ok" && setStats(r.data));
    void commands.getHistory(null, 5, "").then((r) => r.status === "ok" && setRecent(r.data));
  }, []);

  useEffect(() => {
    load();
    const off = events.historyChanged.listen(load);
    return () => void off.then((f) => f());
  }, [load]);

  const n = (v: number | null | undefined, digits = 0) =>
    new Intl.NumberFormat(i18n.language, { maximumFractionDigits: digits }).format(v ?? 0);

  const metrics = [
    { label: t("home.words"), value: n(stats?.wordsThisWeek), tone: "bg-accent" },
    { label: t("home.wpm"), value: n(stats?.averageWpm), tone: "bg-lavender" },
    {
      label: t("home.saved"),
      value: t("home.minutes", { n: n(stats?.minutesSaved, 0) }),
      tone: "bg-teal",
    },
  ];

  return (
    <div className="flex flex-col gap-8">
      <section aria-labelledby="hero" className="flex flex-col gap-4">
        <h1
          id="hero"
          className="text-[26px] font-extrabold leading-tight tracking-[-0.02em] text-balance"
        >
          {t("home.hero.before")} <Keys combo={displayHotkey(hotkey)} large />{" "}
          {t("home.hero.after")}
        </h1>
        <div className="flex items-center gap-3">
          <Button
            variant={trying ? "ghost" : "primary"}
            aria-expanded={trying}
            onClick={() => setTrying((v) => !v)}
          >
            {trying ? t("home.hideTry") : t("home.try")}
          </Button>
        </div>
        {trying && (
          <textarea
            autoFocus
            aria-label={t("home.tryLabel")}
            placeholder={t("home.tryPlaceholder", { hotkey: displayHotkey(hotkey) })}
            className="min-h-24 resize-y rounded-md border-2 border-line-strong bg-surface-2 p-3 text-[14px] text-text-1 shadow-brut-sm transition-[box-shadow,border-color] duration-100 placeholder:text-text-3 focus:border-accent focus:shadow-[3px_3px_0_0_var(--accent)] focus:outline-none"
          />
        )}
      </section>

      {/* One strip, not three equal cards: the numbers are supporting context, so the strip
          stays flat and the hero keeps the emphasis. */}
      <section aria-labelledby="stats" className="flex flex-col gap-3">
        <h2 id="stats" className="text-[15px] font-extrabold">
          {t("home.statsLabel")}
        </h2>
        <dl className="grid grid-cols-3 divide-x-2 divide-line rounded-md border-2 border-line-strong bg-surface-1">
          {metrics.map((m) => (
            <div key={m.label} className="flex flex-col-reverse gap-1 px-5 py-4">
              <dt className="flex items-center gap-2 text-text-2">
                <span
                  aria-hidden="true"
                  className={`size-2.5 shrink-0 rounded-[3px] border-2 border-line-strong ${m.tone}`}
                />
                {m.label}
              </dt>
              <dd className="font-mono text-[26px] font-semibold tracking-[-0.02em] tabular-nums">
                {m.value}
              </dd>
            </div>
          ))}
        </dl>
      </section>

      <section aria-labelledby="recent" className="flex flex-col gap-3">
        <h2 id="recent" className="text-[15px] font-extrabold">
          {t("home.recent")}
        </h2>
        {recent.length === 0 ? (
          <Card flat>
            <p className="text-text-2">{t("home.empty", { hotkey: displayHotkey(hotkey) })}</p>
          </Card>
        ) : (
          <ul className="divide-y-2 divide-line overflow-hidden rounded-md border-2 border-line-strong bg-surface-1">
            {recent.map((item) => (
              <li
                key={item.id}
                className="group flex items-start gap-3 px-4 py-3 transition-colors duration-100 hover:bg-surface-2"
              >
                <p className="line-clamp-2 min-w-0 flex-1 text-[14px] text-text-1 select-text">
                  {item.text}
                </p>
                <span className="shrink-0 pt-0.5 text-[12px] text-text-3 tabular-nums">
                  {relativeTime(item.createdAt ?? 0, i18n.language)}
                </span>
                <IconButton
                  aria-label={t("history.copy")}
                  className="opacity-0 group-focus-within:opacity-100 group-hover:opacity-100"
                  onClick={() => void commands.copyHistoryItem(item.id ?? 0)}
                >
                  <Copy size={14} aria-hidden="true" />
                </IconButton>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
