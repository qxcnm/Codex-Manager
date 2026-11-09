import type { Account } from "@/types";

export function resolveBpsBatch(
  accounts: readonly Account[],
  selectedIds: readonly string[],
  enabled: boolean,
) {
  const ids = [...new Set(selectedIds.map((id) => id.trim()).filter(Boolean))];
  const byId = new Map(accounts.map((account) => [account.id, account]));
  const missingIds = ids.filter((id) => !byId.has(id));
  const unauthorizedIds = enabled
    ? ids.filter((id) => byId.has(id) && byId.get(id)?.bpsAuthorized !== true)
    : [];
  return { accountIds: ids, missingIds, unauthorizedIds };
}

export function normalizeBpsSummary(source: Record<string, unknown>) {
  const rawExpiresAt = source.bpsExpiresAt ?? source.bps_expires_at;
  return {
    bpsEnabled: (source.bpsEnabled ?? source.bps_enabled) === true,
    bpsAuthorized: (source.bpsAuthorized ?? source.bps_authorized) === true,
    bpsExpiresAt: typeof rawExpiresAt === "number" && Number.isFinite(rawExpiresAt)
      ? rawExpiresAt
      : null,
  };
}

export function isBpsAuthorizationUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === "https:" && url.hostname === "auth.openai.com"
      && !url.username && !url.password && (!url.port || url.port === "443");
  } catch {
    return false;
  }
}
