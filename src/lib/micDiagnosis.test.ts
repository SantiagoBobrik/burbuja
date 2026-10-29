import { describe, expect, it } from "vitest";
import { diagnoseMic, displayHotkey } from "./micDiagnosis";

describe("diagnoseMic", () => {
  it("waits 1.5 s of pure zeros before calling it dead", () => {
    expect(diagnoseMic(0, 1000)).toBe("listening");
    expect(diagnoseMic(0, 1500)).toBe("noSignal");
  });

  it("separates background noise from speech at 0.05", () => {
    expect(diagnoseMic(0.01, 3000)).toBe("noise");
    expect(diagnoseMic(0.05, 3000)).toBe("noise");
    expect(diagnoseMic(0.051, 100)).toBe("hearing");
  });
});

describe("displayHotkey", () => {
  it("shows Super as Win and splits sides", () => {
    expect(displayHotkey("Ctrl+Super")).toBe("Ctrl+Win");
    expect(displayHotkey("RightAlt")).toBe("Right Alt");
    expect(displayHotkey("LeftSuper+F9")).toBe("Left Win+F9");
  });
});
