"use client";

export interface LocalDayRange {
  dayStartTs: number;
  dayEndTs: number;
  timeZone: string | null;
}

function getPreferredLocale(): string | undefined {
  if (typeof document !== "undefined") {
    const lang = document.documentElement.lang.trim();
    if (lang) {
      return lang;
    }
  }
  if (typeof navigator !== "undefined") {
    const language = String(navigator.language || "").trim();
    if (language) {
      return language;
    }
  }
  return undefined;
}

export function getBrowserTimeZone(): string | null {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || null;
  } catch {
    return null;
  }
}

export function getLocalDayRange(referenceDate = new Date()): LocalDayRange {
  const start = new Date(
    referenceDate.getFullYear(),
    referenceDate.getMonth(),
    referenceDate.getDate(),
    0,
    0,
    0,
    0,
  );
  const end = new Date(
    referenceDate.getFullYear(),
    referenceDate.getMonth(),
    referenceDate.getDate() + 1,
    0,
    0,
    0,
    0,
  );

  return {
    dayStartTs: Math.floor(start.getTime() / 1000),
    dayEndTs: Math.floor(end.getTime() / 1000),
    timeZone: getBrowserTimeZone(),
  };
}

export function formatLocalDateTimeFromSeconds(
  timestamp: number | null | undefined,
  emptyLabel = "未知",
): string {
  if (!timestamp) return emptyLabel;
  const date = new Date(timestamp * 1000);
  if (Number.isNaN(date.getTime())) return emptyLabel;

  return new Intl.DateTimeFormat(getPreferredLocale(), {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(date);
}

export function formatLocalMinuteFromSeconds(
  timestamp: number | null | undefined,
  emptyLabel = "未知",
): string {
  if (!timestamp) return emptyLabel;
  const date = new Date(timestamp * 1000);
  if (Number.isNaN(date.getTime())) return emptyLabel;

  return new Intl.DateTimeFormat(getPreferredLocale(), {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(date);
}
