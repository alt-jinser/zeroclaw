use clap::Subcommand;
use zeroclaw::{doctor, Config};

#[derive(Subcommand, Debug)]
pub(crate) enum DoctorCommands {
    /// Probe model catalogs across providers and report availability
    Models {
        /// Probe a specific provider only (default: all known providers)
        #[arg(long)]
        provider: Option<String>,

        /// Prefer cached catalogs when available (skip forced live refresh)
        #[arg(long)]
        use_cache: bool,
    },
    /// Query runtime trace events (tool diagnostics and model replies)
    Traces {
        /// Show a specific trace event by id
        #[arg(long)]
        id: Option<String>,
        /// Filter list output by event type
        #[arg(long)]
        event: Option<String>,
        /// Case-insensitive text match across message/payload
        #[arg(long)]
        contains: Option<String>,
        /// Maximum number of events to display
        #[arg(long, default_value = "20")]
        limit: usize,
    },
}

#[derive(clap::Args, Debug)]
pub(crate) struct DoctorArgs {
    #[command(subcommand)]
    doctor_command: Option<DoctorCommands>,
}

pub(crate) async fn run(args: DoctorArgs, config: Config) -> anyhow::Result<()> {
    let DoctorArgs { doctor_command } = args;
    match doctor_command {
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
    }
}
