use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use zeroclaw::{
    security, ChannelCommands, CronCommands, IntegrationCommands, MemoryCommands, MigrateCommands,
    ServiceCommands, SkillCommands, ZEROCLAW_BUILD_VERSION,
};

use crate::QuotaFormat;

fn parse_temperature(s: &str) -> std::result::Result<f64, String> {
    let t: f64 = s.parse().map_err(|e| format!("{e}"))?;
    if !(0.0..=2.0).contains(&t) {
        return Err("temperature must be between 0.0 and 2.0".to_string());
    }
    Ok(t)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum EstopLevelArg {
    #[value(name = "kill-all")]
    KillAll,
    #[value(name = "network-kill")]
    NetworkKill,
    #[value(name = "domain-block")]
    DomainBlock,
    #[value(name = "tool-freeze")]
    ToolFreeze,
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
    pub(crate) command: Commands,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    /// Initialize your workspace and configuration
    Onboard {
        /// Run the full interactive wizard (default is quick setup)
        #[arg(long)]
        interactive: bool,

        /// Run the full-screen TUI onboarding flow (ratatui)
        #[arg(long)]
        interactive_ui: bool,

        /// Overwrite existing config without confirmation
        #[arg(long)]
        force: bool,

        /// Reconfigure channels only (fast repair flow)
        #[arg(long)]
        channels_only: bool,

        /// API key (used in quick mode, ignored with --interactive or --interactive-ui)
        #[arg(long)]
        api_key: Option<String>,

        /// Provider name (used in quick mode, default: openrouter)
        #[arg(long)]
        provider: Option<String>,
        /// Model ID override (used in quick mode)
        #[arg(long)]
        model: Option<String>,
        /// Memory backend (sqlite, lucid, markdown, none) - used in quick mode, default: sqlite
        #[arg(long)]
        memory: Option<String>,

        /// Disable OTP in quick setup (not recommended)
        #[arg(long)]
        no_totp: bool,

        /// Merge-migrate data from OpenClaw during onboarding
        #[arg(long)]
        migrate_openclaw: bool,

        /// Optional OpenClaw workspace path (defaults to ~/.openclaw/workspace)
        #[arg(long)]
        openclaw_source: Option<std::path::PathBuf>,

        /// Optional OpenClaw config path (defaults to ~/.openclaw/openclaw.json)
        #[arg(long)]
        openclaw_config: Option<std::path::PathBuf>,
    },

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
    Agent {
        /// Single message mode (don't enter interactive mode)
        #[arg(short, long)]
        message: Option<String>,

        /// Provider to use (openrouter, anthropic, openai, openai-codex)
        #[arg(short, long)]
        provider: Option<String>,

        /// Model to use
        #[arg(long)]
        model: Option<String>,

        /// Temperature (0.0 - 2.0)
        #[arg(short, long, default_value = "0.7", value_parser = parse_temperature)]
        temperature: f64,

        /// Attach a peripheral (board:path, e.g. nucleo-f401re:/dev/ttyACM0)
        #[arg(long)]
        peripheral: Vec<String>,

        /// Autonomy level (read_only, supervised, full)
        #[arg(long, value_parser = clap::value_parser!(security::AutonomyLevel))]
        autonomy_level: Option<security::AutonomyLevel>,

        /// Maximum shell/tool actions per hour
        #[arg(long)]
        max_actions_per_hour: Option<u32>,

        /// Maximum tool-call iterations per message
        #[arg(long)]
        max_tool_iterations: Option<usize>,

        /// Maximum conversation history messages
        #[arg(long)]
        max_history_messages: Option<usize>,

        /// Enable compact context mode (smaller prompts for limited models)
        #[arg(long)]
        compact_context: bool,

        /// Memory backend (sqlite, markdown, none)
        #[arg(long)]
        memory_backend: Option<String>,
    },

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
    Gateway {
        /// Port to listen on (use 0 for random available port); defaults to config gateway.port
        #[arg(short, long)]
        port: Option<u16>,

        /// Host to bind to; defaults to config gateway.host
        #[arg(long)]
        host: Option<String>,

        /// Clear all paired tokens and generate a fresh pairing code
        #[arg(long)]
        new_pairing: bool,

        /// Open the web dashboard URL in the default browser on startup
        #[arg(long)]
        open_dashboard: bool,
    },

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
    Daemon {
        /// Port to listen on (use 0 for random available port); defaults to config gateway.port
        #[arg(short, long)]
        port: Option<u16>,

        /// Host to bind to; defaults to config gateway.host
        #[arg(long)]
        host: Option<String>,
    },

    /// Manage OS service lifecycle (launchd/systemd user service)
    Service {
        /// Init system to use: auto (detect), systemd, or openrc
        #[arg(long, default_value = "auto", value_parser = ["auto", "systemd", "openrc"])]
        service_init: String,

        #[command(subcommand)]
        service_command: ServiceCommands,
    },

    /// Run diagnostics for daemon/scheduler/channel freshness
    Doctor {
        #[command(subcommand)]
        doctor_command: Option<DoctorCommands>,
    },

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
    Update {
        /// Check for updates without installing
        #[arg(long, conflicts_with_all = ["force", "instructions"])]
        check: bool,

        /// Force update even if already at latest version
        #[arg(long, conflicts_with = "instructions")]
        force: bool,

        /// Show human-friendly update instructions for your installation method
        #[arg(long, conflicts_with_all = ["check", "force"])]
        instructions: bool,
    },

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
    Estop {
        #[command(subcommand)]
        estop_command: Option<EstopSubcommands>,

        /// Level used when engaging estop from `zeroclaw estop`.
        #[arg(long, value_enum)]
        level: Option<EstopLevelArg>,

        /// Domain pattern(s) for `domain-block` (repeatable).
        #[arg(long = "domain")]
        domains: Vec<String>,

        /// Tool name(s) for `tool-freeze` (repeatable).
        #[arg(long = "tool")]
        tools: Vec<String>,
    },

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
    Cron {
        #[command(subcommand)]
        cron_command: CronCommands,
    },

    /// Manage provider model catalogs
    Models {
        #[command(subcommand)]
        model_command: ModelCommands,
    },

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
    ProvidersQuota {
        /// Filter by provider name (optional, shows all if omitted)
        #[arg(long)]
        provider: Option<String>,

        /// Output format (text or json)
        #[arg(long, value_enum, default_value_t = QuotaFormat::Text)]
        format: QuotaFormat,
    },
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
    Channel {
        #[command(subcommand)]
        channel_command: ChannelCommands,
    },

    /// Browse 50+ integrations
    Integrations {
        #[command(subcommand)]
        integration_command: IntegrationCommands,
    },

    /// Manage skills (user-defined capabilities)
    #[command(name = "skill", alias = "skills")]
    Skills {
        #[command(subcommand)]
        skill_command: SkillCommands,
    },

    /// Migrate data from other agent runtimes
    Migrate {
        #[command(subcommand)]
        migrate_command: MigrateCommands,
    },

    /// Manage provider subscription authentication profiles
    Auth {
        #[command(subcommand)]
        auth_command: AuthCommands,
    },

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
    Hardware {
        #[command(subcommand)]
        hardware_command: zeroclaw::HardwareCommands,
    },

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
    Peripheral {
        #[command(subcommand)]
        peripheral_command: zeroclaw::PeripheralCommands,
    },

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
    Memory {
        #[command(subcommand)]
        memory_command: MemoryCommands,
    },

    /// Manage configuration
    #[command(long_about = "\
Manage ZeroClaw configuration.

Inspect, query, and modify configuration settings.

Examples:
  zeroclaw config show                        # show effective config (secrets masked)
  zeroclaw config get gateway.port            # query a specific value by dot-path
  zeroclaw config set gateway.port 8080       # update a value and save to config.toml
  zeroclaw config schema                      # print full JSON Schema to stdout")]
    Config {
        #[command(subcommand)]
        config_command: ConfigCommands,
    },

    /// Generate shell completion script to stdout
    #[command(long_about = "\
Generate shell completion scripts for `zeroclaw`.

The script is printed to stdout so it can be sourced directly:

Examples:
  source <(zeroclaw completions bash)
  zeroclaw completions zsh > ~/.zfunc/_zeroclaw
  zeroclaw completions fish > ~/.config/fish/completions/zeroclaw.fish")]
    Completions {
        /// Target shell
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ConfigCommands {
    /// Show the current effective configuration (secrets masked)
    Show,
    /// Get a specific configuration value by dot-path (e.g. "gateway.port")
    Get {
        /// Dot-separated config path, e.g. "security.estop.enabled"
        key: String,
    },
    /// Set a configuration value and save to config.toml
    Set {
        /// Dot-separated config path, e.g. "gateway.port"
        key: String,
        /// New value (string, number, boolean, or JSON for objects/arrays)
        value: String,
    },
    /// Dump the full configuration JSON Schema to stdout
    Schema,
}

#[derive(Subcommand, Debug)]
pub(crate) enum EstopSubcommands {
    /// Print current estop status.
    Status,
    /// Resume from an engaged estop level.
    Resume {
        /// Resume only network kill.
        #[arg(long)]
        network: bool,
        /// Resume one or more blocked domain patterns.
        #[arg(long = "domain")]
        domains: Vec<String>,
        /// Resume one or more frozen tools.
        #[arg(long = "tool")]
        tools: Vec<String>,
        /// OTP code. If omitted and OTP is required, a prompt is shown.
        #[arg(long)]
        otp: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum AuthCommands {
    /// Login with OAuth (OpenAI Codex or Gemini)
    Login {
        /// Provider (`openai-codex` or `gemini`)
        #[arg(long)]
        provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
        /// Use OAuth device-code flow
        #[arg(long)]
        device_code: bool,
    },
    /// Complete OAuth by pasting redirect URL or auth code
    PasteRedirect {
        /// Provider (`openai-codex`)
        #[arg(long)]
        provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
        /// Full redirect URL or raw OAuth code
        #[arg(long)]
        input: Option<String>,
    },
    /// Paste setup token / auth token (for Anthropic subscription auth)
    PasteToken {
        /// Provider (`anthropic`)
        #[arg(long)]
        provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
        /// Token value (if omitted, read interactively)
        #[arg(long)]
        token: Option<String>,
        /// Auth kind override (`authorization` or `api-key`)
        #[arg(long)]
        auth_kind: Option<String>,
    },
    /// Alias for `paste-token` (interactive by default)
    SetupToken {
        /// Provider (`anthropic`)
        #[arg(long)]
        provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
    },
    /// Refresh OpenAI Codex access token using refresh token
    Refresh {
        /// Provider (`openai-codex`)
        #[arg(long)]
        provider: String,
        /// Profile name or profile id
        #[arg(long)]
        profile: Option<String>,
    },
    /// Remove auth profile
    Logout {
        /// Provider
        #[arg(long)]
        provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
    },
    /// Set active profile for a provider
    Use {
        /// Provider
        #[arg(long)]
        provider: String,
        /// Profile name or full profile id
        #[arg(long)]
        profile: String,
    },
    /// List auth profiles
    List,
    /// Show auth status with active profile and token expiry info
    Status,
}

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

#[derive(Subcommand, Debug)]
pub(crate) enum DoctorCommands {
    /// Probe model catalogs across providers and report availability
    Models {
        /// Probe a specific provider only (default: all known providers)
        #[arg(long)]
        provider: Option<String>,

        /// Prefer cached catalogs when available (skip forced live refresh)
        #[arg(long)]
        use_cache: bool,
    },
    /// Query runtime trace events (tool diagnostics and model replies)
    Traces {
        /// Show a specific trace event by id
        #[arg(long)]
        id: Option<String>,
        /// Filter list output by event type
        #[arg(long)]
        event: Option<String>,
        /// Case-insensitive text match across message/payload
        #[arg(long)]
        contains: Option<String>,
        /// Maximum number of events to display
        #[arg(long, default_value = "20")]
        limit: usize,
    },
}
