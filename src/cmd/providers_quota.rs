use clap::ValueEnum;
use zeroclaw::{providers, Config};

#[derive(Debug, Clone, ValueEnum)]
enum QuotaFormat {
    Text,
    Json,
}

#[derive(clap::Args, Debug)]
pub(crate) struct ProvidersQuotaArgs {
    /// Filter by provider name (optional, shows all if omitted)
    #[arg(long)]
    provider: Option<String>,

    /// Output format (text or json)
    #[arg(long, value_enum, default_value_t = QuotaFormat::Text)]
    format: QuotaFormat,
}

pub(crate) async fn run(args: ProvidersQuotaArgs, config: Config) -> anyhow::Result<()> {
    let ProvidersQuotaArgs { provider, format } = args;
    let format_str = match format {
        QuotaFormat::Text => "text",
        QuotaFormat::Json => "json",
    };
    providers::quota_cli::run(&config, provider.as_deref(), format_str).await
}
