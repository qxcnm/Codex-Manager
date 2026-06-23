export interface ProxyGeoLike {
  ip?: string | null;
  countryCode?: string | null;
  countryName?: string | null;
  regionName?: string | null;
  cityName?: string | null;
  geoCheckedAt?: number | null;
  geoError?: string | null;

  asn?: number | null;
  asOrg?: string | null;
  isp?: string | null;
  asDomain?: string | null;

  timezoneId?: string | null;
  timezoneOffset?: number | null;
  timezoneUtc?: string | null;

  flagImgUrl?: string | null;
  flagEmoji?: string | null;
}

export function normalizeCountryCode(code?: string | null): string | null {
  const normalized = String(code || "")
    .trim()
    .toUpperCase();
  return /^[A-Z]{2}$/.test(normalized) ? normalized : null;
}

export function countryCodeToFlag(code?: string | null): string {
  const normalized = normalizeCountryCode(code);

  if (!normalized) {
    return "🌐";
  }

  return normalized
    .split("")
    .map((char) => String.fromCodePoint(127397 + char.charCodeAt(0)))
    .join("");
}

export function formatProxyGeoCountryLabel(
  countryCode?: string | null,
  countryName?: string | null,
  t?: (key: string) => string
): string {
  const code = normalizeCountryCode(countryCode);
  const name = String(countryName || "").trim();

  if (name) return name;
  if (code) return code;
  return t ? t("未知") : "Unknown";
}

export function formatProxyGeoLocationParts(
  geo: ProxyGeoLike,
  t?: (key: string) => string
): string[] {
  const parts = [
    String(geo.cityName || "").trim(),
    formatProxyGeoCountryLabel(geo.countryCode, geo.countryName, t)
  ].filter(Boolean);

  return Array.from(new Set(parts));
}

export function formatProxyAsn(asn?: number | null): string | null {
  return typeof asn === "number" && Number.isFinite(asn) ? `AS${asn}` : null;
}

export function formatProxyProvider(
  isp?: string | null,
  asOrg?: string | null,
  asDomain?: string | null
): string | null {
  const provider = String(isp || asOrg || "").trim();
  const domain = String(asDomain || "").trim();

  if (provider && domain) return `${provider} (${domain})`;
  if (provider) return provider;
  if (domain) return domain;
  return null;
}

export function formatProxyTimezone(
  timezoneId?: string | null,
  timezoneUtc?: string | null
): string | null {
  const id = String(timezoneId || "").trim();
  const utc = String(timezoneUtc || "").trim();

  if (id && utc) return `${id} (${utc})`;
  if (id) return id;
  if (utc) return `UTC ${utc}`;
  return null;
}

export function resolveProxyFlagDisplay(
  countryCode?: string | null,
  flagEmoji?: string | null
): string {
  const emoji = String(flagEmoji || "").trim();
  if (emoji) return emoji;
  return countryCodeToFlag(countryCode);
}

export function formatProxyGeoTooltip(
  geo: ProxyGeoLike,
  t?: (key: string) => string
): string {
  const localT = t || ((key: string) => key);
  const lines: string[] = [];

  if (geo.ip) {
    lines.push(`${localT("IP")}: ${geo.ip}`);
  }

  const countryLabel = formatProxyGeoCountryLabel(
    geo.countryCode,
    geo.countryName,
    t
  );
  if (countryLabel && countryLabel !== (t ? t("未知") : "Unknown")) {
    lines.push(`${localT("国家")}: ${countryLabel}`);
  }

  const city = String(geo.cityName || "").trim();
  const region = String(geo.regionName || "").trim();
  if (city && region) {
    lines.push(`${localT("城市")}: ${city} (${region})`);
  } else if (city) {
    lines.push(`${localT("城市")}: ${city}`);
  } else if (region) {
    lines.push(`${localT("地区")}: ${region}`);
  }

  const asnLabel = formatProxyAsn(geo.asn);
  if (asnLabel) {
    lines.push(`ASN: ${asnLabel}`);
  }

  const providerLabel = formatProxyProvider(geo.isp, geo.asOrg, geo.asDomain);
  if (providerLabel) {
    lines.push(`Provider / ISP: ${providerLabel}`);
  }

  const timezoneLabel = formatProxyTimezone(geo.timezoneId, geo.timezoneUtc);
  if (timezoneLabel) {
    lines.push(`Timezone: ${timezoneLabel}`);
  }

  if (geo.geoError) {
    lines.push(`${localT("地理位置错误")}: ${geo.geoError}`);
  }

  return lines.length > 0 ? lines.join("\n") : localT("未知代理位置");
}

export function hasProxyGeo(geo: ProxyGeoLike): boolean {
  return Boolean(
    String(geo.ip || "").trim() ||
    normalizeCountryCode(geo.countryCode) ||
    String(geo.countryName || "").trim() ||
    String(geo.cityName || "").trim()
  );
}
