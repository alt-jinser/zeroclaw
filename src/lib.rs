#![forbid(unsafe_code)]
#![recursion_limit = "256"]

pub mod agent;
pub mod approval;
pub mod auth;
pub mod channels;
pub mod config;
pub mod coordination;
pub mod cost;
pub mod cron;
pub mod daemon;
pub mod doctor;
pub mod economic;
pub mod gateway;
pub mod goals;
pub mod hardware;
pub mod health;
pub mod heartbeat;
pub mod hooks;
pub mod identity;
pub mod integrations;
pub mod memory;
pub mod migration;
pub mod multimodal;
pub mod observability;
pub mod onboard;
pub mod peripherals;
pub mod plugins;
pub mod providers;
pub mod rag;
pub mod runtime;
pub mod security;
pub mod service;
pub mod skillforge;
pub mod skills;
#[cfg(test)]
pub mod test_locks;
pub mod tools;
pub mod tunnel;
pub mod update;
pub mod util;

pub use config::Config;

pub const ZEROCLAW_BUILD_VERSION: &str = env!("ZEROCLAW_BUILD_VERSION");
