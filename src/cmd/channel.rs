use zeroclaw::{channels, ChannelCommands, Config};

#[derive(clap::Args, Debug)]
pub(crate) struct ChannelArgs {
    #[command(subcommand)]
    channel_command: ChannelCommands,
}

pub(crate) async fn run(args: ChannelArgs, config: Config) -> anyhow::Result<()> {
    let var_name = match args.channel_command {
        ChannelCommands::Start => Box::pin(channels::start_channels(config)).await,
        ChannelCommands::Doctor => Box::pin(channels::doctor_channels(config)).await,
        other => channels::handle_command(other, &config).await,
    };
    var_name
}
