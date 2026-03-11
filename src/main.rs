#![forbid(unsafe_code)]
#![recursion_limit = "256"]

use anyhow::{bail, Context, Result};
use clap::{CommandFactory, Parser, ValueEnum};
use clap_complete::Shell;
use dialoguer::{Input, Password};
use serde::{Deserialize, Serialize};
use std::io::Write;
use tracing::{info, warn};
use tracing_subscriber::{fmt, EnvFilter};
use zeroclaw::{
    agent, auth, channels, config, cron, daemon, doctor, gateway, hardware, integrations, memory,
    migration, observability, onboard, peripherals, providers, security, service, skills, update,
    ZEROCLAW_BUILD_VERSION,
};

mod cmd;

const PROFILE_MISMATCH_PREFIX: &str = "Pending login profile mismatch:";

#[derive(Debug, Clone, ValueEnum)]
enum QuotaFormat {
    Text,
    Json,
}

fn dashboard_open_url(host: &str, port: u16) -> String {
    let bind_host = host.trim();
    let browser_host = match bind_host {
        "0.0.0.0" | "::" | "[::]" => "127.0.0.1",
        _ => bind_host,
    };
    format!("http://{browser_host}:{port}/")
}

async fn open_url_in_default_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = tokio::process::Command::new("open");
        command.arg(url);
        command
    };

    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = tokio::process::Command::new("xdg-open");
        command.arg(url);
        command
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = tokio::process::Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    };

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = url;
        bail!("automatic dashboard open is unsupported on this platform");
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        let status = command.status().await?;
        if status.success() {
            Ok(())
        } else {
            bail!("browser launcher exited with status {status}");
        }
    }
}

use config::Config;

// Re-export so binary modules can use crate::<CommandEnum> while keeping a single source of truth.
pub use zeroclaw::{
    ChannelCommands, CronCommands, HardwareCommands, IntegrationCommands, MigrateCommands,
    PeripheralCommands, ServiceCommands, SkillCommands,
};

use crate::cmd::{
    AuthCommands, Cli, Commands, ConfigCommands, DoctorCommands, EstopLevelArg, EstopSubcommands,
    ModelCommands,
};
async fn run_onboard(cli: &Cli) -> Result<()> {
    // Onboard runs quick setup by default, interactive wizard with --interactive,
    // or full-screen TUI with --interactive-ui.
    // The onboard wizard uses reqwest::blocking internally, which creates its own
    // Tokio runtime. To avoid "Cannot drop a runtime in a context where blocking is
    // not allowed", we run the wizard on a blocking thread via spawn_blocking.
    if let Commands::Onboard {
        interactive,
        interactive_ui,
        force,
        channels_only,
        ref api_key,
        ref provider,
        ref model,
        ref memory,
        no_totp,
        migrate_openclaw,
        ref openclaw_source,
        ref openclaw_config,
    } = cli.command
    {
        let openclaw_source = openclaw_source.clone();
        let openclaw_config = openclaw_config.clone();
        let openclaw_migration_enabled =
            migrate_openclaw || openclaw_source.is_some() || openclaw_config.is_some();

        if interactive && interactive_ui {
            bail!("Use either --interactive or --interactive-ui, not both");
        }
        if interactive && channels_only {
            bail!("Use either --interactive or --channels-only, not both");
        }
        if interactive_ui && channels_only {
            bail!("Use either --interactive-ui or --channels-only, not both");
        }
        if interactive_ui
            && (api_key.is_some()
                || provider.is_some()
                || model.is_some()
                || memory.is_some()
                || no_totp)
        {
            bail!(
                "--interactive-ui does not accept --api-key, --provider, --model, --memory, or --no-totp"
            );
        }
        if channels_only
            && (api_key.is_some()
                || provider.is_some()
                || model.is_some()
                || memory.is_some()
                || no_totp
                || migrate_openclaw
                || openclaw_source.is_some()
                || openclaw_config.is_some())
        {
            bail!(
                "--channels-only does not accept --api-key, --provider, --model, --memory, --no-totp, or OpenClaw migration flags"
            );
        }
        if channels_only && force {
            bail!("--channels-only does not accept --force");
        }

        let config = if channels_only {
            Box::pin(onboard::run_channels_repair_wizard()).await
        } else if interactive_ui {
            Box::pin(onboard::run_wizard_tui_with_migration(
                force,
                onboard::OpenClawOnboardMigrationOptions {
                    enabled: openclaw_migration_enabled,
                    source_workspace: openclaw_source,
                    source_config: openclaw_config,
                },
            ))
            .await
        } else if interactive {
            Box::pin(onboard::run_wizard_with_migration(
                force,
                onboard::OpenClawOnboardMigrationOptions {
                    enabled: openclaw_migration_enabled,
                    source_workspace: openclaw_source,
                    source_config: openclaw_config,
                },
            ))
            .await
        } else {
            onboard::run_quick_setup_with_migration(
                api_key.as_deref(),
                provider.as_deref(),
                model.as_deref(),
                memory.as_deref(),
                force,
                no_totp,
                onboard::OpenClawOnboardMigrationOptions {
                    enabled: openclaw_migration_enabled,
                    source_workspace: openclaw_source,
                    source_config: openclaw_config,
                },
            )
            .await
        }?;
        // Auto-start channels if user said yes during wizard
        if std::env::var("ZEROCLAW_AUTOSTART_CHANNELS").as_deref() == Ok("1") {
            Box::pin(channels::start_channels(config)).await?;
        }
        Ok(())
    } else {
        unreachable!()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Install default crypto provider for Rustls TLS.
    // This prevents the error: "could not automatically determine the process-level CryptoProvider"
    // when both aws-lc-rs and ring features are available (or neither is explicitly selected).
    if let Err(e) = rustls::crypto::ring::default_provider().install_default() {
        eprintln!("Warning: Failed to install default crypto provider: {e:?}");
    }

    let cli = Cli::parse();

    if let Some(config_dir) = &cli.config_dir {
        if config_dir.trim().is_empty() {
            bail!("--config-dir cannot be empty");
        }
        std::env::set_var("ZEROCLAW_CONFIG_DIR", config_dir);
    }

    // Completions must remain stdout-only and should not load config or initialize logging.
    // This avoids warnings/log lines corrupting sourced completion scripts.
    if let Commands::Completions { shell } = &cli.command {
        let mut stdout = std::io::stdout().lock();
        write_shell_completion(*shell, &mut stdout)?;
        return Ok(());
    }

    // Initialize logging - respects RUST_LOG env var, defaults to INFO
    let subscriber = fmt::Subscriber::builder()
        .with_ansi(true)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    run_onboard(&cli).await?;

    // All other commands need config loaded first
    let mut config = Config::load_or_init().await?;
    config.apply_env_overrides();
    observability::runtime_trace::init_from_config(&config.observability, &config.workspace_dir);
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

    match cli.command {
        Commands::Onboard { .. } | Commands::Completions { .. } => unreachable!(),

        Commands::Agent {
            message,
            provider,
            model,
            temperature,
            peripheral,
            autonomy_level,
            max_actions_per_hour,
            max_tool_iterations,
            max_history_messages,
            compact_context,
            memory_backend,
        } => {
            if let Some(level) = autonomy_level {
                config.autonomy.level = level;
            }
            if let Some(n) = max_actions_per_hour {
                config.autonomy.max_actions_per_hour = n;
            }
            if let Some(n) = max_tool_iterations {
                config.agent.max_tool_iterations = n;
            }
            if let Some(n) = max_history_messages {
                config.agent.max_history_messages = n;
            }
            if compact_context {
                config.agent.compact_context = true;
            }
            if let Some(ref backend) = memory_backend {
                config.memory.backend = backend.clone();
            }
            // interactive=true only when no --message flag (real REPL session).
            // Single-shot mode (-m) runs non-interactively: no TTY approval prompt,
            // so tools are not denied by a stdin read returning EOF.
            let interactive = message.is_none();
            Box::pin(agent::run(
                config,
                message,
                provider,
                model,
                temperature,
                peripheral,
                interactive,
                None,
            ))
            .await
            .map(|_| ())
        }

        Commands::Gateway {
            port,
            host,
            new_pairing,
            open_dashboard,
        } => {
            if new_pairing {
                // Persist token reset from raw config so env-derived overrides are not written to disk.
                let mut persisted_config = Config::load_or_init().await?;
                persisted_config.gateway.paired_tokens.clear();
                persisted_config.save().await?;
                config.gateway.paired_tokens.clear();
                info!("🔐 Cleared paired tokens — a fresh pairing code will be generated");
            }
            let port = port.unwrap_or(config.gateway.port);
            let host = host.unwrap_or_else(|| config.gateway.host.clone());
            if port == 0 {
                info!("🚀 Starting ZeroClaw Gateway on {host} (random port)");
            } else {
                info!("🚀 Starting ZeroClaw Gateway on {host}:{port}");
            }
            if open_dashboard {
                if port == 0 {
                    warn!(
                        "--open-dashboard requires a fixed port; skipping auto-open because --port 0 uses a random port"
                    );
                } else {
                    let dashboard_url = dashboard_open_url(&host, port);
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(750)).await;
                        if let Err(err) = open_url_in_default_browser(&dashboard_url).await {
                            warn!(
                                "Could not open dashboard automatically ({err}). Open manually: {dashboard_url}"
                            );
                        } else {
                            info!("🌐 Opened dashboard in browser: {dashboard_url}");
                        }
                    });
                }
            }
            gateway::run_gateway(&host, port, config).await
        }

        Commands::Daemon { port, host } => {
            let port = port.unwrap_or(config.gateway.port);
            let host = host.unwrap_or_else(|| config.gateway.host.clone());
            if port == 0 {
                info!("🧠 Starting ZeroClaw Daemon on {host} (random port)");
            } else {
                info!("🧠 Starting ZeroClaw Daemon on {host}:{port}");
            }
            daemon::run(config, host, port).await
        }

        Commands::Status => {
            println!("🦀 ZeroClaw Status");
            println!();
            println!("Version:     {}", ZEROCLAW_BUILD_VERSION);
            println!("Workspace:   {}", config.workspace_dir.display());
            println!("Config:      {}", config.config_path.display());
            println!();
            println!(
                "🤖 Provider:      {}",
                config.default_provider.as_deref().unwrap_or("openrouter")
            );
            println!(
                "   Model:         {}",
                config.default_model.as_deref().unwrap_or("(default)")
            );
            println!("📊 Observability:  {}", config.observability.backend);
            println!(
                "🧾 Trace storage:  {} ({})",
                config.observability.runtime_trace_mode, config.observability.runtime_trace_path
            );
            println!("🛡️  Autonomy:      {:?}", config.autonomy.level);
            println!("⚙️  Runtime:       {}", config.runtime.kind);
            let effective_memory_backend = memory::effective_memory_backend_name(
                &config.memory.backend,
                Some(&config.storage.provider.config),
            );
            println!(
                "💓 Heartbeat:      {}",
                if config.heartbeat.enabled {
                    format!("every {}min", config.heartbeat.interval_minutes)
                } else {
                    "disabled".into()
                }
            );
            println!(
                "🧠 Memory:         {} (auto-save: {})",
                effective_memory_backend,
                if config.memory.auto_save { "on" } else { "off" }
            );

            println!();
            println!("Security:");
            println!("  Workspace only:    {}", config.autonomy.workspace_only);
            println!(
                "  Allowed roots:     {}",
                if config.autonomy.allowed_roots.is_empty() {
                    "(none)".to_string()
                } else {
                    config.autonomy.allowed_roots.join(", ")
                }
            );
            println!(
                "  Allowed commands:  {}",
                config.autonomy.allowed_commands.join(", ")
            );
            println!(
                "  Max actions/hour:  {}",
                config.autonomy.max_actions_per_hour
            );
            println!(
                "  Max cost/day:      ${:.2}",
                f64::from(config.autonomy.max_cost_per_day_cents) / 100.0
            );
            println!("  OTP enabled:       {}", config.security.otp.enabled);
            println!("  E-stop enabled:    {}", config.security.estop.enabled);
            println!();
            println!("Channels:");
            println!("  CLI:      ✅ always");
            for (channel, configured) in config.channels_config.channels() {
                println!(
                    "  {:9} {}",
                    channel.name(),
                    if configured {
                        "✅ configured"
                    } else {
                        "❌ not configured"
                    }
                );
            }
            println!();
            println!("Peripherals:");
            println!(
                "  Enabled:   {}",
                if config.peripherals.enabled {
                    "yes"
                } else {
                    "no"
                }
            );
            println!("  Boards:    {}", config.peripherals.boards.len());

            Ok(())
        }

        Commands::Update {
            check,
            force,
            instructions,
        } => {
            if instructions {
                update::print_update_instructions()?;
                Ok(())
            } else {
                update::self_update(force, check).await?;
                Ok(())
            }
        }

        Commands::Estop {
            estop_command,
            level,
            domains,
            tools,
        } => handle_estop_command(&config, estop_command, level, domains, tools),

        Commands::Cron { cron_command } => cron::handle_command(cron_command, &config),

        Commands::Models { model_command } => match model_command {
            ModelCommands::Refresh {
                provider,
                all,
                force,
            } => {
                if all {
                    if provider.is_some() {
                        bail!("`models refresh --all` cannot be combined with --provider");
                    }
                    onboard::run_models_refresh_all(&config, force).await
                } else {
                    onboard::run_models_refresh(&config, provider.as_deref(), force).await
                }
            }
            ModelCommands::List { provider } => {
                onboard::run_models_list(&config, provider.as_deref()).await
            }
            ModelCommands::Set { model } => onboard::run_models_set(&config, &model).await,
            ModelCommands::Status => onboard::run_models_status(&config).await,
        },

        Commands::ProvidersQuota { provider, format } => {
            let format_str = match format {
                QuotaFormat::Text => "text",
                QuotaFormat::Json => "json",
            };
            providers::quota_cli::run(&config, provider.as_deref(), format_str).await
        }

        Commands::Providers => {
            let providers = providers::list_providers();
            let current = config
                .default_provider
                .as_deref()
                .unwrap_or("openrouter")
                .trim()
                .to_ascii_lowercase();
            println!("Supported providers ({} total):\n", providers.len());
            println!("  ID (use in config)  DESCRIPTION");
            println!("  ─────────────────── ───────────");
            for p in &providers {
                let is_active = p.name.eq_ignore_ascii_case(&current)
                    || p.aliases
                        .iter()
                        .any(|alias| alias.eq_ignore_ascii_case(&current));
                let marker = if is_active { " (active)" } else { "" };
                let local_tag = if p.local { " [local]" } else { "" };
                let aliases = if p.aliases.is_empty() {
                    String::new()
                } else {
                    format!("  (aliases: {})", p.aliases.join(", "))
                };
                println!(
                    "  {:<19} {}{}{}{}",
                    p.name, p.display_name, local_tag, marker, aliases
                );
            }
            println!("\n  custom:<URL>   Any OpenAI-compatible endpoint");
            println!("  anthropic-custom:<URL>  Any Anthropic-compatible endpoint");
            Ok(())
        }

        Commands::Service {
            service_command,
            service_init,
        } => {
            let init_system = service_init.parse()?;
            service::handle_command(&service_command, &config, init_system)
        }

        Commands::Doctor { doctor_command } => match doctor_command {
            Some(DoctorCommands::Models {
                provider,
                use_cache,
            }) => doctor::run_models(&config, provider.as_deref(), use_cache).await,
            Some(DoctorCommands::Traces {
                id,
                event,
                contains,
                limit,
            }) => doctor::run_traces(
                &config,
                id.as_deref(),
                event.as_deref(),
                contains.as_deref(),
                limit,
            ),
            None => doctor::run(&config),
        },

        Commands::Channel { channel_command } => match channel_command {
            ChannelCommands::Start => Box::pin(channels::start_channels(config)).await,
            ChannelCommands::Doctor => Box::pin(channels::doctor_channels(config)).await,
            other => channels::handle_command(other, &config).await,
        },

        Commands::Integrations {
            integration_command,
        } => integrations::handle_command(integration_command, &config),

        Commands::Skills { skill_command } => skills::handle_command(skill_command, &config),

        Commands::Migrate { migrate_command } => {
            migration::handle_command(migrate_command, &config).await
        }

        Commands::Memory { memory_command } => {
            memory::cli::handle_command(memory_command, &config).await
        }

        Commands::Auth { auth_command } => handle_auth_command(auth_command, &config).await,

        Commands::Hardware { hardware_command } => {
            hardware::handle_command(hardware_command.clone(), &config)
        }

        Commands::Peripheral { peripheral_command } => {
            peripherals::handle_command(peripheral_command.clone(), &config).await
        }

        Commands::Config { config_command } => match config_command {
            ConfigCommands::Show => {
                let mut json =
                    serde_json::to_value(&config).context("Failed to serialize config")?;
                redact_config_secrets(&mut json);
                println!("{}", serde_json::to_string_pretty(&json)?);
                Ok(())
            }
            ConfigCommands::Get { key } => {
                let mut json =
                    serde_json::to_value(&config).context("Failed to serialize config")?;
                redact_config_secrets(&mut json);

                let mut current = &json;
                for segment in key.split('.') {
                    current = current
                        .get(segment)
                        .with_context(|| format!("Config path not found: {key}"))?;
                }

                match current {
                    serde_json::Value::String(s) => println!("{s}"),
                    serde_json::Value::Bool(b) => println!("{b}"),
                    serde_json::Value::Number(n) => println!("{n}"),
                    serde_json::Value::Null => println!("null"),
                    _ => println!("{}", serde_json::to_string_pretty(current)?),
                }
                Ok(())
            }
            ConfigCommands::Set { key, value } => {
                let mut json =
                    serde_json::to_value(&config).context("Failed to serialize config")?;

                // Parse the new value: try bool, then integer, then float, then JSON, then string
                let new_value = if value == "true" {
                    serde_json::Value::Bool(true)
                } else if value == "false" {
                    serde_json::Value::Bool(false)
                } else if value == "null" {
                    serde_json::Value::Null
                } else if let Ok(n) = value.parse::<i64>() {
                    serde_json::json!(n)
                } else if let Ok(n) = value.parse::<f64>() {
                    serde_json::json!(n)
                } else if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&value) {
                    // JSON object/array (e.g. '["a","b"]' or '{"key":"val"}')
                    parsed
                } else {
                    serde_json::Value::String(value.clone())
                };

                // Navigate to the parent and set the leaf
                let segments: Vec<&str> = key.split('.').collect();
                if segments.is_empty() {
                    bail!("Config key cannot be empty");
                }
                let (parents, leaf) = segments.split_at(segments.len() - 1);

                let mut target = &mut json;
                for segment in parents {
                    target = target
                        .get_mut(*segment)
                        .with_context(|| format!("Config path not found: {key}"))?;
                }

                let leaf_key = leaf[0];
                if target.get(leaf_key).is_none() {
                    bail!("Config path not found: {key}");
                }
                target[leaf_key] = new_value.clone();

                // Deserialize back to Config and save.
                // Preserve runtime-only fields lost during JSON round-trip (#[serde(skip)]).
                let config_path = config.config_path.clone();
                let workspace_dir = config.workspace_dir.clone();
                config = serde_json::from_value(json)
                    .context("Invalid value for this config key — type mismatch")?;
                config.config_path = config_path;
                config.workspace_dir = workspace_dir;
                config.save().await?;

                // Show the saved value
                let display = match &new_value {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                println!("Set {key} = {display}");
                Ok(())
            }
            ConfigCommands::Schema => {
                let schema = schemars::schema_for!(config::Config);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&schema).expect("failed to serialize JSON Schema")
                );
                Ok(())
            }
        },
    }
}

/// Keys whose values are masked in `config show` / `config get` output.
const REDACTED_CONFIG_KEYS: &[&str] = &[
    "api_key",
    "api_keys",
    "bot_token",
    "paired_tokens",
    "db_url",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "secret_key",
    "webhook_secret",
];

fn redact_config_secrets(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if REDACTED_CONFIG_KEYS.contains(&k.as_str()) {
                    match v {
                        serde_json::Value::String(s) if !s.is_empty() => {
                            *v = serde_json::Value::String("***REDACTED***".to_string());
                        }
                        serde_json::Value::Array(arr) if !arr.is_empty() => {
                            *v = serde_json::json!(["***REDACTED***"]);
                        }
                        _ => {}
                    }
                } else {
                    redact_config_secrets(v);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                redact_config_secrets(item);
            }
        }
        _ => {}
    }
}

fn handle_estop_command(
    config: &Config,
    estop_command: Option<EstopSubcommands>,
    level: Option<EstopLevelArg>,
    domains: Vec<String>,
    tools: Vec<String>,
) -> Result<()> {
    if !config.security.estop.enabled {
        bail!("Emergency stop is disabled. Enable [security.estop].enabled = true in config.toml");
    }

    let config_dir = config
        .config_path
        .parent()
        .context("Config path must have a parent directory")?;
    let mut manager = security::EstopManager::load(&config.security.estop, config_dir)?;

    match estop_command {
        Some(EstopSubcommands::Status) => {
            print_estop_status(&manager.status());
            Ok(())
        }
        Some(EstopSubcommands::Resume {
            network,
            domains,
            tools,
            otp,
        }) => {
            let selector = build_resume_selector(network, domains, tools)?;
            let mut otp_code = otp;
            let otp_validator = if config.security.estop.require_otp_to_resume {
                if !config.security.otp.enabled {
                    bail!(
                        "security.estop.require_otp_to_resume=true but security.otp.enabled=false"
                    );
                }
                if otp_code.is_none() {
                    let entered = Password::new()
                        .with_prompt("Enter OTP code")
                        .allow_empty_password(false)
                        .interact()?;
                    otp_code = Some(entered);
                }

                let store = security::SecretStore::new(config_dir, config.secrets.encrypt);
                let (validator, enrollment_uri) =
                    security::OtpValidator::from_config(&config.security.otp, config_dir, &store)?;
                if let Some(uri) = enrollment_uri {
                    println!("Initialized OTP secret for ZeroClaw.");
                    println!("Enrollment URI: {uri}");
                }
                Some(validator)
            } else {
                None
            };

            manager.resume(selector, otp_code.as_deref(), otp_validator.as_ref())?;
            println!("Estop resume completed.");
            print_estop_status(&manager.status());
            Ok(())
        }
        None => {
            let engage_level = build_engage_level(level, domains, tools)?;
            manager.engage(engage_level)?;
            println!("Estop engaged.");
            print_estop_status(&manager.status());
            Ok(())
        }
    }
}

fn build_engage_level(
    level: Option<EstopLevelArg>,
    domains: Vec<String>,
    tools: Vec<String>,
) -> Result<security::EstopLevel> {
    let requested = level.unwrap_or(EstopLevelArg::KillAll);
    match requested {
        EstopLevelArg::KillAll => {
            if !domains.is_empty() || !tools.is_empty() {
                bail!("--domain/--tool are only valid with --level domain-block/tool-freeze");
            }
            Ok(security::EstopLevel::KillAll)
        }
        EstopLevelArg::NetworkKill => {
            if !domains.is_empty() || !tools.is_empty() {
                bail!("--domain/--tool are not valid with --level network-kill");
            }
            Ok(security::EstopLevel::NetworkKill)
        }
        EstopLevelArg::DomainBlock => {
            if domains.is_empty() {
                bail!("--level domain-block requires at least one --domain");
            }
            if !tools.is_empty() {
                bail!("--tool is not valid with --level domain-block");
            }
            Ok(security::EstopLevel::DomainBlock(domains))
        }
        EstopLevelArg::ToolFreeze => {
            if tools.is_empty() {
                bail!("--level tool-freeze requires at least one --tool");
            }
            if !domains.is_empty() {
                bail!("--domain is not valid with --level tool-freeze");
            }
            Ok(security::EstopLevel::ToolFreeze(tools))
        }
    }
}

fn build_resume_selector(
    network: bool,
    domains: Vec<String>,
    tools: Vec<String>,
) -> Result<security::ResumeSelector> {
    let selected =
        usize::from(network) + usize::from(!domains.is_empty()) + usize::from(!tools.is_empty());
    if selected > 1 {
        bail!("Use only one of --network, --domain, or --tool for estop resume");
    }
    if network {
        return Ok(security::ResumeSelector::Network);
    }
    if !domains.is_empty() {
        return Ok(security::ResumeSelector::Domains(domains));
    }
    if !tools.is_empty() {
        return Ok(security::ResumeSelector::Tools(tools));
    }
    Ok(security::ResumeSelector::KillAll)
}

fn print_estop_status(state: &security::EstopState) {
    println!("Estop status:");
    println!(
        "  engaged:        {}",
        if state.is_engaged() { "yes" } else { "no" }
    );
    println!(
        "  kill_all:       {}",
        if state.kill_all { "active" } else { "inactive" }
    );
    println!(
        "  network_kill:   {}",
        if state.network_kill {
            "active"
        } else {
            "inactive"
        }
    );
    if state.blocked_domains.is_empty() {
        println!("  domain_blocks:  (none)");
    } else {
        println!("  domain_blocks:  {}", state.blocked_domains.join(", "));
    }
    if state.frozen_tools.is_empty() {
        println!("  tool_freeze:    (none)");
    } else {
        println!("  tool_freeze:    {}", state.frozen_tools.join(", "));
    }
    if let Some(updated_at) = &state.updated_at {
        println!("  updated_at:     {updated_at}");
    }
}

fn write_shell_completion<W: Write>(generator: Shell, writer: &mut W) -> Result<()> {
    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_owned();

    clap_complete::generate(generator, &mut cmd, bin_name, writer);

    writer.flush()?;
    Ok(())
}

// ─── Generic Pending OAuth Login ────────────────────────────────────────────

/// Generic pending OAuth login state, shared across providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingOAuthLogin {
    provider: String,
    profile: String,
    code_verifier: String,
    state: String,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingOAuthLoginFile {
    #[serde(default)]
    provider: Option<String>,
    profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code_verifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    encrypted_code_verifier: Option<String>,
    state: String,
    created_at: String,
}

fn pending_oauth_login_path(config: &Config, provider: &str) -> std::path::PathBuf {
    let filename = format!("auth-{}-pending.json", provider);
    auth::state_dir_from_config(config).join(filename)
}

fn pending_oauth_secret_store(config: &Config) -> security::secrets::SecretStore {
    security::secrets::SecretStore::new(
        &auth::state_dir_from_config(config),
        config.secrets.encrypt,
    )
}

#[cfg(unix)]
fn set_owner_only_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

/// Check if a pending OAuth login is stale (older than 24 hours).
fn is_pending_login_stale(pending: &PendingOAuthLogin) -> bool {
    if let Ok(created) = chrono::DateTime::parse_from_rfc3339(&pending.created_at) {
        let age = chrono::Utc::now().signed_duration_since(created);
        age > chrono::Duration::hours(24)
    } else {
        // If we can't parse the timestamp, consider it stale
        true
    }
}

fn save_pending_oauth_login(config: &Config, pending: &PendingOAuthLogin) -> Result<()> {
    let path = pending_oauth_login_path(config, &pending.provider);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let secret_store = pending_oauth_secret_store(config);
    let encrypted_code_verifier = secret_store.encrypt(&pending.code_verifier)?;
    let persisted = PendingOAuthLoginFile {
        provider: Some(pending.provider.clone()),
        profile: pending.profile.clone(),
        code_verifier: None,
        encrypted_code_verifier: Some(encrypted_code_verifier),
        state: pending.state.clone(),
        created_at: pending.created_at.clone(),
    };
    let tmp = path.with_extension(format!(
        "tmp.{}.{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let json = serde_json::to_vec_pretty(&persisted)?;
    std::fs::write(&tmp, json)?;
    set_owner_only_permissions(&tmp)?;
    std::fs::rename(tmp, &path)?;
    set_owner_only_permissions(&path)?;
    Ok(())
}

fn load_pending_oauth_login(config: &Config, provider: &str) -> Result<Option<PendingOAuthLogin>> {
    let path = pending_oauth_login_path(config, provider);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path)?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let persisted: PendingOAuthLoginFile = serde_json::from_slice(&bytes)?;
    let secret_store = pending_oauth_secret_store(config);
    let code_verifier = if let Some(encrypted) = persisted.encrypted_code_verifier {
        secret_store.decrypt(&encrypted)?
    } else if let Some(plaintext) = persisted.code_verifier {
        plaintext
    } else {
        bail!("Pending {} login is missing code verifier", provider);
    };

    let pending = PendingOAuthLogin {
        provider: persisted.provider.unwrap_or_else(|| provider.to_string()),
        profile: persisted.profile,
        code_verifier,
        state: persisted.state,
        created_at: persisted.created_at,
    };

    // Auto-cleanup if stale (older than 24 hours)
    if is_pending_login_stale(&pending) {
        println!("ℹ️  Removing stale pending auth file (older than 24h)");
        let _ = std::fs::remove_file(&path);
        return Ok(None);
    }

    Ok(Some(pending))
}

fn clear_pending_oauth_login(config: &Config, provider: &str) {
    let path = pending_oauth_login_path(config, provider);
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&path) {
        let _ = file.set_len(0);
        let _ = file.sync_all();
    }
    let _ = std::fs::remove_file(path);
}

fn read_auth_input(prompt: &str) -> Result<String> {
    let input = Password::new()
        .with_prompt(prompt)
        .allow_empty_password(false)
        .interact()?;
    Ok(input.trim().to_string())
}

fn read_plain_input(prompt: &str) -> Result<String> {
    let input: String = Input::new().with_prompt(prompt).interact_text()?;
    Ok(input.trim().to_string())
}

fn extract_openai_account_id_for_profile(access_token: &str) -> Option<String> {
    let account_id = auth::openai_oauth::extract_account_id_from_jwt(access_token);
    if account_id.is_none() {
        warn!(
            "Could not extract OpenAI account id from OAuth access token; \
             requests may fail until re-authentication."
        );
    }
    account_id
}

fn format_expiry(profile: &auth::profiles::AuthProfile) -> String {
    match profile
        .token_set
        .as_ref()
        .and_then(|token_set| token_set.expires_at)
    {
        Some(ts) => {
            let now = chrono::Utc::now();
            if ts <= now {
                format!("expired at {}", ts.to_rfc3339())
            } else {
                let mins = (ts - now).num_minutes();
                format!("expires in {mins}m ({})", ts.to_rfc3339())
            }
        }
        None => "n/a".to_string(),
    }
}

#[allow(clippy::too_many_lines)]
async fn handle_auth_command(auth_command: AuthCommands, config: &Config) -> Result<()> {
    let auth_service = auth::AuthService::from_config(config);

    match auth_command {
        AuthCommands::Login {
            provider,
            profile,
            device_code,
        } => {
            let provider = auth::normalize_provider(&provider)?;
            let client = reqwest::Client::new();

            match provider.as_str() {
                "gemini" => {
                    // Gemini OAuth flow
                    if device_code {
                        match auth::gemini_oauth::start_device_code_flow(&client).await {
                            Ok(device) => {
                                println!("Google/Gemini device-code login started.");
                                println!("Visit: {}", device.verification_uri);
                                println!("Code:  {}", device.user_code);
                                if let Some(uri_complete) = &device.verification_uri_complete {
                                    println!("Fast link: {uri_complete}");
                                }

                                let token_set =
                                    auth::gemini_oauth::poll_device_code_tokens(&client, &device)
                                        .await?;
                                let account_id = token_set.id_token.as_deref().and_then(
                                    auth::gemini_oauth::extract_account_email_from_id_token,
                                );

                                auth_service
                                    .store_gemini_tokens(&profile, token_set, account_id, true)
                                    .await?;

                                println!("Saved profile {profile}");
                                println!("Active profile for gemini: {profile}");
                                return Ok(());
                            }
                            Err(e) => {
                                let err_msg = e.to_string();
                                if err_msg.contains("403")
                                    || err_msg.contains("Forbidden")
                                    || err_msg.contains("Cloudflare")
                                {
                                    println!(
                                        "ℹ️  Device-code flow is blocked by Cloudflare protection."
                                    );
                                    println!("   This is normal for server environments.");
                                    println!("   Switching to browser authorization flow...");
                                } else if err_msg.contains("invalid_client") {
                                    println!("⚠️  OAuth client configuration error: {}", err_msg);
                                    println!("   Check your GEMINI_OAUTH_CLIENT_ID and GEMINI_OAUTH_CLIENT_SECRET");
                                } else {
                                    println!("ℹ️  Device-code flow unavailable: {}", err_msg);
                                    println!("   Falling back to browser flow.");
                                }
                            }
                        }
                    }

                    let pkce = auth::gemini_oauth::generate_pkce_state();
                    let authorize_url = auth::gemini_oauth::build_authorize_url(&pkce)?;

                    // Save pending login for paste-redirect fallback
                    let pending = PendingOAuthLogin {
                        provider: "gemini".to_string(),
                        profile: profile.clone(),
                        code_verifier: pkce.code_verifier.clone(),
                        state: pkce.state.clone(),
                        created_at: chrono::Utc::now().to_rfc3339(),
                    };
                    save_pending_oauth_login(config, &pending)?;

                    println!("Open this URL in your browser and authorize access:");
                    println!("{authorize_url}");
                    println!();

                    let code = match auth::gemini_oauth::receive_loopback_code(
                        &pkce.state,
                        std::time::Duration::from_secs(180),
                    )
                    .await
                    {
                        Ok(code) => {
                            clear_pending_oauth_login(config, "gemini");
                            code
                        }
                        Err(e) => {
                            println!("Callback capture failed: {e}");
                            println!(
                                "Run `zeroclaw auth paste-redirect --provider gemini --profile {profile}`"
                            );
                            return Ok(());
                        }
                    };

                    let token_set =
                        auth::gemini_oauth::exchange_code_for_tokens(&client, &code, &pkce).await?;
                    let account_id = token_set
                        .id_token
                        .as_deref()
                        .and_then(auth::gemini_oauth::extract_account_email_from_id_token);

                    auth_service
                        .store_gemini_tokens(&profile, token_set, account_id, true)
                        .await?;

                    println!("Saved profile {profile}");
                    println!("Active profile for gemini: {profile}");
                    Ok(())
                }
                "openai-codex" => {
                    // OpenAI Codex OAuth flow
                    if device_code {
                        match auth::openai_oauth::start_device_code_flow(&client).await {
                            Ok(device) => {
                                println!("OpenAI device-code login started.");
                                println!("Visit: {}", device.verification_uri);
                                println!("Code:  {}", device.user_code);
                                if let Some(uri_complete) = &device.verification_uri_complete {
                                    println!("Fast link: {uri_complete}");
                                }
                                if let Some(message) = &device.message {
                                    println!("{message}");
                                }

                                let token_set =
                                    auth::openai_oauth::poll_device_code_tokens(&client, &device)
                                        .await?;
                                let account_id =
                                    extract_openai_account_id_for_profile(&token_set.access_token);

                                auth_service
                                    .store_openai_tokens(&profile, token_set, account_id, true)
                                    .await?;
                                clear_pending_oauth_login(config, "openai");

                                println!("Saved profile {profile}");
                                println!("Active profile for openai-codex: {profile}");
                                return Ok(());
                            }
                            Err(e) => {
                                let err_msg = e.to_string();
                                if err_msg.contains("403")
                                    || err_msg.contains("Forbidden")
                                    || err_msg.contains("Cloudflare")
                                {
                                    println!(
                                        "ℹ️  Device-code flow is blocked by Cloudflare protection."
                                    );
                                    println!("   This is normal for server environments.");
                                    println!("   Switching to browser authorization flow...");
                                } else {
                                    println!("ℹ️  Device-code flow unavailable: {}", err_msg);
                                    println!("   Falling back to browser flow.");
                                }
                            }
                        }
                    }

                    let pkce = auth::openai_oauth::generate_pkce_state();
                    let pending = PendingOAuthLogin {
                        provider: "openai".to_string(),
                        profile: profile.clone(),
                        code_verifier: pkce.code_verifier.clone(),
                        state: pkce.state.clone(),
                        created_at: chrono::Utc::now().to_rfc3339(),
                    };
                    save_pending_oauth_login(config, &pending)?;

                    let authorize_url = auth::openai_oauth::build_authorize_url(&pkce);
                    println!("Open this URL in your browser and authorize access:");
                    println!("{authorize_url}");
                    println!();
                    println!("Waiting for callback at http://localhost:1455/auth/callback ...");

                    let code = match auth::openai_oauth::receive_loopback_code(
                        &pkce.state,
                        std::time::Duration::from_secs(180),
                    )
                    .await
                    {
                        Ok(code) => code,
                        Err(e) => {
                            println!("Callback capture failed: {e}");
                            println!(
                                "Run `zeroclaw auth paste-redirect --provider openai-codex --profile {profile}`"
                            );
                            return Ok(());
                        }
                    };

                    let token_set =
                        auth::openai_oauth::exchange_code_for_tokens(&client, &code, &pkce).await?;
                    let account_id = extract_openai_account_id_for_profile(&token_set.access_token);

                    auth_service
                        .store_openai_tokens(&profile, token_set, account_id, true)
                        .await?;
                    clear_pending_oauth_login(config, "openai");

                    println!("Saved profile {profile}");
                    println!("Active profile for openai-codex: {profile}");
                    Ok(())
                }
                _ => {
                    bail!(
                        "`auth login` supports --provider openai-codex or gemini, got: {provider}"
                    );
                }
            }
        }

        AuthCommands::PasteRedirect {
            provider,
            profile,
            input,
        } => {
            let provider = auth::normalize_provider(&provider)?;

            match provider.as_str() {
                "openai-codex" => {
                    let result = async {
                        let pending =
                            load_pending_oauth_login(config, "openai")?.ok_or_else(|| {
                                anyhow::anyhow!(
                                    "No pending OpenAI login found.\n\n\
                                💡 Please start the login flow first:\n   \
                                zeroclaw auth login --provider openai-codex --profile {}\n\n\
                                Then paste the callback URL or code here.",
                                    profile
                                )
                            })?;

                        if pending.profile != profile {
                            bail!(
                                "{} pending={}, requested={}",
                                PROFILE_MISMATCH_PREFIX,
                                pending.profile,
                                profile
                            );
                        }

                        let redirect_input = match input {
                            Some(value) => value,
                            None => read_plain_input("Paste redirect URL or OAuth code")?,
                        };

                        let code = auth::openai_oauth::parse_code_from_redirect(
                            &redirect_input,
                            Some(&pending.state),
                        )?;

                        let pkce = auth::openai_oauth::PkceState {
                            code_verifier: pending.code_verifier.clone(),
                            code_challenge: String::new(),
                            state: pending.state.clone(),
                        };

                        let client = reqwest::Client::new();
                        let token_set =
                            auth::openai_oauth::exchange_code_for_tokens(&client, &code, &pkce)
                                .await?;
                        let account_id =
                            extract_openai_account_id_for_profile(&token_set.access_token);

                        auth_service
                            .store_openai_tokens(&profile, token_set, account_id, true)
                            .await?;
                        clear_pending_oauth_login(config, "openai");

                        println!("Saved profile {profile}");
                        println!("Active profile for openai-codex: {profile}");
                        Ok(())
                    }
                    .await;

                    if let Err(e) = result {
                        // Cleanup pending file on error
                        if e.to_string().starts_with(PROFILE_MISMATCH_PREFIX) {
                            clear_pending_oauth_login(config, "openai");
                            eprintln!("❌ {}", e);
                            eprintln!(
                                "\n💡 Tip: A previous login attempt was for a different profile."
                            );
                            eprintln!("   The pending auth file has been cleared.");
                            eprintln!("   Please start fresh with:");
                            eprintln!(
                                "   zeroclaw auth login --provider openai-codex --profile {}",
                                profile
                            );
                            std::process::exit(1);
                        }
                        return Err(e);
                    }
                }
                "gemini" => {
                    let result = async {
                        let pending =
                            load_pending_oauth_login(config, "gemini")?.ok_or_else(|| {
                                anyhow::anyhow!(
                                    "No pending Gemini login found.\n\n\
                                💡 Please start the login flow first:\n   \
                                zeroclaw auth login --provider gemini --profile {}\n\n\
                                Then paste the callback URL or code here.",
                                    profile
                                )
                            })?;

                        if pending.profile != profile {
                            bail!(
                                "{} pending={}, requested={}",
                                PROFILE_MISMATCH_PREFIX,
                                pending.profile,
                                profile
                            );
                        }

                        let redirect_input = match input {
                            Some(value) => value,
                            None => read_plain_input("Paste redirect URL or OAuth code")?,
                        };

                        let code = auth::gemini_oauth::parse_code_from_redirect(
                            &redirect_input,
                            Some(&pending.state),
                        )?;

                        let pkce = auth::gemini_oauth::PkceState {
                            code_verifier: pending.code_verifier.clone(),
                            code_challenge: String::new(),
                            state: pending.state.clone(),
                        };

                        let client = reqwest::Client::new();
                        let token_set =
                            auth::gemini_oauth::exchange_code_for_tokens(&client, &code, &pkce)
                                .await?;
                        let account_id = token_set
                            .id_token
                            .as_deref()
                            .and_then(auth::gemini_oauth::extract_account_email_from_id_token);

                        auth_service
                            .store_gemini_tokens(&profile, token_set, account_id, true)
                            .await?;
                        clear_pending_oauth_login(config, "gemini");

                        println!("Saved profile {profile}");
                        println!("Active profile for gemini: {profile}");
                        Ok(())
                    }
                    .await;

                    if let Err(e) = result {
                        // Cleanup pending file on error
                        if e.to_string().starts_with(PROFILE_MISMATCH_PREFIX) {
                            clear_pending_oauth_login(config, "gemini");
                            eprintln!("❌ {}", e);
                            eprintln!(
                                "\n💡 Tip: A previous login attempt was for a different profile."
                            );
                            eprintln!("   The pending auth file has been cleared.");
                            eprintln!("   Please start fresh with:");
                            eprintln!(
                                "   zeroclaw auth login --provider gemini --profile {}",
                                profile
                            );
                            std::process::exit(1);
                        }
                        return Err(e);
                    }
                }
                _ => {
                    bail!("`auth paste-redirect` supports --provider openai-codex or gemini");
                }
            }
            Ok(())
        }

        AuthCommands::PasteToken {
            provider,
            profile,
            token,
            auth_kind,
        } => {
            let provider = auth::normalize_provider(&provider)?;
            let token = match token {
                Some(token) => token.trim().to_string(),
                None => read_auth_input("Paste token")?,
            };
            if token.is_empty() {
                bail!("Token cannot be empty");
            }

            let kind = auth::anthropic_token::detect_auth_kind(&token, auth_kind.as_deref());
            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                "auth_kind".to_string(),
                kind.as_metadata_value().to_string(),
            );

            auth_service
                .store_provider_token(&provider, &profile, &token, metadata, true)
                .await?;
            println!("Saved profile {profile}");
            println!("Active profile for {provider}: {profile}");
            Ok(())
        }

        AuthCommands::SetupToken { provider, profile } => {
            let provider = auth::normalize_provider(&provider)?;
            let token = read_auth_input("Paste token")?;
            if token.is_empty() {
                bail!("Token cannot be empty");
            }

            let kind = auth::anthropic_token::detect_auth_kind(&token, Some("authorization"));
            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                "auth_kind".to_string(),
                kind.as_metadata_value().to_string(),
            );

            auth_service
                .store_provider_token(&provider, &profile, &token, metadata, true)
                .await?;
            println!("Saved profile {profile}");
            println!("Active profile for {provider}: {profile}");
            Ok(())
        }

        AuthCommands::Refresh { provider, profile } => {
            let provider = auth::normalize_provider(&provider)?;

            match provider.as_str() {
                "openai-codex" => {
                    match auth_service
                        .get_valid_openai_access_token(profile.as_deref())
                        .await?
                    {
                        Some(_) => {
                            println!("OpenAI Codex token is valid (refresh completed if needed).");
                            Ok(())
                        }
                        None => {
                            bail!(
                                "No OpenAI Codex auth profile found. Run `zeroclaw auth login --provider openai-codex`."
                            )
                        }
                    }
                }
                "gemini" => {
                    match auth_service
                        .get_valid_gemini_access_token(profile.as_deref())
                        .await?
                    {
                        Some(_) => {
                            let profile_name = profile.as_deref().unwrap_or("default");
                            println!("✓ Gemini token refreshed successfully");
                            println!("  Profile: gemini:{}", profile_name);
                            Ok(())
                        }
                        None => {
                            bail!(
                                "No Gemini auth profile found. Run `zeroclaw auth login --provider gemini`."
                            )
                        }
                    }
                }
                _ => bail!("`auth refresh` supports --provider openai-codex or gemini"),
            }
        }

        AuthCommands::Logout { provider, profile } => {
            let provider = auth::normalize_provider(&provider)?;
            let removed = auth_service.remove_profile(&provider, &profile).await?;
            if removed {
                println!("Removed auth profile {provider}:{profile}");
            } else {
                println!("Auth profile not found: {provider}:{profile}");
            }
            Ok(())
        }

        AuthCommands::Use { provider, profile } => {
            let provider = auth::normalize_provider(&provider)?;
            auth_service.set_active_profile(&provider, &profile).await?;
            println!("Active profile for {provider}: {profile}");
            Ok(())
        }

        AuthCommands::List => {
            let data = auth_service.load_profiles().await?;
            if data.profiles.is_empty() {
                println!("No auth profiles configured.");
                return Ok(());
            }

            for (id, profile) in &data.profiles {
                let active = data
                    .active_profiles
                    .get(&profile.provider)
                    .is_some_and(|active_id| active_id == id);
                let marker = if active { "*" } else { " " };
                println!("{marker} {id}");
            }

            Ok(())
        }

        AuthCommands::Status => {
            let data = auth_service.load_profiles().await?;
            if data.profiles.is_empty() {
                println!("No auth profiles configured.");
                return Ok(());
            }

            for (id, profile) in &data.profiles {
                let active = data
                    .active_profiles
                    .get(&profile.provider)
                    .is_some_and(|active_id| active_id == id);
                let marker = if active { "*" } else { " " };
                println!(
                    "{} {} kind={:?} account={} expires={}",
                    marker,
                    id,
                    profile.kind,
                    crate::security::redact(profile.account_id.as_deref().unwrap_or("unknown")),
                    format_expiry(profile)
                );
            }

            println!();
            println!("Active profiles:");
            for (provider, profile_id) in &data.active_profiles {
                println!("  {provider}: {profile_id}");
            }

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser};

    #[test]
    fn cli_definition_has_no_flag_conflicts() {
        Cli::command().debug_assert();
    }

    #[test]
    fn onboard_help_includes_model_flag() {
        let cmd = Cli::command();
        let onboard = cmd
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == "onboard")
            .expect("onboard subcommand must exist");

        let has_model_flag = onboard
            .get_arguments()
            .any(|arg| arg.get_id().as_str() == "model" && arg.get_long() == Some("model"));

        assert!(
            has_model_flag,
            "onboard help should include --model for quick setup overrides"
        );
    }

    #[test]
    fn onboard_cli_accepts_model_provider_and_api_key_in_quick_mode() {
        let cli = Cli::try_parse_from([
            "zeroclaw",
            "onboard",
            "--provider",
            "openrouter",
            "--model",
            "custom-model-946",
            "--api-key",
            "sk-issue946",
        ])
        .expect("quick onboard invocation should parse");

        match cli.command {
            Commands::Onboard {
                interactive,
                force,
                channels_only,
                api_key,
                provider,
                model,
                ..
            } => {
                assert!(!interactive);
                assert!(!force);
                assert!(!channels_only);
                assert_eq!(provider.as_deref(), Some("openrouter"));
                assert_eq!(model.as_deref(), Some("custom-model-946"));
                assert_eq!(api_key.as_deref(), Some("sk-issue946"));
            }
            other => panic!("expected onboard command, got {other:?}"),
        }
    }

    #[test]
    fn completions_cli_parses_supported_shells() {
        for shell in ["bash", "fish", "zsh", "powershell", "elvish"] {
            let cli = Cli::try_parse_from(["zeroclaw", "completions", shell])
                .expect("completions invocation should parse");
            match cli.command {
                Commands::Completions { .. } => {}
                other => panic!("expected completions command, got {other:?}"),
            }
        }
    }

    #[test]
    fn gateway_help_includes_new_pairing_flag() {
        let cmd = Cli::command();
        let gateway = cmd
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == "gateway")
            .expect("gateway subcommand must exist");

        let has_new_pairing_flag = gateway.get_arguments().any(|arg| {
            arg.get_id().as_str() == "new_pairing" && arg.get_long() == Some("new-pairing")
        });

        assert!(
            has_new_pairing_flag,
            "gateway help should include --new-pairing"
        );
    }

    #[test]
    fn gateway_help_includes_open_dashboard_flag() {
        let cmd = Cli::command();
        let gateway = cmd
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == "gateway")
            .expect("gateway subcommand must exist");

        let has_open_dashboard_flag = gateway.get_arguments().any(|arg| {
            arg.get_id().as_str() == "open_dashboard" && arg.get_long() == Some("open-dashboard")
        });

        assert!(
            has_open_dashboard_flag,
            "gateway help should include --open-dashboard"
        );
    }

    #[test]
    fn gateway_cli_accepts_new_pairing_flag() {
        let cli = Cli::try_parse_from(["zeroclaw", "gateway", "--new-pairing"])
            .expect("gateway --new-pairing should parse");

        match cli.command {
            Commands::Gateway { new_pairing, .. } => assert!(new_pairing),
            other => panic!("expected gateway command, got {other:?}"),
        }
    }

    #[test]
    fn gateway_cli_accepts_open_dashboard_flag() {
        let cli = Cli::try_parse_from(["zeroclaw", "gateway", "--open-dashboard"])
            .expect("gateway --open-dashboard should parse");

        match cli.command {
            Commands::Gateway { open_dashboard, .. } => assert!(open_dashboard),
            other => panic!("expected gateway command, got {other:?}"),
        }
    }

    #[test]
    fn gateway_cli_defaults_flags_to_false() {
        let cli = Cli::try_parse_from(["zeroclaw", "gateway"]).expect("gateway should parse");

        match cli.command {
            Commands::Gateway {
                new_pairing,
                open_dashboard,
                ..
            } => {
                assert!(!new_pairing);
                assert!(!open_dashboard);
            }
            other => panic!("expected gateway command, got {other:?}"),
        }
    }

    #[test]
    fn dashboard_open_url_prefers_loopback_for_wildcard_bind_hosts() {
        assert_eq!(
            dashboard_open_url("0.0.0.0", 42617),
            "http://127.0.0.1:42617/"
        );
        assert_eq!(dashboard_open_url("::", 42617), "http://127.0.0.1:42617/");
        assert_eq!(dashboard_open_url("[::]", 42617), "http://127.0.0.1:42617/");
        assert_eq!(
            dashboard_open_url("127.0.0.1", 42617),
            "http://127.0.0.1:42617/"
        );
    }

    #[test]
    fn completion_generation_mentions_binary_name() {
        let mut output = Vec::new();
        write_shell_completion(CompletionShell::Bash, &mut output)
            .expect("completion generation should succeed");
        let script = String::from_utf8(output).expect("completion output should be valid utf-8");
        assert!(
            script.contains("zeroclaw"),
            "completion script should reference binary name"
        );
    }

    #[test]
    fn onboard_cli_accepts_force_flag() {
        let cli = Cli::try_parse_from(["zeroclaw", "onboard", "--force"])
            .expect("onboard --force should parse");

        match cli.command {
            Commands::Onboard { force, .. } => assert!(force),
            other => panic!("expected onboard command, got {other:?}"),
        }
    }

    #[test]
    fn onboard_cli_accepts_interactive_ui_flag() {
        let cli = Cli::try_parse_from(["zeroclaw", "onboard", "--interactive-ui"])
            .expect("onboard --interactive-ui should parse");

        match cli.command {
            Commands::Onboard {
                interactive,
                interactive_ui,
                ..
            } => {
                assert!(!interactive);
                assert!(interactive_ui);
            }
            other => panic!("expected onboard command, got {other:?}"),
        }
    }

    #[test]
    fn onboard_cli_accepts_no_totp_flag() {
        let cli = Cli::try_parse_from(["zeroclaw", "onboard", "--no-totp"])
            .expect("onboard --no-totp should parse");

        match cli.command {
            Commands::Onboard { no_totp, .. } => assert!(no_totp),
            other => panic!("expected onboard command, got {other:?}"),
        }
    }

    #[test]
    fn onboard_cli_accepts_openclaw_migration_flags() {
        let cli = Cli::try_parse_from([
            "zeroclaw",
            "onboard",
            "--migrate-openclaw",
            "--openclaw-source",
            "/tmp/openclaw-workspace",
            "--openclaw-config",
            "/tmp/openclaw.json",
        ])
        .expect("onboard openclaw migration flags should parse");

        match cli.command {
            Commands::Onboard {
                migrate_openclaw,
                openclaw_source,
                openclaw_config,
                ..
            } => {
                assert!(migrate_openclaw);
                assert_eq!(
                    openclaw_source.as_deref(),
                    Some(std::path::Path::new("/tmp/openclaw-workspace"))
                );
                assert_eq!(
                    openclaw_config.as_deref(),
                    Some(std::path::Path::new("/tmp/openclaw.json"))
                );
            }
            other => panic!("expected onboard command, got {other:?}"),
        }
    }

    #[test]
    fn migrate_openclaw_cli_accepts_source_and_module_flags() {
        let cli = Cli::try_parse_from([
            "zeroclaw",
            "migrate",
            "openclaw",
            "--source",
            "/tmp/openclaw-workspace",
            "--source-config",
            "/tmp/openclaw.json",
            "--dry-run",
            "--no-config",
        ])
        .expect("migrate openclaw flags should parse");

        match cli.command {
            Commands::Migrate {
                migrate_command:
                    MigrateCommands::Openclaw {
                        source,
                        source_config,
                        dry_run,
                        no_memory,
                        no_config,
                    },
            } => {
                assert_eq!(
                    source.as_deref(),
                    Some(std::path::Path::new("/tmp/openclaw-workspace"))
                );
                assert_eq!(
                    source_config.as_deref(),
                    Some(std::path::Path::new("/tmp/openclaw.json"))
                );
                assert!(dry_run);
                assert!(!no_memory);
                assert!(no_config);
            }
            other => panic!("expected migrate openclaw command, got {other:?}"),
        }
    }

    #[test]
    fn cli_parses_estop_default_engage() {
        let cli = Cli::try_parse_from(["zeroclaw", "estop"]).expect("estop command should parse");

        match cli.command {
            Commands::Estop {
                estop_command,
                level,
                domains,
                tools,
            } => {
                assert!(estop_command.is_none());
                assert!(level.is_none());
                assert!(domains.is_empty());
                assert!(tools.is_empty());
            }
            other => panic!("expected estop command, got {other:?}"),
        }
    }

    #[test]
    fn cli_parses_estop_resume_domain() {
        let cli = Cli::try_parse_from(["zeroclaw", "estop", "resume", "--domain", "*.chase.com"])
            .expect("estop resume command should parse");

        match cli.command {
            Commands::Estop {
                estop_command: Some(EstopSubcommands::Resume { domains, .. }),
                ..
            } => assert_eq!(domains, vec!["*.chase.com".to_string()]),
            other => panic!("expected estop resume command, got {other:?}"),
        }
    }

    #[test]
    fn config_help_mentions_show_get_set_examples() {
        let cmd = Cli::command();
        let config_cmd = cmd
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == "config")
            .expect("config subcommand must exist");

        let mut output = Vec::new();
        config_cmd
            .clone()
            .write_long_help(&mut output)
            .expect("help generation should succeed");
        let help = String::from_utf8(output).expect("help output should be utf-8");
        assert!(help.contains("zeroclaw config show"));
        assert!(help.contains("zeroclaw config get gateway.port"));
        assert!(help.contains("zeroclaw config set gateway.port 8080"));
    }

    #[test]
    fn config_cli_parses_show_get_set_subcommands() {
        let show =
            Cli::try_parse_from(["zeroclaw", "config", "show"]).expect("config show should parse");
        match show.command {
            Commands::Config {
                config_command: ConfigCommands::Show,
            } => {}
            other => panic!("expected config show, got {other:?}"),
        }

        let get = Cli::try_parse_from(["zeroclaw", "config", "get", "gateway.port"])
            .expect("config get should parse");
        match get.command {
            Commands::Config {
                config_command: ConfigCommands::Get { key },
            } => assert_eq!(key, "gateway.port"),
            other => panic!("expected config get, got {other:?}"),
        }

        let set = Cli::try_parse_from(["zeroclaw", "config", "set", "gateway.port", "8080"])
            .expect("config set should parse");
        match set.command {
            Commands::Config {
                config_command: ConfigCommands::Set { key, value },
            } => {
                assert_eq!(key, "gateway.port");
                assert_eq!(value, "8080");
            }
            other => panic!("expected config set, got {other:?}"),
        }
    }

    #[test]
    fn redact_config_secrets_masks_nested_sensitive_values() {
        let mut payload = serde_json::json!({
            "api_key": "sk-test",
            "nested": {
                "bot_token": "token",
                "paired_tokens": ["abc", "def"],
                "non_secret": "ok"
            }
        });
        redact_config_secrets(&mut payload);

        assert_eq!(payload["api_key"], serde_json::json!("***REDACTED***"));
        assert_eq!(
            payload["nested"]["bot_token"],
            serde_json::json!("***REDACTED***")
        );
        assert_eq!(
            payload["nested"]["paired_tokens"],
            serde_json::json!(["***REDACTED***"])
        );
        assert_eq!(payload["nested"]["non_secret"], serde_json::json!("ok"));
    }

    #[test]
    fn update_help_mentions_instructions_flag() {
        let cmd = Cli::command();
        let update_cmd = cmd
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == "update")
            .expect("update subcommand must exist");

        let mut output = Vec::new();
        update_cmd
            .clone()
            .write_long_help(&mut output)
            .expect("help generation should succeed");
        let help = String::from_utf8(output).expect("help output should be utf-8");

        assert!(help.contains("--instructions"));
    }

    #[test]
    fn update_cli_parses_instructions_flag() {
        let cli = Cli::try_parse_from(["zeroclaw", "update", "--instructions"])
            .expect("update --instructions should parse");

        match cli.command {
            Commands::Update {
                check,
                force,
                instructions,
            } => {
                assert!(!check);
                assert!(!force);
                assert!(instructions);
            }
            other => panic!("expected update command, got {other:?}"),
        }
    }
}
