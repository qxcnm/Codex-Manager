#![allow(dead_code)]

pub mod account;
pub mod account_manager;
pub mod aggregate_api;
pub mod apikey;
pub mod codex_profile;
pub mod codex_projects;
pub mod codex_skills;
pub mod dashboard;
pub mod diagnostics;
pub mod login;
pub mod model_discovery;
pub mod plugin;
pub mod quota;
mod registry;
pub mod requestlog;
pub mod service;
pub mod service_health;
pub mod settings;
pub mod shared;
pub mod startup;
pub mod storage_space;
pub mod system;
pub mod updater;
pub mod usage;

pub(crate) use registry::invoke_handler;
