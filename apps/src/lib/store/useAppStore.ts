import { create } from "zustand";
import { AppSettings, RuntimeCapabilities, ServiceStatus } from "../../types";
import {
  DEFAULT_CODEX_ORIGINATOR,
  DEFAULT_CODEX_USER_AGENT_VERSION,
} from "../constants/codex";
import {
  DEFAULT_AUTHOR_SERVER_RECOMMENDATIONS,
  DEFAULT_AUTHOR_SPONSORS,
} from "../sponsor-links";
import {
  type TopLevelRoutePath,
  toTopLevelRoutePath,
} from "../app-shell/top-level-routes";
import { buildStaticRouteUrl } from "../utils/static-routes";

interface AppState {
  serviceStatus: ServiceStatus;
  appSettings: AppSettings;
  runtimeCapabilities: RuntimeCapabilities | null;
  isSidebarOpen: boolean;
  isCodexCliGuideOpen: boolean;
  currentShellPath: TopLevelRoutePath;
  openShellTabs: TopLevelRoutePath[];
  
  setServiceStatus: (status: Partial<ServiceStatus>) => void;
  setAppSettings: (settings: Partial<AppSettings>) => void;
  setRuntimeCapabilities: (capabilities: RuntimeCapabilities | null) => void;
  toggleSidebar: () => void;
  setSidebarOpen: (open: boolean) => void;
  openCodexCliGuide: () => void;
  closeCodexCliGuide: () => void;
  syncShellPathFromLocation: (path: string) => void;
  navigateShellPath: (path: string, options?: { replace?: boolean }) => void;
  pruneShellTabs: (allowedPaths: string[], fallbackPath: string) => void;
  closeShellTab: (path: string) => TopLevelRoutePath | null;
}

const initialShellPath: TopLevelRoutePath = "/";

export const useAppStore = create<AppState>((set) => ({
  serviceStatus: {
    connected: false,
    version: "",
    uptime: 0,
    addr: "localhost:48760",
  },
  appSettings: {
    updateAutoCheck: true,
    closeToTrayOnClose: false,
    closeToTraySupported: false,
    lowTransparency: false,
    lightweightModeOnCloseToTray: false,
    codexCliGuideDismissed: false,
    webAccessPasswordConfigured: false,
    webAuthMode: "none",
    webAuthModeOptions: ["none", "password", "accounts"],
    distributionEnabled: false,
    billingModeLock: {
      accountModeLocked: false,
      distributionLocked: false,
      reasons: [],
    },
    appUsersConfigured: false,
    appUserCount: 0,
    locale: "zh-CN",
    localeOptions: ["zh-CN", "en", "ru", "ko"],
    serviceAddr: "localhost:48760",
    serviceListenMode: "loopback",
    serviceListenModeOptions: ["loopback", "all_interfaces"],
    routeStrategy: "ordered",
    routeStrategyOptions: ["ordered", "balanced"],
    freeAccountMaxModel: "auto",
    freeAccountMaxModelOptions: [
      "auto",
      "gpt-5",
      "gpt-5-codex",
      "gpt-5-codex-mini",
      "gpt-5.1",
      "gpt-5.1-codex",
      "gpt-5.1-codex-max",
      "gpt-5.1-codex-mini",
      "gpt-5.2",
      "gpt-5.2-codex",
      "gpt-5.3-codex",
      "gpt-5.4-mini",
      "gpt-5.4",
    ],
    modelForwardRules: "",
    accountMaxInflight: 1,
    quotaGuard: {
      enabled: true,
      primaryMinRemainingPercent: 5,
      secondaryMinRemainingPercent: 10,
      allowAllLowQuotaFallback: true,
    },
    gatewayOriginator: DEFAULT_CODEX_ORIGINATOR,
    gatewayOriginatorDefault: DEFAULT_CODEX_ORIGINATOR,
    gatewayUserAgentVersion: DEFAULT_CODEX_USER_AGENT_VERSION,
    gatewayUserAgentVersionDefault: DEFAULT_CODEX_USER_AGENT_VERSION,
    gatewayResidencyRequirement: "",
    gatewayResidencyRequirementOptions: ["", "us"],
    pluginMarketMode: "builtin",
    pluginMarketSourceUrl: "",
    authorSponsors: DEFAULT_AUTHOR_SPONSORS,
    authorServerRecommendations: DEFAULT_AUTHOR_SERVER_RECOMMENDATIONS,
    upstreamProxyUrl: "",
    upstreamStreamTimeoutMs: 300000,
    upstreamTotalTimeoutMs: 0,
    sseKeepaliveIntervalMs: 15000,
    backgroundTasks: {
      usagePollingEnabled: true,
      usagePollIntervalSecs: 600,
      gatewayKeepaliveEnabled: true,
      gatewayKeepaliveIntervalSecs: 180,
      tokenRefreshPollingEnabled: true,
      tokenRefreshPollIntervalSecs: 60,
      usageRefreshWorkers: 4,
      httpWorkerFactor: 4,
      httpWorkerMin: 8,
      httpStreamWorkerFactor: 1,
      httpStreamWorkerMin: 2,
    },
    envOverrides: {},
    envOverrideCatalog: [],
    envOverrideReservedKeys: [],
    envOverrideUnsupportedKeys: [],
    theme: "tech",
    appearancePreset: "classic",
  },
  runtimeCapabilities: null,
  isSidebarOpen: true,
  isCodexCliGuideOpen: false,
  currentShellPath: initialShellPath,
  openShellTabs: [initialShellPath],

  setServiceStatus: (status) => 
    set((state) => ({ serviceStatus: { ...state.serviceStatus, ...status } })),
  
  setAppSettings: (settings) =>
    set((state) => ({ appSettings: { ...state.appSettings, ...settings } })),

  setRuntimeCapabilities: (runtimeCapabilities) => set({ runtimeCapabilities }),
    
  toggleSidebar: () => set((state) => ({ isSidebarOpen: !state.isSidebarOpen })),
  
  setSidebarOpen: (open) => set({ isSidebarOpen: open }),

  openCodexCliGuide: () => set({ isCodexCliGuideOpen: true }),

  closeCodexCliGuide: () => set({ isCodexCliGuideOpen: false }),

  syncShellPathFromLocation: (path) =>
    set((state) => {
      const nextPath = toTopLevelRoutePath(path);
      return {
        currentShellPath: nextPath,
        openShellTabs: state.openShellTabs.includes(nextPath)
          ? state.openShellTabs
          : [...state.openShellTabs, nextPath],
      };
    }),

  navigateShellPath: (path, options) =>
    set((state) => {
      const nextPath = toTopLevelRoutePath(path);
      const nextTabs = state.openShellTabs.includes(nextPath)
        ? state.openShellTabs
        : [...state.openShellTabs, nextPath];

      if (typeof window !== "undefined") {
        const nextUrl = buildStaticRouteUrl(nextPath);
        if (options?.replace) {
          window.history.replaceState(window.history.state, "", nextUrl);
        } else if (window.location.pathname !== nextUrl) {
          window.history.pushState(window.history.state, "", nextUrl);
        }
      }

      return {
        currentShellPath: nextPath,
        openShellTabs: nextTabs,
      };
    }),

  pruneShellTabs: (allowedPaths, fallbackPath) =>
    set((state) => {
      const allowedSet = new Set(
        allowedPaths.map((path) => toTopLevelRoutePath(path)),
      );
      const fallback = allowedSet.has(toTopLevelRoutePath(fallbackPath))
        ? toTopLevelRoutePath(fallbackPath)
        : "/";
      const nextTabs = state.openShellTabs.filter((path) =>
        allowedSet.has(path),
      );
      const normalizedTabs = nextTabs.length > 0 ? nextTabs : [fallback];
      const nextCurrent = allowedSet.has(state.currentShellPath)
        ? state.currentShellPath
        : normalizedTabs[0] ?? fallback;

      if (
        typeof window !== "undefined" &&
        window.location.pathname !== buildStaticRouteUrl(nextCurrent)
      ) {
        window.history.replaceState(
          window.history.state,
          "",
          buildStaticRouteUrl(nextCurrent),
        );
      }

      return {
        currentShellPath: nextCurrent,
        openShellTabs: normalizedTabs,
      };
    }),

  closeShellTab: (path) => {
    let nextActivePath: TopLevelRoutePath | null = null;

    set((state) => {
      const normalizedPath = toTopLevelRoutePath(path);
      if (normalizedPath === "/") {
        return state;
      }

      const targetIndex = state.openShellTabs.indexOf(normalizedPath);
      if (targetIndex === -1) {
        return state;
      }

      const nextTabs = state.openShellTabs.filter((tab) => tab !== normalizedPath);
      nextActivePath =
        state.currentShellPath === normalizedPath
          ? nextTabs[targetIndex - 1] ?? nextTabs[targetIndex] ?? "/"
          : state.currentShellPath;

      if (
        typeof window !== "undefined" &&
        nextActivePath &&
        state.currentShellPath === normalizedPath
      ) {
        window.history.pushState(
          window.history.state,
          "",
          buildStaticRouteUrl(nextActivePath),
        );
      }

      return {
        currentShellPath: nextActivePath ?? state.currentShellPath,
        openShellTabs: nextTabs,
      };
    });

    return nextActivePath;
  },
}));
