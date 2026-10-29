import { Copy, Eye, EyeOff, RefreshCw, Search, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button, Card, IconButton, Input, Select, Star } from "@/components/ui/kit";
import { commands, events, type HistoryItem, type Settings_Serialize } from "@/lib/ipc";
import { capitalize, clockTime, dayLabel, seconds } from "./format";
import type { UpdateSettings } from "./useSettings";

const PAGE = 50;

export function HistoryPage({
  settings,
  update,
}: {
  settings: Settings_Serialize;
  update: UpdateSettings;
}) {
  const { t, i18n } = useTranslation();
  const [items, setItems] = useState<HistoryItem[]>([]);
  const [query, setQuery] = useState("");
  const [more, setMore] = useState(false);
  const [showRaw, setShowRaw] = useState<Record<number, boolean>>({});
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState<number | null>(null);

  const load = useCallback(async (q: string, before: number | null = null) => {
    const r = await commands.getHistory(before, PAGE, q);
    if (r.status !== "ok") return;
    setItems((prev) => (before === null ? r.data : [...prev, ...r.data]));
    setMore(r.data.length === PAGE);
  }, []);

  // Debounced search.
  useEffect(() => {
    const id = window.setTimeout(() => void load(query), 150);
    return () => window.clearTimeout(id);
  }, [query, load]);

  useEffect(() => {
    const off = events.historyChanged.listen(() => void load(query));
    return () => void off.then((f) => f());
  }, [query, load]);

  const reclean = async (id: number) => {
    setBusy(id);
    await commands.retryCleanup(id);
    setBusy(null);
  };

  // Consecutive items of the same day share one list (items arrive newest first).
  const groups: { day: string; items: HistoryItem[] }[] = [];
  for (const item of items) {
    const day = capitalize(dayLabel(item.createdAt ?? 0, i18n.language));
    const last = groups[groups.length - 1];
    if (last?.day === day) last.items.push(item);
    else groups.push({ day, items: [item] });
  }
  return (
    <div className="flex flex-col gap-5">
      <div className="flex items-center justify-between gap-4">
        <h1 className="text-[22px] font-extrabold tracking-[-0.02em]">{t("nav.history")}</h1>
        <div className="flex items-center gap-3">
          <label className="flex items-center gap-2 text-text-2">
            {t("history.keep")}
            <Select
              value={settings.history_retention_days}
              onChange={(e) =>
                void update({ ...settings, history_retention_days: Number(e.target.value) })
              }
            >
              {[7, 30, 90, 0].map((d) => (
                <option key={d} value={d}>
                  {d === 0 ? t("history.forever") : t("history.days", { n: d })}
                </option>
              ))}
            </Select>
          </label>
          <Button onClick={() => setConfirming(true)} disabled={items.length === 0}>
            {t("history.clear")}
          </Button>
        </div>
      </div>

      {confirming && (
        <Card className="flex items-center justify-between gap-4" flat>
          <p role="alert" className="text-text-1">
            {t("history.confirmClear")}
          </p>
          <div className="flex gap-2">
            <Button size="sm" onClick={() => setConfirming(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              size="sm"
              variant="danger"
              onClick={() => {
                setConfirming(false);
                void commands.clearHistory();
              }}
            >
              {t("history.clearAll")}
            </Button>
          </div>
        </Card>
      )}

      <label className="relative">
        <span className="sr-only">{t("history.search")}</span>
        <Search
          size={15}
          aria-hidden="true"
          className="absolute top-1/2 left-3 -translate-y-1/2 text-text-3"
        />
        <Input
          type="search"
          name="history-search"
          autoComplete="off"
          placeholder={t("history.searchPlaceholder")}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="w-full pl-9"
        />
      </label>

      {items.length === 0 ? (
        <Card flat className="relative flex flex-col items-center gap-2 py-10 text-center">
          <Star className="absolute top-4 right-6 size-6" />
          <Star className="absolute bottom-5 left-8 size-4" color="var(--lavender)" />
          <p className="font-bold">{query ? t("history.noMatches") : t("history.empty")}</p>
        </Card>
      ) : (
        <div className="flex flex-col gap-5">
          {groups.map((group) => (
            <section key={group.day} className="flex flex-col gap-2">
              <h2 className="text-[12px] font-bold tracking-wide text-text-3 uppercase">
                {group.day}
              </h2>
              <ul className="divide-y-2 divide-line overflow-hidden rounded-md border-2 border-line-strong bg-surface-1">
                {group.items.map((item) => {
                  const id = item.id ?? 0;
                  const at = item.createdAt ?? 0;
                  const raw = showRaw[id];
                  return (
                    <li
                      key={id}
                      className="group flex items-start gap-3 px-4 py-3 transition-colors duration-100 [content-visibility:auto] hover:bg-surface-2"
                    >
                      <div className="min-w-0 flex-1">
                        <p
                          className={`text-[14px] select-text ${raw ? "text-text-2" : "text-text-1"}`}
                        >
                          {raw ? item.raw : item.text}
                        </p>
                        <p className="mt-1 text-[12px] text-text-3 tabular-nums">
                          {clockTime(at, i18n.language)}
                          {item.targetApp && ` · ${item.targetApp}`} ·{" "}
                          {seconds(item.durationMs ?? 0, i18n.language)}
                        </p>
                      </div>
                      <div className="flex shrink-0 opacity-0 transition-opacity duration-100 group-focus-within:opacity-100 group-hover:opacity-100">
                        <IconButton
                          aria-label={t("history.copy")}
                          onClick={() => void commands.copyHistoryItem(id)}
                        >
                          <Copy size={14} aria-hidden="true" />
                        </IconButton>
                        <IconButton
                          aria-label={raw ? t("history.showFinal") : t("history.showRaw")}
                          aria-pressed={raw}
                          onClick={() => setShowRaw((s) => ({ ...s, [id]: !raw }))}
                        >
                          {raw ? (
                            <EyeOff size={14} aria-hidden="true" />
                          ) : (
                            <Eye size={14} aria-hidden="true" />
                          )}
                        </IconButton>
                        <IconButton
                          aria-label={t("history.reclean")}
                          disabled={busy === id}
                          onClick={() => void reclean(id)}
                        >
                          <RefreshCw
                            size={14}
                            aria-hidden="true"
                            className={busy === id ? "animate-spin" : ""}
                          />
                        </IconButton>
                        <IconButton
                          aria-label={t("history.delete")}
                          className="hover:text-coral"
                          onClick={() => void commands.deleteHistoryItem(id)}
                        >
                          <Trash2 size={14} aria-hidden="true" />
                        </IconButton>
                      </div>
                    </li>
                  );
                })}
              </ul>
            </section>
          ))}
        </div>
      )}
      {more && (
        <Button
          className="self-center"
          onClick={() => void load(query, items[items.length - 1]?.id ?? null)}
        >
          {t("history.more")}
        </Button>
      )}
    </div>
  );
}
