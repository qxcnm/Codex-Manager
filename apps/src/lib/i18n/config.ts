"use client";

export const SUPPORTED_LOCALES = ["zh-CN", "en", "ru", "ko"] as const;

export type AppLocale = (typeof SUPPORTED_LOCALES)[number];
export const LOCALE_PREFERENCES = ["auto", ...SUPPORTED_LOCALES] as const;
export type AppLocalePreference = (typeof LOCALE_PREFERENCES)[number];
export const DEFAULT_LOCALE_PREFERENCE: AppLocalePreference = "auto";

export const DEFAULT_LOCALE: AppLocale = "en";

export const LOCALE_LABELS: Record<AppLocale, string> = {
  "zh-CN": "简体中文",
  en: "English",
  ru: "Русский",
  ko: "한국어",
};

function localeLanguage(value: unknown): string {
  return String(value || "")
    .trim()
    .toLowerCase()
    .split(/[-_.@]/)[0];
}

export function normalizeLocale(value: unknown): AppLocale {
  switch (localeLanguage(value)) {
    case "zh":
      return "zh-CN";
    case "en":
      return "en";
    case "ru":
      return "ru";
    case "ko":
      return "ko";
    default:
      return DEFAULT_LOCALE;
  }
}

export function normalizeLocalePreference(value: unknown): AppLocalePreference {
  const language = localeLanguage(value);
  return ["zh", "en", "ru", "ko"].includes(language)
    ? normalizeLocale(value)
    : DEFAULT_LOCALE_PREFERENCE;
}

export function resolveLocale(
  preference: AppLocalePreference,
  systemLocale: unknown,
): AppLocale {
  return preference === "auto" ? normalizeLocale(systemLocale) : preference;
}
