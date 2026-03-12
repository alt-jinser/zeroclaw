#![forbid(unsafe_code)]
#![recursion_limit = "256"]

use clap::Subcommand;
use serde::{Deserialize, Serialize};

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

/// Memory management subcommands
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemoryCommands {
    /// List memory entries with optional filters
    List {
        /// Filter by category (core, daily, conversation, or custom name)
        #[arg(long)]
        category: Option<String>,
        /// Filter by session ID
        #[arg(long)]
        session: Option<String>,
        /// Maximum number of entries to display
        #[arg(long, default_value = "50")]
        limit: usize,
        /// Number of entries to skip (for pagination)
        #[arg(long, default_value = "0")]
        offset: usize,
    },
    /// Get a specific memory entry by key
    Get {
        /// Memory key to look up
        key: String,
    },
    /// Show memory backend statistics and health
    Stats,
    /// Clear memories by category, by key, or clear all
    Clear {
        /// Delete a single entry by key (supports prefix match)
        #[arg(long)]
        key: Option<String>,
        /// Only clear entries in this category
        #[arg(long)]
        category: Option<String>,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Rebuild embeddings for all memories (use after changing embedding model)
    Reindex {
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
        /// Show progress during reindex
        #[arg(long, default_value = "true")]
        progress: bool,
    },
}
