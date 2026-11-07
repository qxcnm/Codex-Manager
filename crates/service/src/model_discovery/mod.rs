//! Opt-in, per-source model discovery for one service process.
mod scheduler;
mod store;
mod types;

pub(crate) use scheduler::ensure_model_discovery;
pub(crate) use types::{ModelDiscoveryList, ModelDiscoveryTask};

pub(crate) fn list_tasks() -> Result<ModelDiscoveryList, String> {
    let control = scheduler::control();
    Ok(ModelDiscoveryList {
        items: store::load()?
            .into_iter()
            .map(|record| {
                let mut task = record.task();
                if let Some(retry_at) = control.cooldowns.get(&record.config.source.key()) {
                    task.running = false;
                    task.next_run_at = task.enabled.then_some(*retry_at);
                    task.last_error =
                        Some("model discovery completion could not be saved; retry delayed".into());
                }
                task
            })
            .collect(),
    })
}

pub(crate) fn update_task(
    source_kind: &str,
    source_id: &str,
    enabled: bool,
    interval_minutes: Option<u64>,
) -> Result<ModelDiscoveryTask, String> {
    let source = types::Source::parse(source_kind, source_id)?;
    if let Some(interval) = interval_minutes {
        types::validate_interval(interval)?;
    }
    let mut control = scheduler::control();
    if enabled {
        scheduler::check_source(&source)?;
    }
    let key = source.key();
    let mut record = store::load()?
        .into_iter()
        .find(|record| record.config.source == source)
        .unwrap_or_else(|| types::Record::new(source));
    let changed = record.update(
        enabled,
        interval_minutes,
        codexmanager_core::storage::now_ts(),
    )?;
    store::save(&record, true)?;
    if changed {
        control.cooldowns.remove(&key);
        if let Some(active) = control.active.get_mut(&key) {
            active
                .cancelled
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }
    let task = record.task();
    drop(control);
    scheduler::wake();
    Ok(task)
}

#[cfg(test)]
mod tests;
