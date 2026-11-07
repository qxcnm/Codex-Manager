use crate::storage_helpers::{open_storage, seaorm_block_on, seaorm_enabled};
use codexmanager_core::storage::{ServiceHealthLogSample, ServiceHealthRecord};
use codexmanager_storage_seaorm::ServiceHealthRepository as Remote;

pub fn get(id: &str) -> Result<Option<ServiceHealthRecord>, String> {
    if seaorm_enabled() {
        let id = id.to_owned();
        return seaorm_block_on(move |s| async move {
            Remote::get(s.connection(), &id)
                .await
                .map_err(|_| "health_storage_unavailable".into())
        });
    }
    open_storage()
        .ok_or("health_storage_unavailable")?
        .service_health_get(id)
        .map_err(|_| "health_storage_unavailable".into())
}
pub fn list(kind: &str, now: i64, limit: usize) -> Result<Vec<ServiceHealthRecord>, String> {
    if seaorm_enabled() {
        let kind = kind.to_owned();
        return seaorm_block_on(move |s| async move {
            Remote::list(s.connection(), &kind, now, limit)
                .await
                .map_err(|_| "health_storage_unavailable".into())
        });
    }
    open_storage()
        .ok_or("health_storage_unavailable")?
        .service_health_list(kind, now, limit)
        .map_err(|_| "health_storage_unavailable".into())
}
pub fn put(row: ServiceHealthRecord, expected: Option<i64>) -> Result<bool, String> {
    if seaorm_enabled() {
        return seaorm_block_on(move |s| async move {
            Remote::put(s.connection(), row, expected)
                .await
                .map_err(|_| "health_storage_unavailable".into())
        });
    }
    open_storage()
        .ok_or("health_storage_unavailable")?
        .service_health_put(&row, expected)
        .map_err(|_| "health_storage_unavailable".into())
}
pub fn prune(now: i64) -> Result<(), String> {
    if seaorm_enabled() {
        return seaorm_block_on(move |s| async move {
            Remote::prune(s.connection(), now)
                .await
                .map_err(|_| "health_storage_unavailable".into())
        });
    }
    open_storage()
        .ok_or("health_storage_unavailable")?
        .service_health_prune(now)
        .map_err(|_| "health_storage_unavailable".into())
}
pub fn samples(now: i64) -> Result<Vec<ServiceHealthLogSample>, String> {
    if seaorm_enabled() {
        return seaorm_block_on(move |s| async move {
            Remote::samples(
                s.connection(),
                now - super::types::WINDOW_SECS,
                now + 1,
                super::types::SAMPLE_LIMIT + 1,
            )
            .await
            .map_err(|_| "health_storage_unavailable".into())
        });
    }
    open_storage()
        .ok_or("health_storage_unavailable")?
        .service_health_samples(
            now - super::types::WINDOW_SECS,
            now + 1,
            super::types::SAMPLE_LIMIT + 1,
        )
        .map_err(|_| "health_storage_unavailable".into())
}
