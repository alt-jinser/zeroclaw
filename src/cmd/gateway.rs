use anyhow::bail;
use tracing::{info, warn};
use zeroclaw::{gateway, Config};

fn dashboard_open_url(host: &str, port: u16) -> String {
    let bind_host = host.trim();
    let browser_host = match bind_host {
        "0.0.0.0" | "::" | "[::]" => "127.0.0.1",
        _ => bind_host,
    };
    format!("http://{browser_host}:{port}/")
}

async fn open_url_in_default_browser(url: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = tokio::process::Command::new("open");
        command.arg(url);
        command
    };

    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = tokio::process::Command::new("xdg-open");
        command.arg(url);
        command
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = tokio::process::Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    };

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = url;
        bail!("automatic dashboard open is unsupported on this platform");
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        let status = command.status().await?;
        if status.success() {
            Ok(())
        } else {
            bail!("browser launcher exited with status {status}");
        }
    }
}

#[derive(clap::Args, Debug)]
pub(crate) struct GatewayArgs {
    /// Port to listen on (use 0 for random available port); defaults to config gateway.port
    #[arg(short, long)]
    port: Option<u16>,

    /// Host to bind to; defaults to config gateway.host
    #[arg(long)]
    host: Option<String>,

    /// Clear all paired tokens and generate a fresh pairing code
    #[arg(long)]
    new_pairing: bool,

    /// Open the web dashboard URL in the default browser on startup
    #[arg(long)]
    open_dashboard: bool,
}

pub(crate) async fn run(args: GatewayArgs, mut config: Config) -> anyhow::Result<()> {
    let GatewayArgs {
        port,
        host,
        new_pairing,
        open_dashboard,
    } = args;

    if new_pairing {
        // Persist token reset from raw config so env-derived overrides are not written to disk.
        let mut persisted_config = Config::load_or_init().await?;
        persisted_config.gateway.paired_tokens.clear();
        persisted_config.save().await?;
        config.gateway.paired_tokens.clear();
        info!("🔐 Cleared paired tokens — a fresh pairing code will be generated");
    }
    let port = port.unwrap_or(config.gateway.port);
    let host = host.unwrap_or_else(|| config.gateway.host.clone());
    if port == 0 {
        info!("🚀 Starting ZeroClaw Gateway on {host} (random port)");
    } else {
        info!("🚀 Starting ZeroClaw Gateway on {host}:{port}");
    }
    if open_dashboard {
        if port == 0 {
            warn!(
                        "--open-dashboard requires a fixed port; skipping auto-open because --port 0 uses a random port"
                    );
        } else {
            let dashboard_url = dashboard_open_url(&host, port);
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(750)).await;
                if let Err(err) = open_url_in_default_browser(&dashboard_url).await {
                    warn!(
                                "Could not open dashboard automatically ({err}). Open manually: {dashboard_url}"
                            );
                } else {
                    info!("🌐 Opened dashboard in browser: {dashboard_url}");
                }
            });
        }
    }
    gateway::run_gateway(&host, port, config).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_open_url_prefers_loopback_for_wildcard_bind_hosts() {
        assert_eq!(
            dashboard_open_url("0.0.0.0", 42617),
            "http://127.0.0.1:42617/"
        );
        assert_eq!(dashboard_open_url("::", 42617), "http://127.0.0.1:42617/");
        assert_eq!(dashboard_open_url("[::]", 42617), "http://127.0.0.1:42617/");
        assert_eq!(
            dashboard_open_url("127.0.0.1", 42617),
            "http://127.0.0.1:42617/"
        );
    }
}
