import { invoke, withAddr } from "@/lib/api/transport";

export type ProxyProbeStatus = "unknown" | "available" | "failed";
export type ProxyProfileStatus = "active" | "disabled";
export type ProxyFallbackMode = "none" | "direct" | "proxy";
export type AccountProxyMode = "inherit" | "direct" | "profile";

export interface ProxyProfile {
  id: string;
  name: string;
  proxyUrl: string;
  proxyUsername: string | null;
  hasPassword: boolean;
  status: ProxyProfileStatus;
  fallbackMode: ProxyFallbackMode;
  backupProxyId: string | null;
  exitIp: string | null;
  countryCode: string | null;
  region: string | null;
  latencyMs: number | null;
  lastProbeStatus: ProxyProbeStatus;
  lastProbeError: string | null;
  lastProbeAt: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface AccountProxyBinding {
  accountId: string;
  mode: AccountProxyMode;
  proxyProfileId: string | null;
  updatedAt: number;
}

export interface ProxyProfilesSnapshot {
  items: ProxyProfile[];
  bindings: AccountProxyBinding[];
}

export interface ProxyProfileInput {
  id?: string;
  name: string;
  proxyUrl: string;
  username?: string;
  password?: string;
  keepExistingPassword?: boolean;
  status?: ProxyProfileStatus;
  fallbackMode?: ProxyFallbackMode;
  backupProxyId?: string;
}

export function listProxyProfiles(): Promise<ProxyProfilesSnapshot> {
  return invoke("service_proxy_profiles_list", withAddr());
}

export function saveProxyProfile(input: ProxyProfileInput): Promise<ProxyProfile> {
  return invoke("service_proxy_profile_upsert", withAddr({ ...input }));
}

export function deleteProxyProfile(id: string): Promise<{ ok: boolean }> {
  return invoke("service_proxy_profile_delete", withAddr({ id }));
}

export function probeProxyProfile(id: string): Promise<ProxyProfile> {
  return invoke("service_proxy_profile_probe", withAddr({ id }));
}

export function bindProxyAccounts(
  accountIds: string[],
  mode: AccountProxyMode,
  proxyProfileId?: string,
): Promise<{ updated: number }> {
  return invoke("service_proxy_profile_bind_accounts", withAddr({
    accountIds,
    mode,
    proxyProfileId: proxyProfileId || null,
  }));
}
