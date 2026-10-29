import { describe, expect, it, vi } from "vitest";
import { untilOk } from "./retry";

describe("untilOk", () => {
  it("returns the first success", async () => {
    await expect(untilOk(() => Promise.resolve(42))).resolves.toBe(42);
  });

  it("retries failures until the call succeeds", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    let calls = 0;
    const fn = () =>
      ++calls < 3 ? Promise.reject(new Error("state not managed")) : Promise.resolve("ok");
    await expect(untilOk(fn, [1])).resolves.toBe("ok");
    expect(calls).toBe(3);
  });
});
