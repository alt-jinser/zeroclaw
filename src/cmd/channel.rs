use clap::Subcommand;
use serde::{Deserialize, Serialize};
use zeroclaw::{
    channels::{self, bind_telegram_identity},
    Config,
};

#[derive(clap::Args, Debug)]
pub(crate) struct ChannelArgs {
    #[command(subcommand)]
    channel_command: ChannelCommands,
}

/// Channel management subcommands
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChannelCommands {
    /// List all configured channels
    List,
    /// Start all configured channels (handled in main.rs for async)
    Start,
    /// Run health checks for configured channels (handled in main.rs for async)
    Doctor,
    /// Add a new channel configuration
    #[command(long_about = "\
Add a new channel configuration.

Provide the channel type and a JSON object with the required \
configuration keys for that channel type.

Supported types: telegram, discord, slack, whatsapp, github, matrix, imessage, email.

Examples:
  zeroclaw channel add telegram '{\"bot_token\":\"...\",\"name\":\"my-bot\"}'
  zeroclaw channel add discord '{\"bot_token\":\"...\",\"name\":\"my-discord\"}'")]
    Add {
        /// Channel type (telegram, discord, slack, whatsapp, github, matrix, imessage, email)
        channel_type: String,
        /// Optional configuration as JSON
        config: String,
    },
    /// Remove a channel configuration
    Remove {
        /// Channel name to remove
        name: String,
    },
    /// Bind a Telegram identity (username or numeric user ID) into allowlist
    #[command(long_about = "\
Bind a Telegram identity into the allowlist.

Adds a Telegram username (without the '@' prefix) or numeric user \
ID to the channel allowlist so the agent will respond to messages \
from that identity.

Examples:
  zeroclaw channel bind-telegram zeroclaw_user
  zeroclaw channel bind-telegram 123456789")]
    BindTelegram {
        /// Telegram identity to allow (username without '@' or numeric user ID)
        identity: String,
    },
}

pub(crate) async fn run(args: ChannelArgs, config: Config) -> anyhow::Result<()> {
    let var_name = match args.channel_command {
        ChannelCommands::Start => Box::pin(channels::start_channels(config)).await,
        ChannelCommands::Doctor => Box::pin(channels::doctor_channels(config)).await,
        cmd => match cmd {
            ChannelCommands::Start => {
                anyhow::bail!("Start must be handled in main.rs (requires async runtime)")
            }
            ChannelCommands::Doctor => {
                anyhow::bail!("Doctor must be handled in main.rs (requires async runtime)")
            }
            ChannelCommands::List => {
                println!("Channels:");
                println!("  ✅ CLI (always available)");
                for (channel, configured) in config.channels_config.channels() {
                    println!(
                        "  {} {}",
                        if configured { "✅" } else { "❌" },
                        channel.name()
                    );
                }
                if !cfg!(feature = "channel-matrix") {
                    println!(
                    "  ℹ️ Matrix channel support is disabled in this build (enable `channel-matrix`)."
                );
                }
                if !cfg!(feature = "channel-lark") {
                    println!(
                    "  ℹ️ Lark/Feishu channel support is disabled in this build (enable `channel-lark`)."
                );
                }
                println!("\nTo start channels: zeroclaw channel start");
                println!("To check health:    zeroclaw channel doctor");
                println!("To configure:      zeroclaw onboard");
                Ok(())
            }
            ChannelCommands::Add {
                channel_type,
                config: _,
            } => {
                anyhow::bail!(
                    "Channel type '{channel_type}' — use `zeroclaw onboard` to configure channels"
                );
            }
            ChannelCommands::Remove { name } => {
                anyhow::bail!("Remove channel '{name}' — edit ~/.zeroclaw/config.toml directly");
            }
            ChannelCommands::BindTelegram { identity } => {
                bind_telegram_identity(&config, &identity).await
            }
        },
    };
    var_name
}
