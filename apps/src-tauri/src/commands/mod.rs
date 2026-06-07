#![allow(dead_code)]

pub mod account;
pub mod account_manager;
pub mod aggregate_api;
pub mod apikey;
pub mod codex_profile;
pub mod dashboard;
pub mod login;
pub mod plugin;
pub mod quota;
mod registry;
pub mod requestlog;
pub mod service;
pub mod settings;
pub mod shared;
pub mod startup;
pub mod system;
pub mod updater;
pub mod usage;

pub(crate) use registry::invoke_handler;
