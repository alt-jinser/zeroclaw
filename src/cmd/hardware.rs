use zeroclaw::{hardware, Config};

#[derive(clap::Args, Debug)]
pub(crate) struct HardwareArgs {
    #[command(subcommand)]
    hardware_command: zeroclaw::HardwareCommands,
}

pub(crate) fn run(args: HardwareArgs, config: &Config) -> anyhow::Result<()> {
    hardware::handle_command(args.hardware_command, config)
}
