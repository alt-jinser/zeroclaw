use zeroclaw::{peripherals, Config, PeripheralCommands};

#[derive(clap::Args, Debug)]
pub(crate) struct PeripheralArgs {
    #[command(subcommand)]
    peripheral_command: PeripheralCommands,
}

pub(crate) async fn run(args: PeripheralArgs, config: &Config) -> anyhow::Result<()> {
    peripherals::handle_command(args.peripheral_command, config).await
}
