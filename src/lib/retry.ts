/**
 * Calls `fn` until it resolves, waiting a little longer after each failure (capped at the last
 * delay). For a window's first IPC call: the backend may still be starting, and a single
 * failed call must never leave the window blank.
 */
export async function untilOk<T>(
  fn: () => Promise<T>,
  delaysMs: readonly number[] = [100, 250, 500, 1000, 2000],
): Promise<T> {
  for (let attempt = 0; ; attempt++) {
    try {
      return await fn();
    } catch (e) {
      const delay = delaysMs[Math.min(attempt, delaysMs.length - 1)];
      console.warn(`IPC call failed (attempt ${attempt + 1}); retrying in ${delay} ms`, e);
      await new Promise((r) => setTimeout(r, delay));
    }
  }
}
