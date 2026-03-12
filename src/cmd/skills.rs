use zeroclaw::{skills, Config, SkillCommands};

#[derive(clap::Args, Debug)]
pub(crate) struct SkillsArgs {
    #[command(subcommand)]
    skill_command: SkillCommands,
}

pub(crate) fn run(args: SkillsArgs, config: &Config) -> anyhow::Result<()> {
    skills::handle_command(args.skill_command, config)
}
