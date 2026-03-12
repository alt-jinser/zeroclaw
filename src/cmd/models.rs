use anyhow::bail;
use clap::Subcommand;
use zeroclaw::{onboard, Config};

#[derive(Subcommand, Debug)]
pub(crate) enum ModelCommands {
    /// Refresh and cache provider models
    Refresh {
        /// Provider name (defaults to configured default provider)
        #[arg(long)]
        provider: Option<String>,

        /// Refresh all providers that support live model discovery
        #[arg(long)]
        all: bool,

        /// Force live refresh and ignore fresh cache
        #[arg(long)]
        force: bool,
    },
    /// List cached models for a provider
    List {
        /// Provider name (defaults to configured default provider)
        #[arg(long)]
        provider: Option<String>,
    },
    /// Set the default model in config
    Set {
        /// Model name to set as default
        model: String,
    },
    /// Show current model configuration and cache status
    Status,
}

#[derive(clap::Args, Debug)]
pub(crate) struct ModelsArgs {
    #[command(subcommand)]
    model_command: ModelCommands,
}

pub(crate) async fn run(args: ModelsArgs, config: &Config) -> anyhow::Result<()> {
    match args.model_command {
        ModelCommands::Refresh {
            provider,
            all,
            force,
        } => {
            if all {
                if provider.is_some() {
                    bail!("`models refresh --all` cannot be combined with --provider");
                }
                onboard::run_models_refresh_all(config, force).await
            } else {
                onboard::run_models_refresh(config, provider.as_deref(), force).await
            }
        }
        ModelCommands::List { provider } => {
            onboard::run_models_list(config, provider.as_deref()).await
        }
        ModelCommands::Set { model } => onboard::run_models_set(config, &model).await,
        ModelCommands::Status => onboard::run_models_status(config).await,
    }
}
