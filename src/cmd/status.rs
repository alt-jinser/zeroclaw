use zeroclaw::{memory, Config, ZEROCLAW_BUILD_VERSION};

pub(crate) fn run(config: Config) -> anyhow::Result<()> {
    println!("🦀 ZeroClaw Status");
    println!();
    println!("Version:     {}", ZEROCLAW_BUILD_VERSION);
    println!("Workspace:   {}", config.workspace_dir.display());
    println!("Config:      {}", config.config_path.display());
    println!();
    println!(
        "🤖 Provider:      {}",
        config.default_provider.as_deref().unwrap_or("openrouter")
    );
    println!(
        "   Model:         {}",
        config.default_model.as_deref().unwrap_or("(default)")
    );
    println!("📊 Observability:  {}", config.observability.backend);
    println!(
        "🧾 Trace storage:  {} ({})",
        config.observability.runtime_trace_mode, config.observability.runtime_trace_path
    );
    println!("🛡️  Autonomy:      {:?}", config.autonomy.level);
    println!("⚙️  Runtime:       {}", config.runtime.kind);
    let effective_memory_backend = memory::effective_memory_backend_name(
        &config.memory.backend,
        Some(&config.storage.provider.config),
    );
    println!(
        "💓 Heartbeat:      {}",
        if config.heartbeat.enabled {
            format!("every {}min", config.heartbeat.interval_minutes)
        } else {
            "disabled".into()
        }
    );
    println!(
        "🧠 Memory:         {} (auto-save: {})",
        effective_memory_backend,
        if config.memory.auto_save { "on" } else { "off" }
    );

    println!();
    println!("Security:");
    println!("  Workspace only:    {}", config.autonomy.workspace_only);
    println!(
        "  Allowed roots:     {}",
        if config.autonomy.allowed_roots.is_empty() {
            "(none)".to_string()
        } else {
            config.autonomy.allowed_roots.join(", ")
        }
    );
    println!(
        "  Allowed commands:  {}",
        config.autonomy.allowed_commands.join(", ")
    );
    println!(
        "  Max actions/hour:  {}",
        config.autonomy.max_actions_per_hour
    );
    println!(
        "  Max cost/day:      ${:.2}",
        f64::from(config.autonomy.max_cost_per_day_cents) / 100.0
    );
    println!("  OTP enabled:       {}", config.security.otp.enabled);
    println!("  E-stop enabled:    {}", config.security.estop.enabled);
    println!();
    println!("Channels:");
    println!("  CLI:      ✅ always");
    for (channel, configured) in config.channels_config.channels() {
        println!(
            "  {:9} {}",
            channel.name(),
            if configured {
                "✅ configured"
            } else {
                "❌ not configured"
            }
        );
    }
    println!();
    println!("Peripherals:");
    println!(
        "  Enabled:   {}",
        if config.peripherals.enabled {
            "yes"
        } else {
            "no"
        }
    );
    println!("  Boards:    {}", config.peripherals.boards.len());

    Ok(())
}
