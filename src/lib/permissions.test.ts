import { describe, expect, it } from "vitest";
import { type Permissions, permissionIssues } from "./permissions";

const granted: Permissions = {
  platform: "macos",
  required: true,
  microphone: "granted",
  microphoneBlock: null,
  hasMicrophone: true,
  accessibility: "granted",
  inputMonitoring: "granted",
  hotkeysActive: true,
};

describe("permissionIssues", () => {
  it("is empty when everything is allowed", () => {
    expect(permissionIssues(granted)).toEqual([]);
  });

  it("lists the microphone first, whether denied or not asked yet", () => {
    expect(permissionIssues({ ...granted, microphone: "denied", accessibility: "denied" })).toEqual(
      ["microphone", "accessibility"],
    );
    expect(permissionIssues({ ...granted, microphone: "notAsked" })).toEqual(["microphone"]);
  });

  it("asks for Input Monitoring only when Accessibility didn't start the hotkey", () => {
    expect(permissionIssues({ ...granted, accessibility: "denied", hotkeysActive: false })).toEqual(
      ["accessibility"],
    );
    expect(permissionIssues({ ...granted, hotkeysActive: false })).toEqual(["inputMonitoring"]);
  });

  it("on Windows only the microphone switch matters", () => {
    const win: Permissions = {
      ...granted,
      platform: "windows",
      microphone: "denied",
      microphoneBlock: "desktopApps",
      hotkeysActive: false,
    };
    expect(permissionIssues(win)).toEqual(["microphone"]);
  });

  it("reports a missing microphone", () => {
    expect(permissionIssues({ ...granted, hasMicrophone: false })).toEqual(["noMicrophone"]);
  });
});
