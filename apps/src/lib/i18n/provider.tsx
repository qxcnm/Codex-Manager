"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { toast } from "sonner";
import { appClient } from "@/lib/api/app-client";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useAppStore } from "@/lib/store/useAppStore";
import {
  type AppLocale,
  type AppLocalePreference,
  DEFAULT_LOCALE,
  DEFAULT_LOCALE_PREFERENCE,
  LOCALE_LABELS,
  LOCALE_PREFERENCES,
  normalizeLocale,
  normalizeLocalePreference,
  resolveLocale,
} from "./config";
import { translate } from "./messages";

type TranslationValues = Record<string, string | number>;

type I18nContextValue = {
  locale: AppLocale;
  localePreference: AppLocalePreference;
  localeOptions: AppLocalePreference[];
  isSwitchingLocale: boolean;
  setLocale: (locale: AppLocalePreference) => Promise<void>;
  t: (message: string, values?: TranslationValues) => string;
};

const I18nContext = createContext<I18nContextValue | null>(null);

export function I18nProvider({ children }: { children: React.ReactNode }) {
  const storedLocale = useAppStore((state) => state.appSettings.locale);
  const storedLocaleOptions = useAppStore((state) => state.appSettings.localeOptions);
  const setAppSettings = useAppStore((state) => state.setAppSettings);
  const [isSwitchingLocale, setIsSwitchingLocale] = useState(false);
  const [systemLocale, setSystemLocale] = useState<AppLocale>(DEFAULT_LOCALE);
  const localePreference = normalizeLocalePreference(storedLocale);
  const locale = resolveLocale(localePreference, systemLocale);
  const localeOptions = useMemo(
    () => Array.from(new Set(
      (storedLocaleOptions?.length ? storedLocaleOptions : LOCALE_PREFERENCES)
        .map(normalizeLocalePreference),
    )),
    [storedLocaleOptions],
  );

  useEffect(() => {
    const updateSystemLocale = () => {
      setSystemLocale(normalizeLocale(navigator.language || navigator.languages?.[0]));
    };
    updateSystemLocale();
    window.addEventListener("languagechange", updateSystemLocale);
    return () => window.removeEventListener("languagechange", updateSystemLocale);
  }, []);

  useEffect(() => {
    if (typeof document === "undefined") {
      return;
    }
    document.documentElement.lang = locale;
  }, [locale]);

  const t = useMemo(
    () => (message: string, values?: TranslationValues) => translate(locale, message, values),
    [locale],
  );

  const setLocale = useCallback(async (nextLocale: AppLocalePreference) => {
    const normalizedLocale = normalizeLocalePreference(nextLocale);
    if (normalizedLocale === localePreference) {
      return;
    }
    setIsSwitchingLocale(true);
    try {
      const settings = await appClient.setSettings({ locale: normalizedLocale });
      setAppSettings(settings);
      toast.success(translate(resolveLocale(normalizedLocale, systemLocale), "界面语言已切换"));
    } catch (error: unknown) {
      const message = getAppErrorMessage(error);
      if (message.includes("permission_denied")) {
        setAppSettings({ locale: normalizedLocale });
        toast.success(translate(resolveLocale(normalizedLocale, systemLocale), "界面语言已切换"));
        return;
      }
      toast.error(`${translate(locale, "语言切换失败")}: ${message}`);
    } finally {
      setIsSwitchingLocale(false);
    }
  }, [locale, localePreference, setAppSettings, systemLocale]);

  const value = useMemo<I18nContextValue>(
    () => ({
      locale,
      localePreference,
      localeOptions,
      isSwitchingLocale,
      setLocale,
      t,
    }),
    [isSwitchingLocale, locale, localePreference, localeOptions, setLocale, t],
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n() {
  const context = useContext(I18nContext);
  if (!context) {
    return {
      locale: DEFAULT_LOCALE,
      localePreference: DEFAULT_LOCALE_PREFERENCE,
      localeOptions: LOCALE_PREFERENCES.slice(),
      isSwitchingLocale: false,
      setLocale: async () => undefined,
      t: (message: string, values?: TranslationValues) =>
        translate(DEFAULT_LOCALE, message, values),
    };
  }
  return context;
}

export function getLocaleLabel(
  preference: AppLocalePreference,
  locale: AppLocale = DEFAULT_LOCALE,
): string {
  return preference === "auto" ? translate(locale, "自动检测") : LOCALE_LABELS[preference];
}
