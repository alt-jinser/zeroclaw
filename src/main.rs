#![forbid(unsafe_code)]
#![recursion_limit = "256"]

use anyhow::{bail, Result};
use clap::Parser as _;
use tracing_subscriber::{fmt, EnvFilter};

mod cmd;

// Re-export so binary modules can use crate::<CommandEnum> while keeping a single source of truth.
pub use zeroclaw::{HardwareCommands, MigrateCommands, PeripheralCommands, SkillCommands};

use crate::cmd::Cli;

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

    // Initialize logging - respects RUST_LOG env var, defaults to INFO
    let subscriber = fmt::Subscriber::builder()
        .with_ansi(true)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    cmd::run(cli.command).await
}

// ─── Generic Pending OAuth Login ────────────────────────────────────────────

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

    // #[test]
    // fn onboard_cli_accepts_model_provider_and_api_key_in_quick_mode() {
    //     let cli = Cli::try_parse_from([
    //         "zeroclaw",
    //         "onboard",
    //         "--provider",
    //         "openrouter",
    //         "--model",
    //         "custom-model-946",
    //         "--api-key",
    //         "sk-issue946",
    //     ])
    //     .expect("quick onboard invocation should parse");
    //
    //     match cli.command {
    //         Command::Onboard {
    //             interactive,
    //             force,
    //             channels_only,
    //             api_key,
    //             provider,
    //             model,
    //             ..
    //         } => {
    //             assert!(!interactive);
    //             assert!(!force);
    //             assert!(!channels_only);
    //             assert_eq!(provider.as_deref(), Some("openrouter"));
    //             assert_eq!(model.as_deref(), Some("custom-model-946"));
    //             assert_eq!(api_key.as_deref(), Some("sk-issue946"));
    //         }
    //         other => panic!("expected onboard command, got {other:?}"),
    //     }
    // }

    #[test]
    fn completions_cli_parses_supported_shells() {
        for shell in ["bash", "fish", "zsh", "powershell", "elvish"] {
            let cli = Cli::try_parse_from(["zeroclaw", "completions", shell])
                .expect("completions invocation should parse");
            match cli.command {
                Command::Completions { .. } => {}
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

    // #[test]
    // fn gateway_cli_accepts_new_pairing_flag() {
    //     let cli = Cli::try_parse_from(["zeroclaw", "gateway", "--new-pairing"])
    //         .expect("gateway --new-pairing should parse");
    //
    //     match cli.command {
    //         Command::Gateway { new_pairing, .. } => assert!(new_pairing),
    //         other => panic!("expected gateway command, got {other:?}"),
    //     }
    // }

    // #[test]
    // fn gateway_cli_accepts_open_dashboard_flag() {
    //     let cli = Cli::try_parse_from(["zeroclaw", "gateway", "--open-dashboard"])
    //         .expect("gateway --open-dashboard should parse");
    //
    //     match cli.command {
    //         Command::Gateway { open_dashboard, .. } => assert!(open_dashboard),
    //         other => panic!("expected gateway command, got {other:?}"),
    //     }
    // }

    // #[test]
    // fn gateway_cli_defaults_flags_to_false() {
    //     let cli = Cli::try_parse_from(["zeroclaw", "gateway"]).expect("gateway should parse");
    //
    //     match cli.command {
    //         Command::Gateway {
    //             new_pairing,
    //             open_dashboard,
    //             ..
    //         } => {
    //             assert!(!new_pairing);
    //             assert!(!open_dashboard);
    //         }
    //         other => panic!("expected gateway command, got {other:?}"),
    //     }
    // }

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

    // #[test]
    // fn onboard_cli_accepts_force_flag() {
    //     let cli = Cli::try_parse_from(["zeroclaw", "onboard", "--force"])
    //         .expect("onboard --force should parse");
    //
    //     match cli.command {
    //         Command::Onboard { force, .. } => assert!(force),
    //         other => panic!("expected onboard command, got {other:?}"),
    //     }
    // }
    //
    // #[test]
    // fn onboard_cli_accepts_interactive_ui_flag() {
    //     let cli = Cli::try_parse_from(["zeroclaw", "onboard", "--interactive-ui"])
    //         .expect("onboard --interactive-ui should parse");
    //
    //     match cli.command {
    //         Command::Onboard {
    //             interactive,
    //             interactive_ui,
    //             ..
    //         } => {
    //             assert!(!interactive);
    //             assert!(interactive_ui);
    //         }
    //         other => panic!("expected onboard command, got {other:?}"),
    //     }
    // }
    //
    // #[test]
    // fn onboard_cli_accepts_no_totp_flag() {
    //     let cli = Cli::try_parse_from(["zeroclaw", "onboard", "--no-totp"])
    //         .expect("onboard --no-totp should parse");
    //
    //     match cli.command {
    //         Command::Onboard { no_totp, .. } => assert!(no_totp),
    //         other => panic!("expected onboard command, got {other:?}"),
    //     }
    // }
    //
    // #[test]
    // fn onboard_cli_accepts_openclaw_migration_flags() {
    //     let cli = Cli::try_parse_from([
    //         "zeroclaw",
    //         "onboard",
    //         "--migrate-openclaw",
    //         "--openclaw-source",
    //         "/tmp/openclaw-workspace",
    //         "--openclaw-config",
    //         "/tmp/openclaw.json",
    //     ])
    //     .expect("onboard openclaw migration flags should parse");
    //
    //     match cli.command {
    //         Command::Onboard {
    //             migrate_openclaw,
    //             openclaw_source,
    //             openclaw_config,
    //             ..
    //         } => {
    //             assert!(migrate_openclaw);
    //             assert_eq!(
    //                 openclaw_source.as_deref(),
    //                 Some(std::path::Path::new("/tmp/openclaw-workspace"))
    //             );
    //             assert_eq!(
    //                 openclaw_config.as_deref(),
    //                 Some(std::path::Path::new("/tmp/openclaw.json"))
    //             );
    //         }
    //         other => panic!("expected onboard command, got {other:?}"),
    //     }
    // }
    //
    // #[test]
    // fn migrate_openclaw_cli_accepts_source_and_module_flags() {
    //     let cli = Cli::try_parse_from([
    //         "zeroclaw",
    //         "migrate",
    //         "openclaw",
    //         "--source",
    //         "/tmp/openclaw-workspace",
    //         "--source-config",
    //         "/tmp/openclaw.json",
    //         "--dry-run",
    //         "--no-config",
    //     ])
    //     .expect("migrate openclaw flags should parse");
    //
    //     match cli.command {
    //         Command::Migrate {
    //             migrate_command:
    //                 MigrateCommands::Openclaw {
    //                     source,
    //                     source_config,
    //                     dry_run,
    //                     no_memory,
    //                     no_config,
    //                 },
    //         } => {
    //             assert_eq!(
    //                 source.as_deref(),
    //                 Some(std::path::Path::new("/tmp/openclaw-workspace"))
    //             );
    //             assert_eq!(
    //                 source_config.as_deref(),
    //                 Some(std::path::Path::new("/tmp/openclaw.json"))
    //             );
    //             assert!(dry_run);
    //             assert!(!no_memory);
    //             assert!(no_config);
    //         }
    //         other => panic!("expected migrate openclaw command, got {other:?}"),
    //     }
    // }
    //
    // #[test]
    // fn cli_parses_estop_default_engage() {
    //     let cli = Cli::try_parse_from(["zeroclaw", "estop"]).expect("estop command should parse");
    //
    //     match cli.command {
    //         Command::Estop {
    //             estop_command,
    //             level,
    //             domains,
    //             tools,
    //         } => {
    //             assert!(estop_command.is_none());
    //             assert!(level.is_none());
    //             assert!(domains.is_empty());
    //             assert!(tools.is_empty());
    //         }
    //         other => panic!("expected estop command, got {other:?}"),
    //     }
    // }
    //
    // #[test]
    // fn cli_parses_estop_resume_domain() {
    //     let cli = Cli::try_parse_from(["zeroclaw", "estop", "resume", "--domain", "*.chase.com"])
    //         .expect("estop resume command should parse");
    //
    //     match cli.command {
    //         Command::Estop {
    //             estop_command: Some(EstopSubcommands::Resume { domains, .. }),
    //             ..
    //         } => assert_eq!(domains, vec!["*.chase.com".to_string()]),
    //         other => panic!("expected estop resume command, got {other:?}"),
    //     }
    // }
    //
    // #[test]
    // fn config_help_mentions_show_get_set_examples() {
    //     let cmd = Cli::command();
    //     let config_cmd = cmd
    //         .get_subcommands()
    //         .find(|subcommand| subcommand.get_name() == "config")
    //         .expect("config subcommand must exist");
    //
    //     let mut output = Vec::new();
    //     config_cmd
    //         .clone()
    //         .write_long_help(&mut output)
    //         .expect("help generation should succeed");
    //     let help = String::from_utf8(output).expect("help output should be utf-8");
    //     assert!(help.contains("zeroclaw config show"));
    //     assert!(help.contains("zeroclaw config get gateway.port"));
    //     assert!(help.contains("zeroclaw config set gateway.port 8080"));
    // }
    //
    // #[test]
    // fn config_cli_parses_show_get_set_subcommands() {
    //     let show =
    //         Cli::try_parse_from(["zeroclaw", "config", "show"]).expect("config show should parse");
    //     match show.command {
    //         Command::Config {
    //             config_command: ConfigCommands::Show,
    //         } => {}
    //         other => panic!("expected config show, got {other:?}"),
    //     }
    //
    //     let get = Cli::try_parse_from(["zeroclaw", "config", "get", "gateway.port"])
    //         .expect("config get should parse");
    //     match get.command {
    //         Command::Config {
    //             config_command: ConfigCommands::Get { key },
    //         } => assert_eq!(key, "gateway.port"),
    //         other => panic!("expected config get, got {other:?}"),
    //     }
    //
    //     let set = Cli::try_parse_from(["zeroclaw", "config", "set", "gateway.port", "8080"])
    //         .expect("config set should parse");
    //     match set.command {
    //         Command::Config {
    //             config_command: ConfigCommands::Set { key, value },
    //         } => {
    //             assert_eq!(key, "gateway.port");
    //             assert_eq!(value, "8080");
    //         }
    //         other => panic!("expected config set, got {other:?}"),
    //     }
    // }
    //
    // #[test]
    // fn redact_config_secrets_masks_nested_sensitive_values() {
    //     let mut payload = serde_json::json!({
    //         "api_key": "sk-test",
    //         "nested": {
    //             "bot_token": "token",
    //             "paired_tokens": ["abc", "def"],
    //             "non_secret": "ok"
    //         }
    //     });
    //     redact_config_secrets(&mut payload);
    //
    //     assert_eq!(payload["api_key"], serde_json::json!("***REDACTED***"));
    //     assert_eq!(
    //         payload["nested"]["bot_token"],
    //         serde_json::json!("***REDACTED***")
    //     );
    //     assert_eq!(
    //         payload["nested"]["paired_tokens"],
    //         serde_json::json!(["***REDACTED***"])
    //     );
    //     assert_eq!(payload["nested"]["non_secret"], serde_json::json!("ok"));
    // }
    //
    // #[test]
    // fn update_help_mentions_instructions_flag() {
    //     let cmd = Cli::command();
    //     let update_cmd = cmd
    //         .get_subcommands()
    //         .find(|subcommand| subcommand.get_name() == "update")
    //         .expect("update subcommand must exist");
    //
    //     let mut output = Vec::new();
    //     update_cmd
    //         .clone()
    //         .write_long_help(&mut output)
    //         .expect("help generation should succeed");
    //     let help = String::from_utf8(output).expect("help output should be utf-8");
    //
    //     assert!(help.contains("--instructions"));
    // }
    //
    // #[test]
    // fn update_cli_parses_instructions_flag() {
    //     let cli = Cli::try_parse_from(["zeroclaw", "update", "--instructions"])
    //         .expect("update --instructions should parse");
    //
    //     match cli.command {
    //         Command::Update {
    //             check,
    //             force,
    //             instructions,
    //         } => {
    //             assert!(!check);
    //             assert!(!force);
    //             assert!(instructions);
    //         }
    //         other => panic!("expected update command, got {other:?}"),
    //     }
    // }
}
