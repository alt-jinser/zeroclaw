use zeroclaw::{cron, Config, CronCommands};

#[derive(clap::Args, Debug)]
pub(crate) struct CronArgs {
    #[command(subcommand)]
    cron_command: CronCommands,
}

pub(crate) fn run(args: CronArgs, config: &Config) -> anyhow::Result<()> {
    cron::handle_command(args.cron_command, config)
}
