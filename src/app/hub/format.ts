/** Locale-aware formatting helpers (Intl only; no hard-coded formats). */

export function relativeTime(epochMs: number, locale: string, now = Date.now()): string {
  const seconds = Math.round((epochMs - now) / 1000);
  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  const abs = Math.abs(seconds);
  if (abs < 60) return rtf.format(seconds, "second");
  if (abs < 3600) return rtf.format(Math.round(seconds / 60), "minute");
  if (abs < 86_400) return rtf.format(Math.round(seconds / 3600), "hour");
  return rtf.format(Math.round(seconds / 86_400), "day");
}

/** "Today", "Yesterday" or a date, for grouping the history by day. */
export function dayLabel(epochMs: number, locale: string, now = Date.now()): string {
  const day = (ms: number) => new Date(ms).toDateString();
  if (day(epochMs) === day(now))
    return new Intl.RelativeTimeFormat(locale, { numeric: "auto" }).format(0, "day");
  if (day(epochMs) === day(now - 86_400_000)) {
    return new Intl.RelativeTimeFormat(locale, { numeric: "auto" }).format(-1, "day");
  }
  return new Intl.DateTimeFormat(locale, { weekday: "long", day: "numeric", month: "long" }).format(
    epochMs,
  );
}

export function clockTime(epochMs: number, locale: string): string {
  return new Intl.DateTimeFormat(locale, { hour: "numeric", minute: "2-digit" }).format(epochMs);
}

export function seconds(ms: number, locale: string): string {
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(ms / 1000)} s`;
}

export function capitalize(s: string): string {
  return s.charAt(0).toLocaleUpperCase() + s.slice(1);
}
