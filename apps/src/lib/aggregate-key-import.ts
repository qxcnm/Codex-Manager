export interface AggregateKeyImportItem {
  url: string;
  key: string;
  supplierName: string;
}

const AGGREGATE_AUTO_PROBE_STORAGE_KEY = "openruntime:auto-probe:aggregate";

export function readAggregateAutoProbe(): boolean {
  if (typeof window === "undefined") return true;
  return window.localStorage.getItem(AGGREGATE_AUTO_PROBE_STORAGE_KEY) !== "false";
}

export function writeAggregateAutoProbe(enabled: boolean): void {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(AGGREGATE_AUTO_PROBE_STORAGE_KEY, enabled ? "true" : "false");
}

const URL_FIELD_NAMES = ["url", "baseUrl", "base_url", "apiUrl", "api_url", "endpoint"] as const;
const KEY_FIELD_NAMES = ["key", "apiKey", "api_key", "token"] as const;
const API_KEY_PATTERN = /(?:sk|sess|key)-[A-Za-z0-9._-]{12,}/g;

function normalizedUrl(value: string): string | null {
  const trimmed = value.trim().replace(/[\s,;|]+$/g, "");
  try {
    const parsed = new URL(trimmed);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return null;
    parsed.hash = "";
    return parsed.toString().replace(/\/$/, "");
  } catch {
    return null;
  }
}

function supplierNameForUrl(url: string): string {
  try {
    return new URL(url).hostname || "OpenAI upstream";
  } catch {
    return "OpenAI upstream";
  }
}

function firstTextField(
  source: Record<string, unknown>,
  fields: readonly string[],
): string | null {
  for (const field of fields) {
    const value = source[field];
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return null;
}

function collectJsonItems(value: unknown, output: AggregateKeyImportItem[]): void {
  if (Array.isArray(value)) {
    value.forEach((item) => collectJsonItems(item, output));
    return;
  }
  if (!value || typeof value !== "object") return;

  const record = value as Record<string, unknown>;
  const rawUrl = firstTextField(record, URL_FIELD_NAMES);
  const key = firstTextField(record, KEY_FIELD_NAMES);
  const url = rawUrl ? normalizedUrl(rawUrl) : null;
  if (url && key) {
    output.push({ url, key, supplierName: supplierNameForUrl(url) });
    return;
  }
  Object.values(record).forEach((item) => collectJsonItems(item, output));
}

function parseConcatenatedPair(text: string): AggregateKeyImportItem | null {
  const match = text.trim().match(/^(https?:\/\/.+?)((?:sk|sess|key)-[A-Za-z0-9._-]{12,})$/i);
  if (!match) return null;
  const url = normalizedUrl(match[1]);
  if (!url) return null;
  return { url, key: match[2], supplierName: supplierNameForUrl(url) };
}

function parseDelimitedPair(text: string): AggregateKeyImportItem | null {
  const key = text.match(API_KEY_PATTERN)?.[0];
  if (!key) return null;
  const keyOffset = text.indexOf(key);
  const beforeKey = text.slice(0, keyOffset).trim().replace(/[\s,;|:-]+$/g, "");
  const urlMatch = beforeKey.match(/https?:\/\/[^\s,;|]+/i);
  const url = urlMatch ? normalizedUrl(urlMatch[0]) : null;
  if (!url) return null;
  return { url, key, supplierName: supplierNameForUrl(url) };
}

function dedupe(items: AggregateKeyImportItem[]): AggregateKeyImportItem[] {
  const seen = new Set<string>();
  return items.filter((item) => {
    const identity = `${item.url}\n${item.key}`;
    if (seen.has(identity)) return false;
    seen.add(identity);
    return true;
  });
}

/**
 * 支持 JSON、`URL KEY`、`URL----KEY`，以及用户经常直接粘贴的 `URLsk-...`。
 */
export function parseAggregateKeyImport(content: string): AggregateKeyImportItem[] {
  const trimmed = content.trim();
  if (!trimmed) return [];

  try {
    const parsed = JSON.parse(trimmed) as unknown;
    const jsonItems: AggregateKeyImportItem[] = [];
    collectJsonItems(parsed, jsonItems);
    if (jsonItems.length > 0) return dedupe(jsonItems);
  } catch {
    // 普通文本继续按行识别。
  }

  const items = trimmed
    .split(/\r?\n/)
    .map((line) => parseConcatenatedPair(line) ?? parseDelimitedPair(line))
    .filter((item): item is AggregateKeyImportItem => item !== null);
  if (items.length > 0) return dedupe(items);

  // 兼容 URL 和 KEY 分别占一行或被一段说明文字隔开的情况。
  const urlMatch = trimmed.match(/https?:\/\/[^\s,;|]+/i);
  const key = trimmed.match(API_KEY_PATTERN)?.[0];
  if (!urlMatch || !key) return [];
  const rawUrl = urlMatch[0].endsWith(key) ? urlMatch[0].slice(0, -key.length) : urlMatch[0];
  const url = normalizedUrl(rawUrl);
  return url ? [{ url, key, supplierName: supplierNameForUrl(url) }] : [];
}

export function maskAggregateKey(key: string): string {
  if (key.length <= 12) return "••••••••";
  return `${key.slice(0, 5)}••••${key.slice(-4)}`;
}
