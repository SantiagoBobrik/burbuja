/** What the bubble shows, and its exact size. Pure: no React, no Tauri. */

export type PillKind =
  "idle" | "hover" | "listening" | "handsFree" | "processing" | "inserted" | "message";

export type MessageTone = "info" | "error";

export type PillView = {
  kind: PillKind;
  /** px; `"auto"` sizes to the message text (max 360). */
  width: number | "auto";
  height: number;
  message?: { tone: MessageTone; key: string; fallback: string };
};

export const SIZES: Record<Exclude<PillKind, "message">, { width: number; height: number }> = {
  idle: { width: 12, height: 12 },
  hover: { width: 132, height: 28 },
  listening: { width: 132, height: 32 },
  handsFree: { width: 176, height: 32 },
  processing: { width: 96, height: 32 },
  inserted: { width: 12, height: 12 },
};

export const MESSAGE_MS = { info: 1600, error: 4000 } as const;
export const ENDING_SOON_MS = 30_000;
export const INSERTED_MS = 150;

export function view(kind: Exclude<PillKind, "message">): PillView {
  return { kind, ...SIZES[kind] };
}

export function messageView(tone: MessageTone, key: string, fallback: string): PillView {
  return { kind: "message", width: "auto", height: 32, message: { tone, key, fallback } };
}

/** Error codes from `dictation-error` → tone and i18n key. Mild outcomes are info, not errors. */
export function errorMessage(code: string, fallback: string): PillView {
  const info = code === "too_short" || code === "no_speech" || code === "cleanup_skipped";
  return messageView(info ? "info" : "error", `pill.errors.${code}`, fallback);
}

/** Maps a backend dictation state (plus hover while idle) to a view. */
export function stateView(state: string, hovered: boolean): PillView {
  switch (state) {
    case "listening":
      return view("listening");
    case "handsFree":
      return view("handsFree");
    case "processing":
    case "polishing":
      return view("processing");
    default:
      return view(hovered ? "hover" : "idle");
  }
}

/** `m:ss`, for the hands-free clock. */
export function clock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

/** 16 meter bands → 9 waveform bar heights in px (4–18). */
export function barHeights(levels: ReadonlyArray<number | null>): number[] {
  return Array.from({ length: 9 }, (_, i) => {
    const level = levels[Math.round((i * (levels.length - 1)) / 8)] ?? 0;
    return 4 + Math.min(1, Math.max(0, level)) * 14;
  });
}
