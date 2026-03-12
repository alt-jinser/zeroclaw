use anyhow::bail;
use zeroclaw::{channels, onboard};

#[derive(clap::Args, Debug)]
pub(crate) struct OnboardArgs {
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
}

/// Onboard runs quick setup by default, interactive wizard with --interactive,
/// or full-screen TUI with --interactive-ui.
/// The onboard wizard uses reqwest::blocking internally, which creates its own
/// Tokio runtime. To avoid "Cannot drop a runtime in a context where blocking is
/// not allowed", we run the wizard on a blocking thread via spawn_blocking.
pub(crate) async fn run(args: OnboardArgs) -> anyhow::Result<()> {
    let OnboardArgs {
        interactive,
        interactive_ui,
        force,
        channels_only,
        api_key,
        provider,
        model,
        memory,
        no_totp,
        migrate_openclaw,
        openclaw_source,
        openclaw_config,
    } = args;
    {
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
    }
}
