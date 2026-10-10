"use client";

import { useSyncExternalStore } from "react";
import { getLocalDayRange, type LocalDayRange } from "@/lib/utils/time";

let sharedDayRange = getLocalDayRange();
const listeners = new Set<() => void>();
let intervalId: ReturnType<typeof setInterval> | null = null;

function refreshSharedDayRange() {
  const next = getLocalDayRange();
  if (
    sharedDayRange.dayStartTs === next.dayStartTs &&
    sharedDayRange.dayEndTs === next.dayEndTs &&
    sharedDayRange.timeZone === next.timeZone
  ) {
    return;
  }

  sharedDayRange = next;
  listeners.forEach((listener) => listener());
}

function subscribe(onStoreChange: () => void): () => void {
  listeners.add(onStoreChange);
  if (listeners.size === 1) {
    refreshSharedDayRange();
    intervalId = setInterval(refreshSharedDayRange, 60_000);
  }

  return () => {
    listeners.delete(onStoreChange);
    if (listeners.size === 0 && intervalId !== null) {
      clearInterval(intervalId);
      intervalId = null;
    }
  };
}

function getSnapshot(): LocalDayRange {
  return sharedDayRange;
}

/**
 * 函数 `useLocalDayRange`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-13
 *
 * # 参数
 * 无
 *
 * # 返回
 * 返回当前浏览器本地时区对应的当天时间范围
 */
export function useLocalDayRange(): LocalDayRange {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}
