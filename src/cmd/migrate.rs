use zeroclaw::{migration, Config, MigrateCommands};

#[derive(clap::Args, Debug)]
pub(crate) struct MigrateArgs {
    #[command(subcommand)]
    migrate_command: MigrateCommands,
}

pub(crate) async fn run(args: MigrateArgs, config: &Config) -> anyhow::Result<()> {
    migration::handle_command(args.migrate_command, config).await
}
