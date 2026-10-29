import { useSyncExternalStore } from "react";
import { commands, type PermissionKind, type Permissions_Serialize } from "@/lib/ipc";

export type Permissions = Permissions_Serialize;

/** Something that stops dictation from working, in the order the user should fix it. */
export type PermissionIssue = PermissionKind | "noMicrophone";

/**
 * What is still missing. Accessibility alone normally starts the macOS hotkey; Input Monitoring
 * only counts when it didn't.
 */
export function permissionIssues(p: Permissions): PermissionIssue[] {
  const issues: PermissionIssue[] = [];
  if (p.required && p.microphone !== "granted") issues.push("microphone");
  if (!p.hasMicrophone) issues.push("noMicrophone");
  if (p.platform === "macos") {
    if (p.accessibility !== "granted") issues.push("accessibility");
    else if (!p.hotkeysActive) issues.push("inputMonitoring");
  }
  return issues;
}

// One shared poll for every component: macOS and Windows send no change event, and the user
// flips the switch in another app, so the status is re-read every second while anything shows
// it, and at once when the Hub gets focus back.
const POLL_MS = 1000;
let current: Permissions | null = null;
const listeners = new Set<() => void>();
let timer: number | undefined;

function refresh() {
  void commands
    .getPermissions()
    .then((p) => {
      if (JSON.stringify(p) === JSON.stringify(current)) return;
      current = p;
      listeners.forEach((l) => l());
    })
    .catch(() => {});
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  if (listeners.size === 1) {
    refresh();
    timer = window.setInterval(refresh, POLL_MS);
    window.addEventListener("focus", refresh);
  }
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) {
      window.clearInterval(timer);
      window.removeEventListener("focus", refresh);
    }
  };
}

/** Live OS permission status; `null` until the first answer. */
export function usePermissions(): Permissions | null {
  return useSyncExternalStore(subscribe, () => current);
}

/** Re-reads the status now (after a request), instead of waiting for the next poll. */
export const refreshPermissions = refresh;
