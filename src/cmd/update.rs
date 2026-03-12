use zeroclaw::update;

#[derive(clap::Args, Debug)]
pub(crate) struct UpdateArgs {
    /// Check for updates without installing
    #[arg(long, conflicts_with_all = ["force", "instructions"])]
    check: bool,

    /// Force update even if already at latest version
    #[arg(long, conflicts_with = "instructions")]
    force: bool,

    /// Show human-friendly update instructions for your installation method
    #[arg(long, conflicts_with_all = ["check", "force"])]
    instructions: bool,
}

pub(crate) async fn run(args: UpdateArgs) -> anyhow::Result<()> {
    let UpdateArgs {
        check,
        force,
        instructions,
    } = args;
    if instructions {
        update::print_update_instructions()?;
    } else {
        update::self_update(force, check).await?;
    }
    Ok(())
}
