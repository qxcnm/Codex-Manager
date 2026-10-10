#[path = "refresh/mod.rs"]
mod refresh;

#[cfg(test)]
pub(crate) use refresh::subscribe_usage_refresh_completed;
pub(crate) use refresh::{
    background_tasks_settings, drain_usage_background_tasks,
    enqueue_usage_refresh_after_account_add, enqueue_usage_refresh_for_account,
    ensure_gateway_keepalive, ensure_reset_warmup, ensure_token_refresh_polling,
    ensure_usage_polling, ensure_warmup_cron, refresh_usage_for_account,
    refresh_usage_for_account_async, refresh_usage_for_account_result,
    refresh_usage_for_account_result_async, refresh_usage_for_all_accounts_result,
    refresh_usage_for_all_accounts_result_async, reload_background_tasks_runtime_from_env,
    set_background_tasks_settings, subscribe_usage_refresh_completed_async,
    validate_background_tasks_settings_patch, BackgroundTasksSettingsPatch,
};
pub use refresh::{set_usage_refresh_completed_handler, UsageRefreshCompletedEvent};

#[cfg(test)]
pub(crate) use refresh::{notify_usage_refresh_completed, usage_refresh_async_subscriber_count};
