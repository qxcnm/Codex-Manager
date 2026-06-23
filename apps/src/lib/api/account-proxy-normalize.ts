import type { Account } from "@/types";
import { normalizeCountryCode } from "@/lib/utils/proxy-geo";

export type AccountProxySummaryFields = Pick<
  Account,
  | "proxyEnabled"
  | "proxySource"
  | "proxyProfileId"
  | "proxyProfileName"
  | "proxyStatus"
  | "proxyUrl"
  | "proxyIp"
  | "proxyCountryCode"
  | "proxyCountryName"
  | "proxyRegionName"
  | "proxyCityName"
  | "proxyGeoCheckedAt"
  | "proxyAsn"
  | "proxyAsOrg"
  | "proxyIsp"
  | "proxyAsDomain"
  | "proxyTimezoneId"
  | "proxyTimezoneUtc"
  | "proxyFlagImgUrl"
  | "proxyFlagEmoji"
>;

function asString(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function toNullableNumber(value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string" && value.trim()) {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

function toNullableBoolean(value: unknown): boolean | null {
  if (typeof value === "boolean") return value;
  if (typeof value === "number" && Number.isFinite(value)) return value !== 0;
  if (typeof value === "string") {
    const normalized = value.trim().toLowerCase();
    if (["1", "true", "yes", "on"].includes(normalized)) return true;
    if (["0", "false", "no", "off"].includes(normalized)) return false;
  }
  return null;
}

export function normalizeAccountProxySummaryFields(
  source: Record<string, unknown>,
): AccountProxySummaryFields {
  return {
    proxyEnabled: toNullableBoolean(source.proxyEnabled ?? source.proxy_enabled),
    proxySource: asString(source.proxySource ?? source.proxy_source) || null,
    proxyProfileId:
      asString(source.proxyProfileId ?? source.proxy_profile_id) || null,
    proxyProfileName:
      asString(source.proxyProfileName ?? source.proxy_profile_name) || null,
    proxyStatus: asString(source.proxyStatus ?? source.proxy_status) || null,
    proxyUrl: asString(source.proxyUrl ?? source.proxy_url) || null,
    proxyIp: asString(source.proxyIp ?? source.proxy_ip) || null,
    proxyCountryCode:
      normalizeCountryCode(
        asString(source.proxyCountryCode ?? source.proxy_country_code),
      ) || null,
    proxyCountryName:
      asString(source.proxyCountryName ?? source.proxy_country_name) || null,
    proxyRegionName:
      asString(source.proxyRegionName ?? source.proxy_region_name) || null,
    proxyCityName:
      asString(source.proxyCityName ?? source.proxy_city_name) || null,
    proxyGeoCheckedAt: toNullableNumber(
      source.proxyGeoCheckedAt ?? source.proxy_geo_checked_at,
    ),
    proxyAsn: toNullableNumber(source.proxyAsn ?? source.proxy_asn),
    proxyAsOrg:
      asString(source.proxyAsOrg ?? source.proxy_as_org) || null,
    proxyIsp:
      asString(source.proxyIsp ?? source.proxy_isp) || null,
    proxyAsDomain:
      asString(source.proxyAsDomain ?? source.proxy_as_domain) || null,
    proxyTimezoneId:
      asString(source.proxyTimezoneId ?? source.proxy_timezone_id) || null,
    proxyTimezoneUtc:
      asString(source.proxyTimezoneUtc ?? source.proxy_timezone_utc) || null,
    proxyFlagImgUrl:
      asString(source.proxyFlagImgUrl ?? source.proxy_flag_img_url) || null,
    proxyFlagEmoji:
      asString(source.proxyFlagEmoji ?? source.proxy_flag_emoji) || null,
  };
}
