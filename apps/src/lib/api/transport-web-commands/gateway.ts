import type { WebCommandDescriptor } from "./shared";

export function createGatewayWebCommands(): Record<string, WebCommandDescriptor> {
  return {
    service_gateway_transport_get: { rpcMethod: "gateway/transport/get" },
    service_gateway_transport_set: { rpcMethod: "gateway/transport/set" },
    service_gateway_upstream_proxy_get: { rpcMethod: "gateway/upstreamProxy/get" },
    service_gateway_upstream_proxy_set: { rpcMethod: "gateway/upstreamProxy/set" },
    service_gateway_route_strategy_get: { rpcMethod: "gateway/routeStrategy/get" },
    service_gateway_route_strategy_set: { rpcMethod: "gateway/routeStrategy/set" },
    service_gateway_manual_account_get: { rpcMethod: "gateway/manualAccount/get" },
    service_gateway_manual_account_set: { rpcMethod: "gateway/manualAccount/set" },
    service_gateway_manual_account_clear: { rpcMethod: "gateway/manualAccount/clear" },
    service_gateway_background_tasks_get: { rpcMethod: "gateway/backgroundTasks/get" },
    service_gateway_background_tasks_set: { rpcMethod: "gateway/backgroundTasks/set" },
    service_gateway_concurrency_recommend_get: { rpcMethod: "gateway/concurrencyRecommendation/get" },
    service_gateway_codex_latest_version_get: { rpcMethod: "gateway/codexLatestVersion/get" },
  };
}
