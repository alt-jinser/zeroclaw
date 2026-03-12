use zeroclaw::{service, Config, ServiceCommands};

#[derive(clap::Args, Debug)]
pub(crate) struct ServiceArgs {
    /// Init system to use: auto (detect), systemd, or openrc
    #[arg(long, default_value = "auto", value_parser = ["auto", "systemd", "openrc"])]
    service_init: String,

    #[command(subcommand)]
    service_command: ServiceCommands,
}

pub(crate) async fn run(args: ServiceArgs, config: Config) -> anyhow::Result<()> {
    let ServiceArgs {
        service_init,
        service_command,
    } = args;
    let init_system = service_init.parse()?;
    service::handle_command(&service_command, &config, init_system)
}
