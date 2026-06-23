use super::{
    AccountListResult, AccountSummary, ApiKeyUsageStatSummary, DashboardAdminUsageSummaryResult,
    DashboardDailyUsagePoint, DashboardSourceUsageSummary, DashboardTokenUsageResult,
    DashboardUserUsageSummary, ProxyProfileEntry, ProxyProfileListResult, ProxyProfileUrlTestEntry,
    ProxyTestDefaults, ProxyTestFileSizePreset, ProxyTestPresetsResult,
    ProxyTestProviderFilePreset, ProxyTestSpeedProviderPreset, ProxyTestUploadEndpointStatus,
    RequestLogFilterSummaryResult, RequestLogListParams, RequestLogListResult, RequestLogSummary,
};

/// 函数 `account_summary_serialization_matches_compact_contract`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn account_summary_serialization_matches_compact_contract() {
    let summary = AccountSummary {
        id: "acc-1".to_string(),
        label: "主账号".to_string(),
        group_name: Some("TEAM".to_string()),
        preferred: true,
        sort: 10,
        status: "active".to_string(),
        status_reason: Some("account_deactivated".to_string()),
        has_token: true,
        plan_type: Some("team".to_string()),
        plan_type_raw: None,
        has_subscription: Some(true),
        subscription_plan: Some("team".to_string()),
        subscription_expires_at: Some(1_745_000_000),
        subscription_renews_at: Some(1_745_000_000),
        note: Some("主账号".to_string()),
        tags: Some("高频,团队A".to_string()),
        model_slugs: vec!["gpt-5.4".to_string()],
        quota_capacity_primary_window_tokens: Some(100_000),
        quota_capacity_secondary_window_tokens: Some(1_000_000),
        proxy_enabled: None,
        proxy_source: None,
        proxy_profile_id: None,
        proxy_profile_name: None,
        proxy_status: None,
        proxy_url: None,
        proxy_ip: None,
        proxy_country_code: None,
        proxy_country_name: None,
        proxy_region_name: None,
        proxy_city_name: None,
        proxy_geo_checked_at: None,
        proxy_asn: None,
        proxy_as_org: None,
        proxy_isp: None,
        proxy_as_domain: None,
        proxy_timezone_id: None,
        proxy_timezone_utc: None,
        proxy_flag_img_url: None,
        proxy_flag_emoji: None,
    };

    let value = serde_json::to_value(summary).expect("serialize account summary");
    let obj = value.as_object().expect("account summary object");

    for key in [
        "id",
        "label",
        "groupName",
        "preferred",
        "sort",
        "status",
        "statusReason",
        "hasToken",
        "note",
        "tags",
    ] {
        assert!(obj.contains_key(key), "missing key: {key}");
    }

    for key in ["workspaceId", "workspaceName", "updatedAt"] {
        assert!(!obj.contains_key(key), "unexpected key: {key}");
    }
}

/// 函数 `account_list_result_serialization_includes_pagination_fields`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn account_list_result_serialization_includes_pagination_fields() {
    let result = AccountListResult {
        items: vec![AccountSummary {
            id: "acc-1".to_string(),
            label: "主账号".to_string(),
            group_name: Some("TEAM".to_string()),
            preferred: true,
            sort: 10,
            status: "active".to_string(),
            status_reason: Some("account_deactivated".to_string()),
            has_token: true,
            plan_type: Some("team".to_string()),
            plan_type_raw: None,
            has_subscription: Some(true),
            subscription_plan: Some("team".to_string()),
            subscription_expires_at: Some(1_745_000_000),
            subscription_renews_at: Some(1_745_000_000),
            note: Some("主账号".to_string()),
            tags: Some("高频,团队A".to_string()),
            model_slugs: vec!["gpt-5.4".to_string()],
            quota_capacity_primary_window_tokens: Some(100_000),
            quota_capacity_secondary_window_tokens: Some(1_000_000),
            proxy_enabled: None,
            proxy_source: None,
            proxy_profile_id: None,
            proxy_profile_name: None,
            proxy_status: None,
            proxy_url: None,
            proxy_ip: None,
            proxy_country_code: None,
            proxy_country_name: None,
            proxy_region_name: None,
            proxy_city_name: None,
            proxy_geo_checked_at: None,
            proxy_asn: None,
            proxy_as_org: None,
            proxy_isp: None,
            proxy_as_domain: None,
            proxy_timezone_id: None,
            proxy_timezone_utc: None,
            proxy_flag_img_url: None,
            proxy_flag_emoji: None,
        }],
        total: 9,
        page: 2,
        page_size: 3,
    };

    let value = serde_json::to_value(result).expect("serialize account list result");
    let obj = value.as_object().expect("account list result object");
    for key in ["items", "total", "page", "pageSize"] {
        assert!(obj.contains_key(key), "missing key: {key}");
    }
}

/// 函数 `request_log_summary_serialization_includes_trace_route_fields`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn request_log_summary_serialization_includes_trace_route_fields() {
    let summary = RequestLogSummary {
        trace_id: Some("trc_1".to_string()),
        key_id: Some("gk_1".to_string()),
        account_id: Some("acc_1".to_string()),
        initial_account_id: Some("acc_free".to_string()),
        attempted_account_ids: vec!["acc_free".to_string(), "acc_1".to_string()],
        request_path: "/v1/responses".to_string(),
        original_path: Some("/v1/chat/completions".to_string()),
        adapted_path: Some("/v1/responses".to_string()),
        method: "POST".to_string(),
        route_strategy: Some("balanced".to_string()),
        route_source: Some("conversation_bound".to_string()),
        client_model: Some("gpt-5-client".to_string()),
        model: Some("gpt-5.3-codex".to_string()),
        model_source: Some("gateway_override".to_string()),
        upstream_model: Some("gpt-upstream".to_string()),
        actual_source_kind: Some("openai_account".to_string()),
        actual_source_id: Some("acc_1".to_string()),
        client_reasoning_effort: Some("low".to_string()),
        reasoning_effort: Some("high".to_string()),
        reasoning_source: Some("api_key_profile".to_string()),
        service_tier: Some("fast".to_string()),
        effective_service_tier: Some("fast".to_string()),
        service_tier_source: Some("client_request".to_string()),
        response_adapter: Some("OpenAIChatCompletionsJson".to_string()),
        canonical_source: Some("openai_compat".to_string()),
        size_reject_stage: Some("-".to_string()),
        upstream_url: Some("https://api.openai.com/v1".to_string()),
        aggregate_api_supplier_name: Some("方木木提供".to_string()),
        aggregate_api_url: Some("https://api.example.com/v1".to_string()),
        status_code: Some(502),
        duration_ms: Some(1450),
        input_tokens: Some(10),
        cached_input_tokens: Some(0),
        output_tokens: Some(3),
        total_tokens: Some(13),
        reasoning_output_tokens: Some(1),
        estimated_cost_usd: Some(0.12),
        error: Some("internal_error".to_string()),
        created_at: 1,
        ..Default::default()
    };

    let value = serde_json::to_value(summary).expect("serialize request log summary");
    let obj = value.as_object().expect("request log summary object");
    for key in [
        "traceId",
        "initialAccountId",
        "attemptedAccountIds",
        "originalPath",
        "adaptedPath",
        "responseAdapter",
        "canonicalSource",
        "sizeRejectStage",
        "serviceTier",
        "effectiveServiceTier",
        "serviceTierSource",
        "routeStrategy",
        "routeSource",
        "requestPath",
        "upstreamModel",
        "actualSourceKind",
        "actualSourceId",
        "upstreamUrl",
        "durationMs",
        "firstResponseMs",
    ] {
        assert!(obj.contains_key(key), "missing key: {key}");
    }
}

/// 函数 `request_log_list_params_default_to_first_page_with_twenty_items`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn request_log_list_params_default_to_first_page_with_twenty_items() {
    let params: RequestLogListParams =
        serde_json::from_value(serde_json::json!({})).expect("deserialize params");
    let normalized = params.normalized();

    assert_eq!(normalized.page, 1);
    assert_eq!(normalized.page_size, 20);
}

/// 函数 `request_log_list_result_serialization_includes_pagination_fields`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn request_log_list_result_serialization_includes_pagination_fields() {
    let result = RequestLogListResult {
        items: vec![RequestLogSummary {
            trace_id: Some("trc_1".to_string()),
            key_id: Some("gk_1".to_string()),
            account_id: Some("acc_1".to_string()),
            initial_account_id: Some("acc_free".to_string()),
            attempted_account_ids: vec!["acc_free".to_string(), "acc_1".to_string()],
            request_path: "/v1/responses".to_string(),
            original_path: Some("/v1/chat/completions".to_string()),
            adapted_path: Some("/v1/responses".to_string()),
            method: "POST".to_string(),
            route_strategy: Some("balanced".to_string()),
            route_source: Some("conversation_bound".to_string()),
            client_model: Some("gpt-5-client".to_string()),
            model: Some("gpt-5.3-codex".to_string()),
            model_source: Some("gateway_override".to_string()),
            client_reasoning_effort: Some("low".to_string()),
            reasoning_effort: Some("high".to_string()),
            reasoning_source: Some("api_key_profile".to_string()),
            service_tier: Some("fast".to_string()),
            effective_service_tier: Some("fast".to_string()),
            service_tier_source: Some("client_request".to_string()),
            response_adapter: Some("OpenAIChatCompletionsJson".to_string()),
            canonical_source: Some("openai_compat".to_string()),
            size_reject_stage: Some("-".to_string()),
            upstream_url: Some("https://api.openai.com/v1".to_string()),
            aggregate_api_supplier_name: Some("方木木提供".to_string()),
            aggregate_api_url: Some("https://api.example.com/v1".to_string()),
            status_code: Some(200),
            duration_ms: Some(1200),
            input_tokens: Some(10),
            cached_input_tokens: Some(1),
            output_tokens: Some(2),
            total_tokens: Some(12),
            reasoning_output_tokens: Some(1),
            estimated_cost_usd: Some(0.12),
            error: None,
            created_at: 1,
            ..Default::default()
        }],
        total: 88,
        page: 3,
        page_size: 25,
    };

    let value = serde_json::to_value(result).expect("serialize request log list result");
    let obj = value.as_object().expect("request log list result object");
    for key in ["items", "total", "page", "pageSize"] {
        assert!(obj.contains_key(key), "missing key: {key}");
    }
}

/// 函数 `request_log_filter_summary_serialization_uses_camel_case`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn request_log_filter_summary_serialization_uses_camel_case() {
    let result = RequestLogFilterSummaryResult {
        total_count: 120,
        filtered_count: 33,
        success_count: 30,
        error_count: 3,
        total_tokens: 123456,
        total_cost_usd: 12.34,
    };

    let value = serde_json::to_value(result).expect("serialize request log filter summary");
    let obj = value
        .as_object()
        .expect("request log filter summary object");
    for key in [
        "totalCount",
        "filteredCount",
        "successCount",
        "errorCount",
        "totalTokens",
        "totalCostUsd",
    ] {
        assert!(obj.contains_key(key), "missing key: {key}");
    }
}

/// 函数 `api_key_usage_stat_summary_serialization_uses_camel_case`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn api_key_usage_stat_summary_serialization_uses_camel_case() {
    let result = ApiKeyUsageStatSummary {
        key_id: "gk_test".to_string(),
        total_tokens: 123,
        estimated_cost_usd: 4.56,
    };

    let value = serde_json::to_value(result).expect("serialize api key usage stat summary");
    let obj = value
        .as_object()
        .expect("api key usage stat summary object");
    for key in ["keyId", "totalTokens", "estimatedCostUsd"] {
        assert!(obj.contains_key(key), "missing key: {key}");
    }
}

#[test]
fn dashboard_admin_usage_summary_serialization_uses_camel_case() {
    let usage = DashboardTokenUsageResult {
        input_tokens: 100,
        cached_input_tokens: 20,
        output_tokens: 50,
        reasoning_output_tokens: 5,
        total_tokens: 130,
        estimated_cost_usd: 0.42,
        request_count: 3,
        success_count: 2,
        error_count: 1,
    };
    let result = DashboardAdminUsageSummaryResult {
        range_start_ts: 1_700_000_000,
        range_end_ts: 1_700_086_400,
        today_start_ts: 1_700_000_000,
        today_end_ts: 1_700_086_400,
        today_usage: usage.clone(),
        daily_usage: vec![DashboardDailyUsagePoint {
            day_start_ts: 1_700_000_000,
            day_end_ts: 1_700_086_400,
            usage: usage.clone(),
        }],
        users: vec![DashboardUserUsageSummary {
            user_id: "usr-1".to_string(),
            username: Some("member-one".to_string()),
            display_name: None,
            role: Some("member".to_string()),
            status: Some("active".to_string()),
            wallet_available_credit_micros: Some(1_000_000),
            today_usage: usage.clone(),
            range_usage: usage.clone(),
        }],
        openai_accounts: vec![DashboardSourceUsageSummary {
            source_kind: "openai_account".to_string(),
            source_id: "acc-1".to_string(),
            name: Some("main".to_string()),
            status: Some("active".to_string()),
            provider: Some("openai".to_string()),
            today_usage: usage.clone(),
            range_usage: usage.clone(),
        }],
        aggregate_apis: vec![DashboardSourceUsageSummary {
            source_kind: "aggregate_api".to_string(),
            source_id: "agg-1".to_string(),
            name: Some("supplier".to_string()),
            status: Some("active".to_string()),
            provider: Some("openai-compatible".to_string()),
            today_usage: usage.clone(),
            range_usage: usage,
        }],
    };

    let value = serde_json::to_value(result).expect("serialize dashboard admin usage");
    let obj = value.as_object().expect("dashboard admin usage object");
    for key in [
        "rangeStartTs",
        "rangeEndTs",
        "todayStartTs",
        "todayEndTs",
        "todayUsage",
        "dailyUsage",
        "openaiAccounts",
        "aggregateApis",
    ] {
        assert!(obj.contains_key(key), "missing key: {key}");
    }
    assert!(obj["todayUsage"].get("inputTokens").is_some());
    assert!(obj["users"][0].get("walletAvailableCreditMicros").is_some());
    assert!(obj["openaiAccounts"][0].get("sourceKind").is_some());
}

#[test]
fn proxy_profile_list_result_serialization_uses_camel_case() {
    let result = ProxyProfileListResult {
        items: vec![ProxyProfileEntry {
            id: "pp_1".to_string(),
            name: "Proxy One".to_string(),
            proxy_url_redacted: "http://example.com:8080".to_string(),
            scheme: Some("http".to_string()),
            host: Some("example.com".to_string()),
            port: Some(8080),
            enabled: true,
            status: "unchecked".to_string(),
            last_error: None,
            last_url_latency_ms: Some(42),
            last_download_mbps: Some(10.5),
            last_upload_mbps: Some(5.25),
            last_tested_at: Some(1_700_000_000),
            ip: Some("203.0.113.10".to_string()),
            country_code: Some("US".to_string()),
            country_name: Some("United States".to_string()),
            region_name: Some("California".to_string()),
            city_name: Some("San Francisco".to_string()),
            asn: Some(64512),
            as_org: Some("Example ASN".to_string()),
            isp: Some("Example ISP".to_string()),
            as_domain: Some("example.com".to_string()),
            flag_img_url: None,
            flag_emoji: None,
            timezone_id: None,
            timezone_offset: None,
            timezone_utc: None,
            tags_json: Some(r#"["work"]"#.to_string()),
            notes: Some("Primary proxy".to_string()),
            accounts_count: Some(2),
            created_at: 1,
            updated_at: 2,
        }],
    };

    let value = serde_json::to_value(result).expect("serialize proxy profile list result");
    let obj = value.as_object().expect("proxy profile list result object");
    assert!(obj.contains_key("items"));
    let first = obj["items"][0]
        .as_object()
        .expect("proxy profile entry object");
    for key in [
        "proxyUrlRedacted",
        "lastError",
        "lastUrlLatencyMs",
        "lastDownloadMbps",
        "lastUploadMbps",
        "lastTestedAt",
        "countryCode",
        "countryName",
        "regionName",
        "cityName",
        "asOrg",
        "tagsJson",
        "createdAt",
        "updatedAt",
    ] {
        assert!(first.contains_key(key), "missing key: {key}");
    }
}

#[test]
fn proxy_test_presets_result_serialization_uses_camel_case() {
    let result = ProxyTestPresetsResult {
        speed_providers: vec![ProxyTestSpeedProviderPreset {
            id: "cachefly".to_string(),
            label: "CacheFly".to_string(),
            provider_family: "cachefly".to_string(),
            files: vec![ProxyTestProviderFilePreset {
                file_size_id: "size_10mb".to_string(),
                download_url: "http://cachefly.cachefly.net/10mb.test".to_string(),
                read_limit_bytes: None,
            }],
        }],
        file_sizes: vec![ProxyTestFileSizePreset {
            id: "size_10mb".to_string(),
            label: "10MB".to_string(),
            bytes: 10_000_000,
            warning: false,
        }],
        defaults: ProxyTestDefaults {
            speed_provider_id: "cachefly".to_string(),
            file_size_id: "size_10mb".to_string(),
            latency_preset_id: "google_gstatic_204".to_string(),
        },
        upload_endpoint: ProxyTestUploadEndpointStatus {
            status: "configured".to_string(),
            configured: true,
            source: "env".to_string(),
            url: Some("https://upload.example.com/proxy-test".to_string()),
        },
    };

    let value = serde_json::to_value(result).expect("serialize proxy test presets result");
    let obj = value.as_object().expect("proxy test presets result object");
    for key in [
        "speedProviders",
        "fileSizes",
        "defaults",
        "uploadEndpoint",
    ] {
        assert!(obj.contains_key(key), "missing key: {key}");
    }
    assert!(obj["uploadEndpoint"].get("configured").is_some());
    assert!(obj["speedProviders"][0]["files"][0]
        .get("fileSizeId")
        .is_some());
}

#[test]
fn proxy_profile_url_test_result_serialization_uses_camel_case() {
    let result = ProxyProfileUrlTestEntry {
        id: 7,
        proxy_profile_id: "pp_1".to_string(),
        status: "failed".to_string(),
        url_latency_ms: Some(123),
        status_code: Some(302),
        test_url: "http://example.test/generate_204".to_string(),
        final_url: Some("http://example.test/login".to_string()),
        redirected: true,
        tested_at: 1_700_000_123,
        error_code: Some("redirect_detected".to_string()),
        error: Some("Received redirect".to_string()),
    };

    let value = serde_json::to_value(result).expect("serialize proxy url test result");
    let obj = value.as_object().expect("proxy url test result object");
    for key in [
        "proxyProfileId",
        "urlLatencyMs",
        "statusCode",
        "testUrl",
        "finalUrl",
        "redirected",
        "testedAt",
        "errorCode",
    ] {
        assert!(obj.contains_key(key), "missing key: {key}");
    }
}
