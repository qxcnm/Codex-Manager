import {
  buildOpenAiGatewayEndpointFromPublicOrigin,
  buildOpenAiGatewayEndpointFromServiceAddr,
} from "../gateway/endpoints";

export const CCSWITCH_PROVIDER_IMPORT_BASE = "ccswitch://v1/import";

export interface CcSwitchProviderImportOptions {
  app?: "claude" | "codex" | "gemini" | "opencode" | "openclaw";
  name: string;
  endpoint: string;
  apiKey: string;
  model?: string | null;
  homepage?: string | null;
  notes?: string | null;
  enabled?: boolean;
}

export interface CcSwitchGatewayEndpointOptions {
  publicOrigin?: string | null;
  preferPublicOrigin?: boolean;
}

function normalizeText(value: string | null | undefined): string {
  return String(value || "").trim();
}

export function normalizeCodexManagerGatewayPublicEndpoint(
  publicOrigin?: string | null,
): string | null {
  const endpoint = buildOpenAiGatewayEndpointFromPublicOrigin(publicOrigin);
  return endpoint || null;
}

export function normalizeCodexManagerGatewayEndpoint(
  serviceAddr?: string | null,
  options: CcSwitchGatewayEndpointOptions = {},
): string {
  if (options.preferPublicOrigin) {
    const publicEndpoint = normalizeCodexManagerGatewayPublicEndpoint(
      options.publicOrigin,
    );
    if (publicEndpoint) {
      return publicEndpoint;
    }
  }

  return buildOpenAiGatewayEndpointFromServiceAddr(serviceAddr);
}

export function buildCcSwitchProviderName(name?: string | null, id?: string | null): string {
  const label = normalizeText(name) || normalizeText(id) || "Platform Key";
  return label.toLowerCase().startsWith("codexmanager")
    ? label
    : `CodexManager - ${label}`;
}

export function buildCcSwitchProviderImportUrl(
  options: CcSwitchProviderImportOptions,
): string {
  const params = new URLSearchParams({
    resource: "provider",
    app: options.app || "codex",
    name: normalizeText(options.name) || "CodexManager",
    endpoint: normalizeText(options.endpoint),
    apiKey: normalizeText(options.apiKey),
  });

  const model = normalizeText(options.model);
  const homepage = normalizeText(options.homepage);
  const notes = normalizeText(options.notes);

  if (model) params.set("model", model);
  if (homepage) params.set("homepage", homepage);
  if (notes) params.set("notes", notes);
  if (typeof options.enabled === "boolean") {
    params.set("enabled", String(options.enabled));
  }

  return `${CCSWITCH_PROVIDER_IMPORT_BASE}?${params.toString()}`;
}