use anyhow::{bail, Context as _};
use clap::Subcommand;
use zeroclaw::{config, Config};
///
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

#[derive(clap::Args, Debug)]
pub(crate) struct ConfigArgs {
    #[command(subcommand)]
    config_command: ConfigCommands,
}

pub(crate) async fn run(args: ConfigArgs, mut config: Config) -> anyhow::Result<()> {
    match args.config_command {
        ConfigCommands::Show => {
            let mut json = serde_json::to_value(&config).context("Failed to serialize config")?;
            redact_config_secrets(&mut json);
            println!("{}", serde_json::to_string_pretty(&json)?);
            Ok(())
        }
        ConfigCommands::Get { key } => {
            let mut json = serde_json::to_value(&config).context("Failed to serialize config")?;
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
            let mut json = serde_json::to_value(&config).context("Failed to serialize config")?;

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
    }
}
