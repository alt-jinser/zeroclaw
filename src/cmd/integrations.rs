use zeroclaw::{integrations, Config, IntegrationCommands};

#[derive(clap::Args, Debug)]
pub(crate) struct IntegrationsArgs {
    #[command(subcommand)]
    integration_command: IntegrationCommands,
}

pub(crate) async fn run(args: IntegrationsArgs, config: Config) -> anyhow::Result<()> {
    integrations::handle_command(args.integration_command, &config)
}
