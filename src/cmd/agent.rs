use zeroclaw::{agent, security, Config};

fn parse_temperature(s: &str) -> std::result::Result<f64, String> {
    let t: f64 = s.parse().map_err(|e| format!("{e}"))?;
    if !(0.0..=2.0).contains(&t) {
        return Err("temperature must be between 0.0 and 2.0".to_string());
    }
    Ok(t)
}

#[derive(clap::Args, Debug)]
pub(crate) struct AgentArgs {
    /// Single message mode (don't enter interactive mode)
    #[arg(short, long)]
    message: Option<String>,

    /// Provider to use (openrouter, anthropic, openai, openai-codex)
    #[arg(short, long)]
    provider: Option<String>,

    /// Model to use
    #[arg(long)]
    model: Option<String>,

    /// Temperature (0.0 - 2.0)
    #[arg(short, long, default_value = "0.7", value_parser = parse_temperature)]
    temperature: f64,

    /// Attach a peripheral (board:path, e.g. nucleo-f401re:/dev/ttyACM0)
    #[arg(long)]
    peripheral: Vec<String>,

    /// Autonomy level (read_only, supervised, full)
    #[arg(long, value_parser = clap::value_parser!(security::AutonomyLevel))]
    autonomy_level: Option<security::AutonomyLevel>,

    /// Maximum shell/tool actions per hour
    #[arg(long)]
    max_actions_per_hour: Option<u32>,

    /// Maximum tool-call iterations per message
    #[arg(long)]
    max_tool_iterations: Option<usize>,

    /// Maximum conversation history messages
    #[arg(long)]
    max_history_messages: Option<usize>,

    /// Enable compact context mode (smaller prompts for limited models)
    #[arg(long)]
    compact_context: bool,

    /// Memory backend (sqlite, markdown, none)
    #[arg(long)]
    memory_backend: Option<String>,
}

pub(crate) async fn run(args: AgentArgs, mut config: Config) -> anyhow::Result<()> {
    let AgentArgs {
        message,
        provider,
        model,
        temperature,
        peripheral,
        autonomy_level,
        max_actions_per_hour,
        max_tool_iterations,
        max_history_messages,
        compact_context,
        memory_backend,
    } = args;

    if let Some(level) = autonomy_level {
        config.autonomy.level = level;
    }
    if let Some(n) = max_actions_per_hour {
        config.autonomy.max_actions_per_hour = n;
    }
    if let Some(n) = max_tool_iterations {
        config.agent.max_tool_iterations = n;
    }
    if let Some(n) = max_history_messages {
        config.agent.max_history_messages = n;
    }
    if compact_context {
        config.agent.compact_context = true;
    }
    if let Some(ref backend) = memory_backend {
        config.memory.backend = backend.clone();
    }
    // interactive=true only when no --message flag (real REPL session).
    // Single-shot mode (-m) runs non-interactively: no TTY approval prompt,
    // so tools are not denied by a stdin read returning EOF.
    let interactive = message.is_none();
    Box::pin(agent::run(
        config,
        message,
        provider,
        model,
        temperature,
        peripheral,
        interactive,
        None,
    ))
    .await
    .map(|_| ())
}
