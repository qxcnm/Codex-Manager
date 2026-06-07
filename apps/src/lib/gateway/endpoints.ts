"use client";

import type { RuntimeMode } from "@/types";

const DEFAULT_GATEWAY_ADDR = "localhost:48760";

type ResolveGatewayOriginOptions = {
  browserOrigin?: string | null;
  runtimeMode?: RuntimeMode | string | null;
  serviceAddr?: string | null;
};

function normalizeText(value: string | null | undefined): string {
  return String(value || "").trim();
}

function replaceLoopbackHost(host: string): string {
  return host === "0.0.0.0" || host === "::" || host === "[::]"
    ? "localhost"
    : host;
}

function sanitizeGatewayUrl(url: URL): string {
  url.search = "";
  url.hash = "";
  return url.toString().replace(/\/$/, "");
}

function appendPathSegment(url: URL, segment: string): string {
  const path = url.pathname.replace(/\/+$/, "");
  url.pathname = path.endsWith(segment) ? path : `${path || ""}${segment}`;
  return sanitizeGatewayUrl(url);
}

export function normalizeGatewayOrigin(value: string | null | undefined): string {
  const normalized = normalizeText(value).replace(/\/+$/, "");
  if (!normalized) {
    return "";
  }
  if (/^https?:\/\//i.test(normalized)) {
    return normalized;
  }
  return `http://${normalized}`;
}

export function normalizeGatewayServiceOrigin(
  serviceAddr: string | null | undefined,
  fallbackAddr = DEFAULT_GATEWAY_ADDR,
): string {
  const raw = normalizeText(serviceAddr) || normalizeText(fallbackAddr);
  if (!raw) {
    return "";
  }
  const value = raw.replace(/^https?:\/\//i, "");
  const target = value.split("/")[0] || normalizeText(fallbackAddr);

  try {
    const url = new URL(`http://${target}`);
    url.hostname = replaceLoopbackHost(url.hostname);
    return sanitizeGatewayUrl(url);
  } catch {
    return normalizeGatewayOrigin(fallbackAddr);
  }
}

export function normalizeGatewayPublicOrigin(
  publicOrigin: string | null | undefined,
): string {
  const raw = normalizeText(publicOrigin);
  if (!raw) {
    return "";
  }

  try {
    const url = new URL(raw);
    if (url.protocol !== "http:" && url.protocol !== "https:") {
      return "";
    }
    return sanitizeGatewayUrl(url);
  } catch {
    return "";
  }
}

export function resolveGatewayOrigin({
  browserOrigin,
  runtimeMode,
  serviceAddr,
}: ResolveGatewayOriginOptions): string {
  const webOrigin =
    runtimeMode === "web-gateway" ? normalizeGatewayPublicOrigin(browserOrigin) : "";
  if (webOrigin) {
    return webOrigin;
  }

  return normalizeGatewayServiceOrigin(serviceAddr, DEFAULT_GATEWAY_ADDR);
}

export function buildOpenAiGatewayEndpoint(origin: string): string {
  const normalized = normalizeGatewayOrigin(origin);
  if (!normalized) {
    return "";
  }

  try {
    const url = new URL(normalized);
    return appendPathSegment(url, "/v1");
  } catch {
    return "";
  }
}

export function buildOpenAiGatewayEndpointFromServiceAddr(
  serviceAddr: string | null | undefined,
  fallbackAddr = DEFAULT_GATEWAY_ADDR,
): string {
  return buildOpenAiGatewayEndpoint(
    normalizeGatewayServiceOrigin(serviceAddr, fallbackAddr),
  );
}

export function buildOpenAiGatewayEndpointFromPublicOrigin(
  publicOrigin: string | null | undefined,
): string {
  const normalized = normalizeGatewayPublicOrigin(publicOrigin);
  return normalized ? buildOpenAiGatewayEndpoint(normalized) : "";
}

export function buildGatewayRootEndpoint(origin: string): string {
  const normalized = normalizeGatewayOrigin(origin);
  if (!normalized) {
    return "";
  }

  try {
    const url = new URL(normalized);
    url.pathname = url.pathname.replace(/\/(?:v1|v1alpha|v1beta)\/?$/i, "") || "/";
    return sanitizeGatewayUrl(url);
  } catch {
    return "";
  }
}

export function buildClaudeCodeGatewayEndpoint(origin: string): string {
  return buildGatewayRootEndpoint(origin);
}

export function buildGeminiGatewayEndpoint(origin: string): string {
  return buildGatewayRootEndpoint(origin);
}