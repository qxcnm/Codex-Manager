import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { fetchWithRetry, runWithControl, RequestOptions } from "../utils/request";
import { DEFAULT_UNSUPPORTED_WEB_REASON } from "../runtime/runtime-capabilities";
import { useAppStore } from "../store/useAppStore";
import {
  isCommandMissingError,
  unwrapRpcPayload,
} from "./transport-errors";
export { getAppErrorMessage, isCommandMissingError } from "./transport-errors";
import { createWebCommandMap } from "./transport-web-commands";
import type { InvokeParams, WebCommandDescriptor } from "./transport-web-commands";
import { postJsonRpc } from "./rpc-http";
import {
  isTauriRuntime,
  loadRuntimeCapabilities,
} from "./transport-runtime";
export {
  getCachedRuntimeCapabilities,
  isTauriRuntime,
  loadRuntimeCapabilities,
} from "./transport-runtime";

const WEB_COMMAND_MAP: Record<string, WebCommandDescriptor> =
  createWebCommandMap(postWebRpc);

/**
 * 函数 `invokeWebRpc`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - method: 参数 method
 * - params?: 参数 params?
 * - options: 参数 options
 *
 * # 返回
 * 返回函数执行结果
 */
async function invokeWebRpc<T>(
  method: string,
  params?: InvokeParams,
  options: RequestOptions = {}
): Promise<T> {
  const descriptor = WEB_COMMAND_MAP[method];
  if (!descriptor) {
    throw new Error("当前 Web / Docker 版暂不支持该操作");
  }
  if (descriptor.direct) {
    return (await descriptor.direct(params, options)) as T;
  }
  if (!descriptor.rpcMethod) {
    throw new Error("当前 Web / Docker 版暂不支持该操作");
  }
  return postWebRpc<T>(
    descriptor.rpcMethod,
    descriptor.mapParams ? descriptor.mapParams(params) : params ?? {},
    options
  );
}

/**
 * 函数 `postWebRpc`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - rpcMethod: 参数 rpcMethod
 * - params?: 参数 params?
 * - options: 参数 options
 *
 * # 返回
 * 返回函数执行结果
 */
async function postWebRpc<T>(
  rpcMethod: string,
  params?: InvokeParams,
  options: RequestOptions = {}
): Promise<T> {
  const runtimeCapabilities = await loadRuntimeCapabilities();
  if (runtimeCapabilities.mode === "unsupported-web") {
    throw new Error(
      runtimeCapabilities.unsupportedReason || DEFAULT_UNSUPPORTED_WEB_REASON
    );
  }

  return postJsonRpc<T>(
    fetchWithRetry,
    runtimeCapabilities.rpcBaseUrl,
    rpcMethod,
    params ?? {},
    options
  );
}

/**
 * 函数 `withAddr`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - params: 参数 params
 *
 * # 返回
 * 返回函数执行结果
 */
export function withAddr(
  params: Record<string, unknown> = {}
): Record<string, unknown> {
  const addr = useAppStore.getState().serviceStatus.addr;
  return {
    addr: addr || null,
    ...params,
  };
}

/**
 * 函数 `invokeFirst`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - methods: 参数 methods
 * - params?: 参数 params?
 * - options: 参数 options
 *
 * # 返回
 * 返回函数执行结果
 */
export async function invokeFirst<T>(
  methods: string[],
  params?: Record<string, unknown>,
  options: RequestOptions = {}
): Promise<T> {
  let lastErr: unknown;
  for (const method of methods) {
    try {
      return await invoke<T>(method, params, options);
    } catch (err) {
      lastErr = err;
      if (!isCommandMissingError(err)) {
        throw err;
      }
    }
  }
  throw lastErr || new Error("未配置可用命令");
}

/**
 * 函数 `invoke`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - method: 参数 method
 * - params?: 参数 params?
 * - options: 参数 options
 *
 * # 返回
 * 返回函数执行结果
 */
export async function invoke<T>(
  method: string,
  params?: InvokeParams,
  options: RequestOptions = {}
): Promise<T> {
  if (!isTauriRuntime()) {
    return invokeWebRpc(method, params, options);
  }

  const response = await runWithControl<unknown>(
    () => tauriInvoke(method, params || {}),
    options
  );
  return unwrapRpcPayload<T>(response);
}

/**
 * 函数 `requestlogListViaHttpRpc`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - params: 参数 params
 * - addr: 参数 addr
 * - options: 参数 options
 *
 * # 返回
 * 返回函数执行结果
 */
export async function requestlogListViaHttpRpc<T>(
  params: {
    query?: string;
    statusFilter?: string;
    page?: number;
    pageSize?: number;
  },
  addr: string,
  options: RequestOptions = {}
): Promise<T> {
  // Desktop environment should use Tauri invoke for reliability
  if (isTauriRuntime()) {
    return invoke<T>(
      "service_requestlog_list",
      {
        query: params.query || "",
        statusFilter: params.statusFilter || "all",
        page: params.page ?? 1,
        pageSize: params.pageSize ?? 20,
        addr,
      },
      options
    );
  }

  // Fallback for web mode if needed (though not primary for this app)
  return postJsonRpc<T>(
    fetchWithRetry,
    `http://${addr}/rpc`,
    "requestlog/list",
    {
      query: params.query || "",
      statusFilter: params.statusFilter || "all",
      page: params.page ?? 1,
      pageSize: params.pageSize ?? 20,
    },
    options
  );
}
