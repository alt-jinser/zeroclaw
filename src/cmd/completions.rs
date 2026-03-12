use std::io::Write;

use clap::CommandFactory as _;
use clap_complete::Shell;

use crate::cmd::Cli;

#[derive(clap::Args, Debug)]
pub(crate) struct CompletionsArgs {
    /// Target shell
    #[arg(value_enum)]
    shell: Shell,
}

fn write_shell_completion<W: Write>(generator: Shell, writer: &mut W) -> anyhow::Result<()> {
    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_owned();

    clap_complete::generate(generator, &mut cmd, bin_name, writer);

    writer.flush()?;
    Ok(())
}

// Completions must remain stdout-only and should not load config or initialize logging.
// This avoids warnings/log lines corrupting sourced completion scripts.
pub(crate) fn run(args: CompletionsArgs) -> anyhow::Result<()> {
    let mut stdout = std::io::stdout().lock();
    write_shell_completion(args.shell, &mut stdout)?;
    Ok(())
}
