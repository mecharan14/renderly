/** Theme bootstrap (improvement-plan D1; redesign approved 2026-07-12).
 *
 * Dark is the product default — a fresh install gets dark regardless of the OS setting
 * (approved redesign decision). Users opt into `"light"` or `"system"` via the theme
 * cycle action; the choice persists in localStorage `renderly.theme`. Writes
 * `data-theme` on `<html>` so CSS semantic tokens apply.
 */

export type ThemePreference = "light" | "dark" | "system";
export type ResolvedTheme = "light" | "dark";

const STORAGE_KEY = "renderly.theme";

function systemPrefersDark(): boolean {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function getThemePreference(): ThemePreference {
  const raw = localStorage.getItem(STORAGE_KEY);
  if (raw === "light" || raw === "dark" || raw === "system") return raw;
  return "dark";
}

export function resolveTheme(pref: ThemePreference = getThemePreference()): ResolvedTheme {
  if (pref === "system") return systemPrefersDark() ? "dark" : "light";
  return pref;
}

export function applyTheme(resolved: ResolvedTheme): void {
  document.documentElement.setAttribute("data-theme", resolved);
}

/** Persist preference and apply resolved theme. */
export function setTheme(pref: ThemePreference): void {
  localStorage.setItem(STORAGE_KEY, pref);
  applyTheme(resolveTheme(pref));
}

/** Call once at app boot (before first paint if possible). */
export function bootstrapTheme(): void {
  applyTheme(resolveTheme());
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
    if (getThemePreference() === "system") {
      applyTheme(resolveTheme("system"));
    }
  });
}

/** Dev helper: cycle dark → light → system. Exposed for Sprint 3 flag toggle. */
export function cycleTheme(): ResolvedTheme {
  const order: ThemePreference[] = ["dark", "light", "system"];
  const cur = getThemePreference();
  const next = order[(order.indexOf(cur) + 1) % order.length] ?? "dark";
  setTheme(next);
  return resolveTheme(next);
}
