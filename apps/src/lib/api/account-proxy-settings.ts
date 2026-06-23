import type { ProxyGeoLike } from "@/lib/utils/proxy-geo";

export type AccountProxySource = "custom" | "profile";

export interface AccountProxySettings extends ProxyGeoLike {
  accountId: string;
  enabled: boolean;
  source: AccountProxySource;
  proxyProfileId: string | null;
  proxyProfileName: string | null;
  proxyProfileEnabled: boolean | null;
  proxyUrl: string;
  proxyUrlRedacted: string;
  status: string;
  latencyMs: number | null;
  lastDownloadMbps: number | null;
  lastUploadMbps: number | null;
  lastCheckAt: number | null;
  lastError: string | null;
}

export interface AccountProxySetPayload {
  accountId: string;
  enabled: boolean;
  source?: AccountProxySource | null;
  proxyProfileId?: string | null;
  proxyUrl?: string | null;
  status?: string | null;
  latencyMs?: number | null;
  lastError?: string | null;
  ip?: string | null;
  countryCode?: string | null;
  countryName?: string | null;
  regionName?: string | null;
  cityName?: string | null;
  geoCheckedAt?: number | null;
  geoError?: string | null;
}

export interface AccountProxyTestPayload {
  accountId: string;
  enabled?: boolean;
  source?: AccountProxySource | null;
  proxyProfileId?: string | null;
  proxyUrl?: string | null;
}

function readNumber(value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value)) {
    return value;
  }

  if (typeof value === "string" && value.trim()) {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }

  return null;
}

function readString(value: unknown): string {
  return typeof value === "string" ? value : value == null ? "" : String(value);
}

function readNullableString(value: unknown): string | null {
  const text = readString(value).trim();
  return text ? text : null;
}

function readNullableBoolean(value: unknown): boolean | null {
  if (typeof value === "boolean") return value;
  if (typeof value === "number" && Number.isFinite(value)) return value !== 0;
  if (typeof value === "string") {
    const normalized = value.trim().toLowerCase();
    if (["1", "true", "yes", "on"].includes(normalized)) return true;
    if (["0", "false", "no", "off"].includes(normalized)) return false;
  }
  return null;
}

export function readAccountProxySettings(payload: unknown): AccountProxySettings {
  const source =
    payload && typeof payload === "object"
      ? (payload as Record<string, unknown>)
      : {};

  return {
    accountId: readString(source.accountId ?? source.account_id),
    enabled: Boolean(source.enabled),
    source:
      readString(source.source).toLowerCase() === "profile" ? "profile" : "custom",
    proxyProfileId: readNullableString(
      source.proxyProfileId ?? source.proxy_profile_id,
    ),
    proxyProfileName: readNullableString(
      source.proxyProfileName ?? source.proxy_profile_name,
    ),
    proxyProfileEnabled: readNullableBoolean(
      source.proxyProfileEnabled ?? source.proxy_profile_enabled,
    ),
    proxyUrl: readString(source.proxyUrl ?? source.proxy_url),
    proxyUrlRedacted:
      readString(source.proxyUrlRedacted ?? source.proxy_url_redacted) ||
      "<invalid>",
    status: readString(source.status || "not_configured"),
    latencyMs: readNumber(source.latencyMs ?? source.latency_ms),
    lastDownloadMbps: readNumber(
      source.lastDownloadMbps ?? source.last_download_mbps,
    ),
    lastUploadMbps: readNumber(
      source.lastUploadMbps ?? source.last_upload_mbps,
    ),
    lastCheckAt: readNumber(source.lastCheckAt ?? source.last_check_at),
    lastError:
      source.lastError == null && source.last_error == null
        ? null
        : readString(source.lastError ?? source.last_error),
    ip: readNullableString(source.ip),
    countryCode: readNullableString(source.countryCode ?? source.country_code),
    countryName: readNullableString(source.countryName ?? source.country_name),
    regionName: readNullableString(source.regionName ?? source.region_name),
    cityName: readNullableString(source.cityName ?? source.city_name),
    geoCheckedAt: readNumber(source.geoCheckedAt ?? source.geo_checked_at),
    geoError: readNullableString(source.geoError ?? source.geo_error),

    asn: readNumber(source.asn),
    asOrg: readNullableString(source.asOrg ?? source.as_org),
    isp: readNullableString(source.isp),
    asDomain: readNullableString(source.asDomain ?? source.as_domain),

    timezoneId: readNullableString(source.timezoneId ?? source.timezone_id),
    timezoneOffset: readNumber(source.timezoneOffset ?? source.timezone_offset),
    timezoneUtc: readNullableString(source.timezoneUtc ?? source.timezone_utc),

    flagImgUrl: readNullableString(source.flagImgUrl ?? source.flag_img_url),
    flagEmoji: readNullableString(source.flagEmoji ?? source.flag_emoji),
  };
}
