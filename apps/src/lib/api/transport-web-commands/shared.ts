import type { RequestOptions } from "../../utils/request";

export type InvokeParams = Record<string, unknown>;

export type WebCommandDescriptor = {
  rpcMethod?: string;
  mapParams?: (params?: InvokeParams) => InvokeParams;
  direct?: (params?: InvokeParams, options?: RequestOptions) => Promise<unknown>;
};

export type WebRpcCaller = <T>(
  rpcMethod: string,
  params?: InvokeParams,
  options?: RequestOptions
) => Promise<T>;

export function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

export function mapKeyIdToId(params?: InvokeParams): InvokeParams {
  const source = params ?? {};
  const keyId =
    typeof source.keyId === "string" && source.keyId.trim()
      ? source.keyId.trim()
      : undefined;
  if (!keyId) {
    return source;
  }
  return {
    ...source,
    id: keyId,
  };
}