use super::{
    resolve_gateway_upstream_execution_plan, resolve_gateway_upstream_executor_kind,
    GatewayUpstreamExecutionPlan, GatewayUpstreamExecutorKind, GatewayUpstreamRouteKind,
};

#[test]
fn protocol_type_maps_to_executor_kind() {
    assert_eq!(
        resolve_gateway_upstream_executor_kind("openai_compat"),
        GatewayUpstreamExecutorKind::CodexResponses
    );
    assert_eq!(
        resolve_gateway_upstream_executor_kind("anthropic_native"),
        GatewayUpstreamExecutorKind::Claude
    );
    assert_eq!(
        resolve_gateway_upstream_executor_kind("gemini_native"),
        GatewayUpstreamExecutorKind::Gemini
    );
}

#[test]
fn protocol_and_rotation_map_to_execution_plan() {
    assert_eq!(
        resolve_gateway_upstream_execution_plan("openai_compat", "account_rotation"),
        GatewayUpstreamExecutionPlan {
            executor_kind: GatewayUpstreamExecutorKind::CodexResponses,
            route_kind: GatewayUpstreamRouteKind::AccountRotation,
        }
    );
    assert_eq!(
        resolve_gateway_upstream_execution_plan("anthropic_native", "aggregate_api_rotation"),
        GatewayUpstreamExecutionPlan {
            executor_kind: GatewayUpstreamExecutorKind::Claude,
            route_kind: GatewayUpstreamRouteKind::AggregateApi,
        }
    );
    assert_eq!(
        resolve_gateway_upstream_execution_plan("openai_compat", "hybrid_rotation"),
        GatewayUpstreamExecutionPlan {
            executor_kind: GatewayUpstreamExecutorKind::CodexResponses,
            route_kind: GatewayUpstreamRouteKind::HybridAccountFirst,
        }
    );
    assert_eq!(
        resolve_gateway_upstream_execution_plan("openai_compat", "hybrid_aggregate_first_rotation"),
        GatewayUpstreamExecutionPlan {
            executor_kind: GatewayUpstreamExecutorKind::CodexResponses,
            route_kind: GatewayUpstreamRouteKind::HybridAggregateFirst,
        }
    );
    assert_eq!(
        resolve_gateway_upstream_execution_plan("gemini_native", "aggregate_api_rotation"),
        GatewayUpstreamExecutionPlan {
            executor_kind: GatewayUpstreamExecutorKind::Gemini,
            route_kind: GatewayUpstreamRouteKind::AggregateApi,
        }
    );
}
