"use client";

import { type AppLocale } from "../config";
import { EN_MESSAGES } from "./en";
import { KO_MESSAGES } from "./ko";
import { RU_MESSAGES } from "./ru";
import type { MessageCatalog, TranslationValues } from "./types";

const MESSAGE_CATALOG: Record<AppLocale, MessageCatalog> = {
  "zh-CN": {},
  en: EN_MESSAGES,
  ru: RU_MESSAGES,
  ko: KO_MESSAGES,
};

function interpolate(template: string, values?: TranslationValues): string {
  if (!values) return template;
  return template.replace(/\{(\w+)\}/g, (_, key: string) =>
    Object.prototype.hasOwnProperty.call(values, key) ? String(values[key]) : `{${key}}`,
  );
}

export function translate(
  locale: AppLocale,
  message: string,
  values?: TranslationValues,
): string {
  const normalizedMessage = String(message || "");
  const template =
    locale === "zh-CN"
      ? normalizedMessage
      : MESSAGE_CATALOG[locale]?.[normalizedMessage] ??
        EN_MESSAGES[normalizedMessage] ??
        normalizedMessage;
  return interpolate(template, values);
}

export type { MessageCatalog, TranslationValues } from "./types";
