use zeroclaw::{providers, Config};

pub(crate) fn run(config: &Config) -> anyhow::Result<()> {
    let providers = providers::list_providers();
    let current = config
        .default_provider
        .as_deref()
        .unwrap_or("openrouter")
        .trim()
        .to_ascii_lowercase();
    println!("Supported providers ({} total):\n", providers.len());
    println!("  ID (use in config)  DESCRIPTION");
    println!("  ─────────────────── ───────────");
    for p in &providers {
        let is_active = p.name.eq_ignore_ascii_case(&current)
            || p.aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(&current));
        let marker = if is_active { " (active)" } else { "" };
        let local_tag = if p.local { " [local]" } else { "" };
        let aliases = if p.aliases.is_empty() {
            String::new()
        } else {
            format!("  (aliases: {})", p.aliases.join(", "))
        };
        println!(
            "  {:<19} {}{}{}{}",
            p.name, p.display_name, local_tag, marker, aliases
        );
    }
    println!("\n  custom:<URL>   Any OpenAI-compatible endpoint");
    println!("  anthropic-custom:<URL>  Any Anthropic-compatible endpoint");
    Ok(())
}
