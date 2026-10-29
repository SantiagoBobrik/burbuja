/** Settings-meter diagnostic, from the raw peak since the preview opened. */
export type MicDiagnosis = "listening" | "noSignal" | "noise" | "hearing";

export const NO_SIGNAL_AFTER_MS = 1500;
export const SPEECH_PEAK = 0.05;

export function diagnoseMic(peak: number, msSinceOpen: number): MicDiagnosis {
  if (peak > SPEECH_PEAK) return "hearing";
  if (peak > 0) return "noise";
  return msSinceOpen >= NO_SIGNAL_AFTER_MS ? "noSignal" : "listening";
}

/** Canonical hotkey (`Ctrl+Super`) as shown on Windows (`Ctrl+Win`). */
export function displayHotkey(canonical: string): string {
  return canonical
    .split("+")
    .map((part) =>
      part.replace(/^(Left|Right)?Super$/, (_, side) => (side ? `${side} Win` : "Win")),
    )
    .map((part) => part.replace(/^(Left|Right)(Ctrl|Shift|Alt)$/, "$1 $2"))
    .join("+");
}
