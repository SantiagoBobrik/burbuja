import { describe, expect, it } from "vitest";
import { barHeights, clock, errorMessage, SIZES, stateView } from "./pillModel";

describe("pill sizes", () => {
  it("matches the spec table", () => {
    expect(SIZES.idle).toEqual({ width: 12, height: 12 });
    expect(SIZES.hover).toEqual({ width: 132, height: 28 });
    expect(SIZES.listening).toEqual({ width: 132, height: 32 });
    expect(SIZES.handsFree).toEqual({ width: 176, height: 32 });
    expect(SIZES.processing).toEqual({ width: 96, height: 32 });
  });
});

describe("stateView", () => {
  it("maps backend states and hover", () => {
    expect(stateView("idle", false).kind).toBe("idle");
    expect(stateView("idle", true).kind).toBe("hover");
    expect(stateView("listening", true).kind).toBe("listening");
    expect(stateView("handsFree", false).kind).toBe("handsFree");
    expect(stateView("polishing", false).kind).toBe("processing");
  });
});

describe("errorMessage", () => {
  it("treats too short and no speech as info, the rest as errors", () => {
    expect(errorMessage("too_short", "Too short").message?.tone).toBe("info");
    expect(errorMessage("no_speech", "x").message?.tone).toBe("info");
    expect(errorMessage("mic_silent", "x").message?.tone).toBe("error");
    expect(errorMessage("elevated_target", "x").message?.key).toBe("pill.errors.elevated_target");
  });
});

describe("clock and bars", () => {
  it("formats m:ss", () => {
    expect(clock(42_000)).toBe("0:42");
    expect(clock(301_500)).toBe("5:01");
  });

  it("maps 16 bands to 9 bars of 4–18 px", () => {
    const silent = barHeights(new Array(16).fill(0));
    expect(silent).toHaveLength(9);
    expect(silent.every((h) => h === 4)).toBe(true);
    expect(barHeights(new Array(16).fill(1)).every((h) => h === 18)).toBe(true);
    expect(barHeights([null, 2, -1]).every((h) => h >= 4 && h <= 18)).toBe(true);
  });
});
