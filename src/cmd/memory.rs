use zeroclaw::{memory, Config, MemoryCommands};

#[derive(clap::Args, Debug)]
pub(crate) struct MemoryArgs {
    #[command(subcommand)]
    memory_command: MemoryCommands,
}

pub(crate) async fn run(args: MemoryArgs, config: &Config) -> anyhow::Result<()> {
    memory::cli::handle_command(args.memory_command, config).await
}
