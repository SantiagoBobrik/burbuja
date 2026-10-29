import { describe, expect, it } from "vitest";
import { capitalize, dayLabel, relativeTime } from "./format";

const NOW = new Date(2026, 9, 27, 15, 0, 0).getTime();

describe("format", () => {
  it("relative times", () => {
    expect(relativeTime(NOW - 5 * 60_000, "en", NOW)).toBe("5 minutes ago");
    expect(relativeTime(NOW - 2 * 3_600_000, "es", NOW)).toBe("hace 2 horas");
  });

  it("day labels", () => {
    expect(capitalize(dayLabel(NOW - 1000, "en", NOW))).toBe("Today");
    expect(capitalize(dayLabel(NOW - 86_400_000, "es", NOW))).toBe("Ayer");
    expect(dayLabel(NOW - 5 * 86_400_000, "en", NOW)).toContain("October");
  });
});
