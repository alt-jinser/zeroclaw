use clap::Subcommand;
use serde::{Deserialize, Serialize};
use zeroclaw::{service, Config};

#[derive(clap::Args, Debug)]
pub(crate) struct ServiceArgs {
    /// Init system to use: auto (detect), systemd, or openrc
    #[arg(long, default_value = "auto", value_parser = ["auto", "systemd", "openrc"])]
    service_init: String,

    #[command(subcommand)]
    service_command: ServiceCommands,
}

/// Service management subcommands
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum ServiceCommands {
    /// Install daemon service unit for auto-start and restart
    Install,
    /// Start daemon service
    Start,
    /// Stop daemon service
    Stop,
    /// Restart daemon service to apply latest config
    Restart,
    /// Check daemon service status
    Status,
    /// Uninstall daemon service unit
    Uninstall,
}

pub(crate) async fn run(args: ServiceArgs, config: &Config) -> anyhow::Result<()> {
    let ServiceArgs {
        service_init,
        service_command,
    } = args;
    let init_system = service_init.parse()?;
    match service_command {
        ServiceCommands::Install => service::install(config, init_system),
        ServiceCommands::Start => service::start(config, init_system),
        ServiceCommands::Stop => service::stop(config, init_system),
        ServiceCommands::Restart => service::restart(config, init_system),
        ServiceCommands::Status => service::status(config, init_system),
        ServiceCommands::Uninstall => service::uninstall(config, init_system),
    }
}
