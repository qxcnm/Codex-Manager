import type { SponsorLinkItem } from "../lib/sponsor-links";

export interface EnvOverrideCatalogItem {
  key: string;
  label: string;
  defaultValue: string;
  scope: string;
  applyMode: string;
  riskLevel: string;
  effectScope: string;
  safetyNote: string;
}

export interface BackgroundTaskSettings {
  usagePollingEnabled: boolean;
  usagePollIntervalSecs: number;
  gatewayKeepaliveEnabled: boolean;
  gatewayKeepaliveIntervalSecs: number;
  tokenRefreshPollingEnabled: boolean;
  tokenRefreshPollIntervalSecs: number;
  usageRefreshWorkers: number;
  httpWorkerFactor: number;
  httpWorkerMin: number;
  httpStreamWorkerFactor: number;
  httpStreamWorkerMin: number;
}

export interface QuotaGuardSettings {
  enabled: boolean;
  primaryMinRemainingPercent: number;
  secondaryMinRemainingPercent: number;
  allowAllLowQuotaFallback: boolean;
}

export interface AppSettings {
  updateAutoCheck: boolean;
  closeToTrayOnClose: boolean;
  closeToTraySupported: boolean;
  lowTransparency: boolean;
  lightweightModeOnCloseToTray: boolean;
  codexCliGuideDismissed: boolean;
  webAccessPasswordConfigured: boolean;
  webAuthMode: string;
  webAuthModeOptions: string[];
  distributionEnabled: boolean;
  billingModeLock: BillingModeLock;
  appUsersConfigured: boolean;
  appUserCount: number;
  locale: string;
  localeOptions: string[];
  serviceAddr: string;
  serviceListenMode: string;
  serviceListenModeOptions: string[];
  routeStrategy: string;
  routeStrategyOptions: string[];
  freeAccountMaxModel: string;
  freeAccountMaxModelOptions: string[];
  modelForwardRules: string;
  accountMaxInflight: number;
  quotaGuard: QuotaGuardSettings;
  gatewayOriginator: string;
  gatewayOriginatorDefault: string;
  gatewayUserAgentVersion: string;
  gatewayUserAgentVersionDefault: string;
  gatewayResidencyRequirement: string;
  gatewayResidencyRequirementOptions: string[];
  pluginMarketMode: string;
  pluginMarketSourceUrl: string;
  authorSponsors: SponsorLinkItem[];
  authorServerRecommendations: SponsorLinkItem[];
  upstreamProxyUrl: string;
  upstreamStreamTimeoutMs: number;
  upstreamTotalTimeoutMs: number;
  sseKeepaliveIntervalMs: number;
  backgroundTasks: BackgroundTaskSettings;
  envOverrides: Record<string, string>;
  envOverrideCatalog: EnvOverrideCatalogItem[];
  envOverrideReservedKeys: string[];
  envOverrideUnsupportedKeys: string[];
  theme: string;
  appearancePreset: string;
  [key: string]: unknown;
}

export interface BillingModeLock {
  accountModeLocked: boolean;
  distributionLocked: boolean;
  reasons: string[];
}

export interface CodexLatestVersionInfo {
  packageName: string;
  version: string;
  distTag: string;
  registryUrl: string;
}

export interface AccountManagerStatus {
  mode: string;
  modeOptions: string[];
  passwordConfigured: boolean;
  appUsersConfigured: boolean;
  appUserCount: number;
  activeAdminCount: number;
  distributionEnabled: boolean;
  billingModeLock: BillingModeLock;
}

export interface AppWallet {
  id: string;
  ownerKind: string;
  ownerId: string;
  balanceCreditMicros: number;
  frozenCreditMicros: number;
  availableCreditMicros: number;
  status: string;
  createdAt: number;
  updatedAt: number;
}

export interface AppUser {
  id: string;
  username: string;
  displayName?: string | null;
  role: string;
  status: string;
  createdAt: number;
  updatedAt: number;
  lastLoginAt?: number | null;
  wallet?: AppWallet | null;
}

export interface ApiKeyOwner {
  keyId: string;
  ownerKind: string;
  ownerUserId?: string | null;
  projectId?: string | null;
  updatedAt: number;
}

export type AppRole = "system_admin" | "admin" | "member";
export type AppPermission =
  | "system:admin"
  | "apikey:self"
  | "requestlog:self"
  | "models:read"
  | "profile:self";

export interface AppSessionResult {
  mode: string;
  currentUser?: AppUser | null;
  role: AppRole;
  permissions: AppPermission[];
  distributionEnabled: boolean;
  billingModeLock: BillingModeLock;
}
