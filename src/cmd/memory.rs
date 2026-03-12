use clap::Subcommand;
use serde::{Deserialize, Serialize};
use zeroclaw::{
    memory::cli::{handle_clear, handle_get, handle_list, handle_reindex, handle_stats},
    Config,
};

#[derive(clap::Args, Debug)]
pub(crate) struct MemoryArgs {
    #[command(subcommand)]
    memory_command: MemoryCommands,
}

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

pub(crate) async fn run(args: MemoryArgs, config: &Config) -> anyhow::Result<()> {
    match args.memory_command {
        MemoryCommands::List {
            category,
            session,
            limit,
            offset,
        } => handle_list(config, category, session, limit, offset).await,
        MemoryCommands::Get { key } => handle_get(config, &key).await,
        MemoryCommands::Stats => handle_stats(config).await,
        MemoryCommands::Clear { key, category, yes } => {
            handle_clear(config, key, category, yes).await
        }
        MemoryCommands::Reindex { yes, progress } => handle_reindex(config, yes, progress).await,
    }
}
