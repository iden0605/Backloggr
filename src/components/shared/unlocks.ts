import { useAppStore } from "../../store/useAppStore";

/**
 * First-of-kind "unlock" moments (first session ever, first clip, first completed game) —
 * a single quiet banner per lifetime event, not an achievement system. Each key fires at
 * most once per install (localStorage flag); the flag is set even when the check says
 * "not actually the first" (e.g. an existing library) so pre-existing users never get a
 * late false banner.
 */
export type UnlockKey = "first-session" | "first-clip" | "first-completed";

const STORAGE_PREFIX = "backloggr-unlock:";

const UNLOCK_COPY: Record<UnlockKey, string> = {
  "first-session": "First session tracked — playtime logs itself from here.",
  "first-clip": "First clip saved — it lives in Clips, attached to its game.",
  "first-completed": "First game marked completed.",
};

/**
 * Fires the banner for `key` if it has never fired AND `isActuallyFirst` confirms the
 * event really is the first of its kind. The flag is consumed either way.
 */
export async function maybeUnlock(key: UnlockKey, isActuallyFirst: () => Promise<boolean>) {
  const storageKey = STORAGE_PREFIX + key;
  if (localStorage.getItem(storageKey)) return;
  localStorage.setItem(storageKey, "1");
  try {
    if (await isActuallyFirst()) {
      useAppStore.getState().setUnlockNotice(UNLOCK_COPY[key]);
    }
  } catch {
    // Best-effort — a failed check just skips the banner.
  }
}
