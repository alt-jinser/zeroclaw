use anyhow::Context as _;
use clap::{Parser, Subcommand};
use zeroclaw::{observability, security, Config, ZEROCLAW_BUILD_VERSION};

use crate::cmd::{
    agent::AgentArgs, auth::AuthArgs, channel::ChannelArgs, completions::CompletionsArgs,
    config::ConfigArgs, cron::CronArgs, daemon::DaemonArgs, doctor::DoctorArgs, estop::EstopArgs,
    gateway::GatewayArgs, hardware::HardwareArgs, integrations::IntegrationsArgs,
    memory::MemoryArgs, migrate::MigrateArgs, models::ModelsArgs, onboard::OnboardArgs,
    peripheral::PeripheralArgs, providers_quota::ProvidersQuotaArgs, service::ServiceArgs,
    skills::SkillsArgs, update::UpdateArgs,
};

mod agent;
mod auth;
mod channel;
mod completions;
mod config;
mod cron;
mod daemon;
mod doctor;
mod estop;
mod gateway;
mod hardware;
mod integrations;
mod memory;
mod migrate;
mod models;
mod onboard;
mod peripheral;
mod providers;
mod providers_quota;
mod service;
mod skills;
mod status;
mod update;

pub(crate) async fn run(command: Command) -> anyhow::Result<()> {
    match command {
        Command::Onboard(args) => onboard::run(args).await,
        Command::Completions(args) => completions::run(args),
        cmd_with_config => {
            let mut config = Config::load_or_init().await?;
            config.apply_env_overrides();
            observability::runtime_trace::init_from_config(
                &config.observability,
                &config.workspace_dir,
            );
            if config.security.otp.enabled {
                let config_dir = config
                    .config_path
                    .parent()
                    .context("Config path must have a parent directory")?;
                let store = security::SecretStore::new(config_dir, config.secrets.encrypt);
                let (_validator, enrollment_uri) =
                    security::OtpValidator::from_config(&config.security.otp, config_dir, &store)?;
                if let Some(uri) = enrollment_uri {
                    println!("Initialized OTP secret for ZeroClaw.");
                    println!("Enrollment URI: {uri}");
                }
            }
            match cmd_with_config {
                Command::Onboard(_) | Command::Completions(_) => unreachable!(),
                Command::Agent(args) => agent::run(args, config).await,
                Command::Gateway(args) => gateway::run(args, config).await,
                Command::Daemon(args) => daemon::run(args, config).await,
                Command::Service(args) => service::run(args, config).await,
                Command::Doctor(args) => doctor::run(args, config).await,
                Command::Status => status::run(config),
                Command::Update(args) => update::run(args).await,
                Command::Estop(args) => estop::run(args, &config).await,
                Command::Cron(args) => cron::run(args, &config),
                Command::Models(args) => models::run(args, &config).await,
                Command::Providers => providers::run(&config),
                Command::ProvidersQuota(args) => providers_quota::run(args, config).await,
                Command::Channel(args) => channel::run(args, config).await,
                Command::Integrations(args) => integrations::run(args, config).await,
                Command::Skills(args) => skills::run(args, &config),
                Command::Migrate(args) => migrate::run(args, &config).await,
                Command::Auth(args) => auth::run(args, &config).await,
                Command::Hardware(args) => hardware::run(args, &config),
                Command::Peripheral(args) => peripheral::run(args, &config).await,
                Command::Memory(args) => memory::run(args, &config).await,
                Command::Config(args) => config::run(args, config).await,
            }
        }
    }
}

/// `ZeroClaw` - Zero overhead. Zero compromise. 100% Rust.
#[derive(Parser, Debug)]
#[command(name = "zeroclaw")]
#[command(author = "theonlyhennygod")]
#[command(version = ZEROCLAW_BUILD_VERSION)]
#[command(about = "The fastest, smallest AI assistant.", long_about = None)]
pub(crate) struct Cli {
    #[arg(long, global = true)]
    pub(crate) config_dir: Option<String>,

    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// Initialize your workspace and configuration
    Onboard(OnboardArgs),

    /// Start the AI agent loop
    #[command(long_about = "\
Start the AI agent loop.

Launches an interactive chat session with the configured AI provider. \
Use --message for single-shot queries without entering interactive mode.

Examples:
  zeroclaw agent                              # interactive session
  zeroclaw agent -m \"Summarize today's logs\"  # single message
  zeroclaw agent -p anthropic --model claude-sonnet-4-20250514
  zeroclaw agent --peripheral nucleo-f401re:/dev/ttyACM0
  zeroclaw agent --autonomy-level full --max-actions-per-hour 100
  zeroclaw agent -m \"quick task\" --memory-backend none --compact-context")]
    Agent(AgentArgs),

    /// Start the gateway server (webhooks, websockets)
    #[command(long_about = "\
Start the gateway server (webhooks, websockets).

Runs the HTTP/WebSocket gateway that accepts incoming webhook events \
and WebSocket connections. Bind address defaults to the values in \
your config file (gateway.host / gateway.port).

Examples:
  zeroclaw gateway                  # use config defaults
  zeroclaw gateway -p 8080          # listen on port 8080
  zeroclaw gateway --host 0.0.0.0   # bind to all interfaces
  zeroclaw gateway --open-dashboard # open web dashboard automatically
  zeroclaw gateway -p 0             # random available port
  zeroclaw gateway --new-pairing    # clear tokens and generate fresh pairing code")]
    Gateway(GatewayArgs),

    /// Start long-running autonomous runtime (gateway + channels + heartbeat + scheduler)
    #[command(long_about = "\
Start the long-running autonomous daemon.

Launches the full ZeroClaw runtime: gateway server, all configured \
channels (Telegram, Discord, Slack, etc.), heartbeat monitor, and \
the cron scheduler. This is the recommended way to run ZeroClaw in \
production or as an always-on assistant.

Use 'zeroclaw service install' to register the daemon as an OS \
service (systemd/launchd) for auto-start on boot.

Examples:
  zeroclaw daemon                   # use config defaults
  zeroclaw daemon -p 9090           # gateway on port 9090
  zeroclaw daemon --host 127.0.0.1  # localhost only")]
    Daemon(DaemonArgs),

    /// Manage OS service lifecycle (launchd/systemd user service)
    Service(ServiceArgs),

    /// Run diagnostics for daemon/scheduler/channel freshness
    Doctor(DoctorArgs),

    /// Show system status (full details)
    Status,

    /// Self-update ZeroClaw to the latest version
    #[command(long_about = "\
Self-update ZeroClaw to the latest release from GitHub.

Downloads the appropriate pre-built binary for your platform and
replaces the current executable. Requires write permissions to
the binary location.

Examples:
  zeroclaw update              # Update to latest version
  zeroclaw update --check      # Check for updates without installing
  zeroclaw update --instructions # Show install-method-specific update instructions
  zeroclaw update --force      # Reinstall even if already up to date")]
    Update(UpdateArgs),

    /// Engage, inspect, and resume emergency-stop states.
    ///
    /// Examples:
    /// - `zeroclaw estop`
    /// - `zeroclaw estop --level network-kill`
    /// - `zeroclaw estop --level domain-block --domain "*.chase.com"`
    /// - `zeroclaw estop --level tool-freeze --tool shell --tool browser`
    /// - `zeroclaw estop status`
    /// - `zeroclaw estop resume --network`
    /// - `zeroclaw estop resume --domain "*.chase.com"`
    /// - `zeroclaw estop resume --tool shell`
    Estop(EstopArgs),

    /// Configure and manage scheduled tasks
    #[command(long_about = "\
Configure and manage scheduled tasks.

Schedule recurring, one-shot, or interval-based tasks using cron \
expressions, RFC 3339 timestamps, durations, or fixed intervals.

Cron expressions use the standard 5-field format: \
'min hour day month weekday'. Timezones default to UTC; \
override with --tz and an IANA timezone name.

Examples:
  zeroclaw cron list
  zeroclaw cron add '0 9 * * 1-5' 'Good morning' --tz America/New_York
  zeroclaw cron add '*/30 * * * *' 'Check system health'
  zeroclaw cron add-at 2025-01-15T14:00:00Z 'Send reminder'
  zeroclaw cron add-every 60000 'Ping heartbeat'
  zeroclaw cron once 30m 'Run backup in 30 minutes'
  zeroclaw cron pause <task-id>
  zeroclaw cron update <task-id> --expression '0 8 * * *' --tz Europe/London")]
    Cron(CronArgs),

    /// Manage provider model catalogs
    Models(ModelsArgs),

    /// List supported AI providers
    Providers,

    /// Show provider quota and rate limit status
    #[command(
        name = "providers-quota",
        long_about = "\
Show provider quota and rate limit status.

Displays quota remaining, rate limit resets, circuit breaker state, \
and per-profile breakdown for all configured providers. Helps diagnose \
quota exhaustion and rate limiting issues.

Examples:
  zeroclaw providers-quota                    # text output, all providers
  zeroclaw providers-quota --format json      # JSON output
  zeroclaw providers-quota --provider gemini  # filter by provider"
    )]
    ProvidersQuota(ProvidersQuotaArgs),
    /// Manage channels (telegram, discord, slack, github)
    #[command(long_about = "\
Manage communication channels.

Add, remove, list, and health-check channels that connect ZeroClaw \
to messaging platforms. Supported channel types: telegram, discord, \
slack, whatsapp, github, matrix, imessage, email.

Examples:
  zeroclaw channel list
  zeroclaw channel doctor
  zeroclaw channel add telegram '{\"bot_token\":\"...\",\"name\":\"my-bot\"}'
  zeroclaw channel remove my-bot
  zeroclaw channel bind-telegram zeroclaw_user")]
    Channel(ChannelArgs),

    /// Browse 50+ integrations
    Integrations(IntegrationsArgs),

    /// Manage skills (user-defined capabilities)
    #[command(name = "skill", alias = "skills")]
    Skills(SkillsArgs),

    /// Migrate data from other agent runtimes
    Migrate(MigrateArgs),

    /// Manage provider subscription authentication profiles
    Auth(AuthArgs),

    /// Discover and introspect USB hardware
    #[command(long_about = "\
Discover and introspect USB hardware.

Enumerate connected USB devices, identify known development boards \
(STM32 Nucleo, Arduino, ESP32), and retrieve chip information via \
probe-rs / ST-Link.

Examples:
  zeroclaw hardware discover
  zeroclaw hardware introspect /dev/ttyACM0
  zeroclaw hardware info --chip STM32F401RETx")]
    Hardware(HardwareArgs),

    /// Manage hardware peripherals (STM32, RPi GPIO, etc.)
    #[command(long_about = "\
Manage hardware peripherals.

Add, list, flash, and configure hardware boards that expose tools \
to the agent (GPIO, sensors, actuators). Supported boards: \
nucleo-f401re, rpi-gpio, esp32, arduino-uno.

Examples:
  zeroclaw peripheral list
  zeroclaw peripheral add nucleo-f401re /dev/ttyACM0
  zeroclaw peripheral add rpi-gpio native
  zeroclaw peripheral flash --port /dev/cu.usbmodem12345
  zeroclaw peripheral flash-nucleo")]
    Peripheral(PeripheralArgs),

    /// Manage agent memory (list, get, stats, clear)
    #[command(long_about = "\
Manage agent memory entries.

List, inspect, and clear memory entries stored by the agent. \
Supports filtering by category and session, pagination, and \
batch clearing with confirmation.

Examples:
  zeroclaw memory stats
  zeroclaw memory list
  zeroclaw memory list --category core --limit 10
  zeroclaw memory get <key>
  zeroclaw memory clear --category conversation --yes")]
    Memory(MemoryArgs),

    /// Manage configuration
    #[command(long_about = "\
Manage ZeroClaw configuration.

Inspect, query, and modify configuration settings.

Examples:
  zeroclaw config show                        # show effective config (secrets masked)
  zeroclaw config get gateway.port            # query a specific value by dot-path
  zeroclaw config set gateway.port 8080       # update a value and save to config.toml
  zeroclaw config schema                      # print full JSON Schema to stdout")]
    Config(ConfigArgs),

    /// Generate shell completion script to stdout
    #[command(long_about = "\
Generate shell completion scripts for `zeroclaw`.

The script is printed to stdout so it can be sourced directly:

Examples:
  source <(zeroclaw completions bash)
  zeroclaw completions zsh > ~/.zfunc/_zeroclaw
  zeroclaw completions fish > ~/.config/fish/completions/zeroclaw.fish")]
    Completions(CompletionsArgs),
}
