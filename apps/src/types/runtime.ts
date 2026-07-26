export type AvailabilityLevel = "ok" | "warn" | "bad" | "unknown";

export type RuntimeMode = "desktop-tauri" | "web-gateway" | "unsupported-web";

export interface RuntimeCapabilities {
  mode: RuntimeMode;
  rpcBaseUrl: string;
  authorContentUrl?: string | null;
  canManageService: boolean;
  canSelfUpdate: boolean;
  canAutoStart: boolean;
  canCloseToTray: boolean;
  canOpenLocalDir: boolean;
  canUseBrowserFileImport: boolean;
  canUseBrowserDownloadExport: boolean;
  unsupportedReason?: string | null;
}

export interface ServiceStatus {
  connected: boolean;
  version: string;
  uptime: number;
  addr: string;
}

export interface ServiceInitializationResult {
  version: string;
  userAgent: string;
  codexHome: string;
  platformFamily: string;
  platformOs: string;
}
