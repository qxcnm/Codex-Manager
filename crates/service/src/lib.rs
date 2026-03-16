use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};

mod account;
mod account_identity;
mod apikey;
pub(crate) mod app_settings;
mod auth;
mod errors;
mod gateway;
mod http;
mod lifecycle;
mod requestlog;
mod rpc_dispatch;
mod runtime;
mod startup_snapshot;
mod storage;
mod usage;

pub(crate) use account::availability as account_availability;
pub(crate) use account::cleanup as account_cleanup;
pub(crate) use account::delete as account_delete;
pub(crate) use account::delete_many as account_delete_many;
pub(crate) use account::export as account_export;
pub(crate) use account::import as account_import;
pub(crate) use account::list as account_list;
pub(crate) use account::plan as account_plan;
pub(crate) use account::status as account_status;
pub(crate) use account::update as account_update;
pub(crate) use apikey::create as apikey_create;
pub(crate) use apikey::delete as apikey_delete;
pub(crate) use apikey::disable as apikey_disable;
pub(crate) use apikey::enable as apikey_enable;
pub(crate) use apikey::list as apikey_list;
pub(crate) use apikey::models as apikey_models;
pub(crate) use apikey::profile as apikey_profile;
pub(crate) use apikey::read_secret as apikey_read_secret;
pub(crate) use apikey::update_model as apikey_update_model;
pub(crate) use apikey::usage_stats as apikey_usage_stats;
pub(crate) use auth::account as auth_account;
pub(crate) use auth::callback as auth_callback;
pub(crate) use auth::login as auth_login;
pub(crate) use auth::tokens as auth_tokens;
pub(crate) use errors as error_codes;
pub(crate) use requestlog::clear as requestlog_clear;
pub(crate) use requestlog::list as requestlog_list;
pub(crate) use requestlog::summary as requestlog_summary;
pub(crate) use requestlog::today_summary as requestlog_today_summary;
pub(crate) use runtime::lock_utils;
pub use runtime::process_env;
pub(crate) use runtime::reasoning_effort;
pub(crate) use storage::helpers as storage_helpers;
pub(crate) use usage::account_meta as usage_account_meta;
pub(crate) use usage::aggregate as usage_aggregate;
pub(crate) use usage::http as usage_http;
pub(crate) use usage::keepalive as usage_keepalive;
pub(crate) use usage::list as usage_list;
pub(crate) use usage::read as usage_read;
pub(crate) use usage::refresh as usage_refresh;
pub(crate) use usage::scheduler as usage_scheduler;
pub(crate) use usage::snapshot_store as usage_snapshot_store;
pub(crate) use usage::token_refresh as usage_token_refresh;

pub use app_settings::{
    app_settings_get, app_settings_get_with_overrides, app_settings_set,
    bind_all_interfaces_enabled, current_close_to_tray_on_close_setting,
    current_gateway_free_account_max_model, current_gateway_sse_keepalive_interval_ms,
    current_gateway_upstream_stream_timeout_ms, current_lightweight_mode_on_close_to_tray_setting,
    current_saved_service_addr, current_service_bind_mode, current_ui_low_transparency_enabled,
    current_ui_theme, current_update_auto_check_enabled, default_listener_bind_addr,
    listener_bind_addr, set_close_to_tray_on_close_setting, set_gateway_background_tasks,
    set_gateway_cpa_no_cookie_header_mode, set_gateway_free_account_max_model,
    set_gateway_route_strategy, set_gateway_sse_keepalive_interval_ms,
    set_gateway_upstream_proxy_url, set_gateway_upstream_stream_timeout_ms,
    set_lightweight_mode_on_close_to_tray_setting, set_saved_service_addr, set_service_bind_mode,
    set_ui_low_transparency_enabled, set_ui_theme, set_update_auto_check_enabled,
    sync_runtime_settings_from_storage, BackgroundTasksInput,
    APP_SETTING_CLOSE_TO_TRAY_ON_CLOSE_KEY, APP_SETTING_ENV_OVERRIDES_KEY,
    APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY, APP_SETTING_GATEWAY_CPA_NO_COOKIE_HEADER_MODE_KEY,
    APP_SETTING_GATEWAY_FREE_ACCOUNT_MAX_MODEL_KEY, APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY,
    APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY, APP_SETTING_GATEWAY_UPSTREAM_PROXY_URL_KEY,
    APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY,
    APP_SETTING_LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_KEY, APP_SETTING_SERVICE_ADDR_KEY,
    APP_SETTING_UI_LOW_TRANSPARENCY_KEY, APP_SETTING_UI_THEME_KEY,
    APP_SETTING_UPDATE_AUTO_CHECK_KEY, APP_SETTING_WEB_ACCESS_PASSWORD_HASH_KEY, DEFAULT_ADDR,
    DEFAULT_BIND_ADDR, SERVICE_BIND_MODE_ALL_INTERFACES, SERVICE_BIND_MODE_LOOPBACK,
    SERVICE_BIND_MODE_SETTING_KEY, WEB_ACCESS_SESSION_COOKIE_NAME,
};
pub use auth::{
    build_web_access_session_token, current_web_access_password_hash, set_web_access_password,
    verify_web_access_password, web_access_password_configured, web_auth_status_value,
};
pub use auth::{rpc_auth_token, rpc_auth_token_matches};
pub use lifecycle::bootstrap::{initialize_storage_if_needed, portable};
pub use lifecycle::shutdown::{clear_shutdown_flag, request_shutdown, shutdown_requested};
pub use lifecycle::startup::{start_one_shot_server, start_server, ServerHandle};

pub(crate) fn handle_request(req: JsonRpcRequest) -> JsonRpcResponse {
    rpc_dispatch::handle_request(req)
}

#[cfg(test)]
#[path = "tests/lib_tests.rs"]
mod tests;
