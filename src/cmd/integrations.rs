use clap::Subcommand;
use serde::{Deserialize, Serialize};
use zeroclaw::{
    integrations::{list_integrations, search_integrations, show_integration_info},
    Config,
};

#[derive(clap::Args, Debug)]
pub(crate) struct IntegrationsArgs {
    #[command(subcommand)]
    integration_command: IntegrationCommands,
}

/// Integration subcommands
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum IntegrationCommands {
    /// List all integrations (optionally filter by category or status)
    List {
        /// Filter by category (e.g. "chat", "ai", "productivity")
        #[arg(long, short)]
        category: Option<String>,
        /// Filter by status: active, available, coming-soon
        #[arg(long, short)]
        status: Option<String>,
    },
    /// Search integrations by keyword (matches name and description)
    Search {
        /// Search query
        query: String,
    },
    /// Show details about a specific integration
    Info {
        /// Integration name
        name: String,
    },
}

pub(crate) async fn run(args: IntegrationsArgs, config: &Config) -> anyhow::Result<()> {
    match args.integration_command {
        IntegrationCommands::List { category, status } => {
            list_integrations(config, category.as_deref(), status.as_deref())
        }
        IntegrationCommands::Search { query } => search_integrations(config, &query),
        IntegrationCommands::Info { name } => show_integration_info(config, &name),
    }
}
