use clap::Subcommand;
use serde::{Deserialize, Serialize};
use zeroclaw::{
    migration::{migrate_openclaw, print_report, OpenClawMigrationOptions},
    Config,
};

#[derive(clap::Args, Debug)]
pub(crate) struct MigrateArgs {
    #[command(subcommand)]
    migrate_command: MigrateCommands,
}

/// Migration subcommands
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MigrateCommands {
    /// Import OpenClaw data into this ZeroClaw workspace (memory, config, agents)
    Openclaw {
        /// Optional path to `OpenClaw` workspace (defaults to ~/.openclaw/workspace)
        #[arg(long)]
        source: Option<std::path::PathBuf>,

        /// Optional path to `OpenClaw` config file (defaults to ~/.openclaw/openclaw.json)
        #[arg(long)]
        source_config: Option<std::path::PathBuf>,

        /// Validate and preview migration without writing any data
        #[arg(long)]
        dry_run: bool,

        /// Skip memory migration
        #[arg(long)]
        no_memory: bool,

        /// Skip configuration and agents migration
        #[arg(long)]
        no_config: bool,
    },
}

pub(crate) async fn run(args: MigrateArgs, config: &Config) -> anyhow::Result<()> {
    match args.migrate_command {
        MigrateCommands::Openclaw {
            source,
            source_config,
            dry_run,
            no_memory,
            no_config,
        } => {
            let options = OpenClawMigrationOptions {
                source_workspace: source,
                source_config,
                include_memory: !no_memory,
                include_config: !no_config,
                dry_run,
            };
            let report = migrate_openclaw(config, options).await?;
            print_report(&report);
            Ok(())
        }
    }
}
