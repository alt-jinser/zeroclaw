use tracing::info;
use zeroclaw::{daemon, Config};

#[derive(clap::Args, Debug)]
pub(crate) struct DaemonArgs {
    /// Port to listen on (use 0 for random available port); defaults to config gateway.port
    #[arg(short, long)]
    port: Option<u16>,

    /// Host to bind to; defaults to config gateway.host
    #[arg(long)]
    host: Option<String>,
}

pub(crate) async fn run(args: DaemonArgs, config: Config) -> anyhow::Result<()> {
    let DaemonArgs { port, host } = args;
    let port = port.unwrap_or(config.gateway.port);
    let host = host.unwrap_or_else(|| config.gateway.host.clone());
    if port == 0 {
        info!("🧠 Starting ZeroClaw Daemon on {host} (random port)");
    } else {
        info!("🧠 Starting ZeroClaw Daemon on {host}:{port}");
    }
    daemon::run(config, host, port).await
}
