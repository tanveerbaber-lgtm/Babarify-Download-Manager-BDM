import { useCallback, useEffect, useState } from "react";
import { api } from "../lib/api";
import type { AppError, Settings, SettingsPatch, Theme } from "../lib/types";

const THEME_CACHE_KEY = "bdm.theme";

/** Applies the theme to <html> and caches it so the next launch paints without a flash. */
function applyTheme(theme: Theme) {
  const root = document.documentElement;
  const dark =
    theme === "dark" ||
    (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  root.dataset.theme = dark ? "dark" : "light";
  root.style.colorScheme = dark ? "dark" : "light";
  try {
    localStorage.setItem(THEME_CACHE_KEY, theme);
  } catch {
    // Storage is a convenience only; the database remains the source of truth.
  }
}

export function useSettings() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<AppError | null>(null);

  useEffect(() => {
    api
      .getSettings()
      .then(setSettings)
      .catch((err: AppError) => setError(err));
  }, []);

  const theme = settings?.theme;
  useEffect(() => {
    if (!theme) return;
    applyTheme(theme);
    if (theme !== "system") return;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => applyTheme("system");
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, [theme]);

  const update = useCallback(async (patch: SettingsPatch) => {
    const next = await api.updateSettings(patch);
    setSettings(next);
    return next;
  }, []);

  return { settings, error, update };
}
