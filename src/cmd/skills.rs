use std::path::PathBuf;

use anyhow::Context as _;
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use zeroclaw::{
    skills::{
        audit, clawhub_download_url, enforce_workspace_skill_symlink_trust,
        install_git_skill_source, install_local_skill_source, install_local_zip_source,
        install_registry_skill_source, install_zip_url_source, is_clawhub_source, is_git_source,
        is_registry_source, is_zip_url_source, load_skills_full_with_config,
        resolve_trusted_skill_roots, scaffold_skill, skills_dir, templates, test_skill_locally,
        zip_url_from_source,
    },
    Config,
};

#[derive(clap::Args, Debug)]
pub(crate) struct SkillsArgs {
    #[command(subcommand)]
    skill_command: SkillCommands,
}

/// Skills management subcommands
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkillCommands {
    /// List all installed skills
    List,
    /// Scaffold a new skill project from a template
    New {
        /// Skill name (snake_case recommended, e.g. my_weather_tool)
        name: String,
        /// Template language: typescript, rust, go, python
        #[arg(long, short, default_value = "typescript")]
        template: String,
    },
    /// Run a skill tool locally for testing (reads args from --args or stdin)
    Test {
        /// Path to the skill directory or installed skill name
        path: String,
        /// Optional tool name inside the skill (defaults to first tool found)
        #[arg(long)]
        tool: Option<String>,
        /// JSON arguments to pass to the tool, e.g. '{"city":"Hanoi"}'
        #[arg(long, short)]
        args: Option<String>,
    },
    /// Audit a skill source directory or installed skill name
    Audit {
        /// Skill path or installed skill name
        source: String,
    },
    /// Install a new skill from a local path, git URL, or registry (namespace/name)
    Install {
        /// Source: local path, git URL, or registry package (e.g. acme/my-tool)
        source: String,
    },
    /// Remove an installed skill
    Remove {
        /// Skill name to remove
        name: String,
    },
    /// List all available skill templates
    Templates,
}

pub(crate) fn run(args: SkillsArgs, config: &Config) -> anyhow::Result<()> {
    let workspace_dir = &config.workspace_dir;
    match args.skill_command {
        SkillCommands::New { name, template } => {
            let dest = std::env::current_dir().unwrap_or_else(|_| workspace_dir.clone());

            scaffold_skill(&name, &template, &dest)
                .with_context(|| format!("failed to scaffold skill '{name}'"))?;

            // Resolve template again for display (find is cheap; scaffold_skill already
            // validated that the template exists, so this should never be None).
            let tmpl = templates::find(&template).ok_or_else(|| {
                anyhow::anyhow!("template '{}' not found after scaffold", template)
            })?;

            let skill_dir = dest.join(&name);
            println!(
                "  {} Skill '{}' created at {}",
                console::style("✓").green().bold(),
                name,
                skill_dir.display()
            );
            println!(
                "  Template: {} ({})",
                console::style(tmpl.name).cyan(),
                tmpl.language
            );
            println!();
            println!("  Next steps:");
            println!("    cd {name}");
            match tmpl.language {
                "typescript" => {
                    println!("    npm install && npm run build   # → tool.wasm");
                }
                "rust" => {
                    println!(
                        "    {}  # one-time setup",
                        console::style("rustup target add wasm32-wasip1").yellow()
                    );
                    println!("    cargo build --target wasm32-wasip1 --release");
                    println!("    cp target/wasm32-wasip1/release/*.wasm tool.wasm");
                }
                "go" => {
                    println!("    tinygo build -o tool.wasm -target wasi .");
                }
                "python" => {
                    println!("    pip install componentize-py");
                    println!("    componentize-py -d wit/ -w zeroclaw-skill componentize main -o tool.wasm");
                }
                _ => {}
            }
            println!("    zeroclaw skill test . --args '{}'", tmpl.test_args);
            println!();
            println!(
                "  {} 'zeroclaw skill test' requires the {} CLI:",
                console::style("Note:").dim(),
                console::style("wasmtime").cyan()
            );
            println!(
                "    macOS:       {}",
                console::style("brew install wasmtime").yellow()
            );
            println!(
                "    Linux/macOS: {}",
                console::style("curl https://wasmtime.dev/install.sh -sSf | bash").yellow()
            );
            println!();
            println!(
                "  To publish: upload this folder to {}",
                console::style("https://zeromarket.dev/upload").underlined()
            );

            Ok(())
        }

        SkillCommands::Test { path, tool, args } => {
            let skill_path = std::path::Path::new(&path);
            let skill_path = if skill_path.is_absolute() {
                skill_path.to_path_buf()
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| workspace_dir.clone())
                    .join(skill_path)
            };

            // If `path` is just a skill name, resolve from installed skills dir
            let skill_path = if !skill_path.exists() && !path.contains('/') && !path.contains('\\')
            {
                skills_dir(workspace_dir).join(&path)
            } else {
                skill_path
            };

            if !skill_path.exists() {
                anyhow::bail!(
                    "Skill path not found: {}\n\
                     Tip: run from the skill directory or pass an absolute path.",
                    skill_path.display()
                );
            }

            let args_json = args.as_deref().unwrap_or("{\"input\":\"test\"}");

            test_skill_locally(&skill_path, tool.as_deref(), args_json)
                .with_context(|| format!("skill test failed for {}", skill_path.display()))?;

            Ok(())
        }

        SkillCommands::List => {
            let skills = load_skills_full_with_config(workspace_dir, config);
            if skills.is_empty() {
                println!("No skills installed.");
                println!();
                println!("  Create one: mkdir -p ~/.zeroclaw/workspace/skills/my-skill");
                println!("              echo '# My Skill' > ~/.zeroclaw/workspace/skills/my-skill/SKILL.md");
                println!();
                println!("  Or install: zeroclaw skills install <source>");
            } else {
                println!("Installed skills ({}):", skills.len());
                println!();
                for skill in &skills {
                    println!(
                        "  {} {} — {}",
                        console::style(&skill.name).white().bold(),
                        console::style(format!("v{}", skill.version)).dim(),
                        skill.description
                    );
                    if !skill.tools.is_empty() {
                        println!(
                            "    Tools: {}",
                            skill
                                .tools
                                .iter()
                                .map(|t| t.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if !skill.tags.is_empty() {
                        println!("    Tags:  {}", skill.tags.join(", "));
                    }
                }
            }
            println!();
            Ok(())
        }
        SkillCommands::Audit { source } => {
            let source_path = PathBuf::from(&source);
            let target = if source_path.exists() {
                source_path
            } else {
                skills_dir(workspace_dir).join(&source)
            };

            if !target.exists() {
                anyhow::bail!("Skill source or installed skill not found: {source}");
            }

            let trusted_skill_roots =
                resolve_trusted_skill_roots(workspace_dir, &config.skills.trusted_skill_roots);
            if let Ok(metadata) = std::fs::symlink_metadata(&target) {
                if metadata.file_type().is_symlink() {
                    enforce_workspace_skill_symlink_trust(&target, &trusted_skill_roots)
                        .with_context(|| {
                            format!(
                                "trusted-symlink policy rejected audit target {}",
                                target.display()
                            )
                        })?;
                }
            }

            let report = audit::audit_skill_directory_with_options(
                &target,
                audit::SkillAuditOptions {
                    allow_scripts: config.skills.allow_scripts,
                },
            )?;
            if report.is_clean() {
                println!(
                    "  {} Skill audit passed for {} ({} files scanned).",
                    console::style("✓").green().bold(),
                    target.display(),
                    report.files_scanned
                );
                return Ok(());
            }

            println!(
                "  {} Skill audit failed for {}",
                console::style("✗").red().bold(),
                target.display()
            );
            for finding in report.findings {
                println!("    - {finding}");
            }
            anyhow::bail!("Skill audit failed.");
        }
        SkillCommands::Install { source } => {
            println!("Installing skill from: {source}");

            let skills_path = skills_dir(workspace_dir);
            std::fs::create_dir_all(&skills_path)?;

            if is_clawhub_source(&source) {
                let download_url = clawhub_download_url(&source)
                    .with_context(|| format!("invalid ClawhHub source: {source}"))?;
                let token = config.skills.clawhub_token.as_deref();
                let (installed_dir, files_written) =
                    install_zip_url_source(&download_url, &skills_path, token)
                        .with_context(|| format!("failed to install ClawhHub skill: {source}"))?;
                println!(
                    "  {} ClawhHub skill installed: {} ({} files written)",
                    console::style("✓").green().bold(),
                    installed_dir.display(),
                    files_written
                );
                println!("  Run 'zeroclaw skill list' to verify the new tools are available.");
            } else if is_zip_url_source(&source) {
                // Generic zip-URL install: supports `zip:https://...` prefix and
                // direct `.zip` URLs.  No system `unzip` binary required.
                let url = zip_url_from_source(&source);
                let (installed_dir, files_written) =
                    install_zip_url_source(url, &skills_path, None)
                        .with_context(|| format!("failed to install zip skill from: {url}"))?;
                println!(
                    "  {} Skill installed from zip: {} ({} files written)",
                    console::style("✓").green().bold(),
                    installed_dir.display(),
                    files_written
                );
                println!("  Run 'zeroclaw skill list' to verify the new tools are available.");
            } else if is_git_source(&source) {
                let (installed_dir, files_scanned) =
                    install_git_skill_source(&source, &skills_path, config.skills.allow_scripts)
                        .with_context(|| format!("failed to install git skill source: {source}"))?;
                println!(
                    "  {} Skill installed and audited: {} ({} files scanned)",
                    console::style("✓").green().bold(),
                    installed_dir.display(),
                    files_scanned
                );
                println!("  Security audit completed successfully.");
            } else if is_registry_source(&source) {
                // ZeroMarket (or compatible) registry: `namespace/name[@version]`
                let registry_url = &config.wasm.registry_url;
                let (installed_dir, files_written) =
                    install_registry_skill_source(&source, &skills_path, registry_url)
                        .with_context(|| format!("failed to install registry package: {source}"))?;
                println!(
                    "  {} WASM skill package installed: {} ({} files written)",
                    console::style("✓").green().bold(),
                    installed_dir.display(),
                    files_written
                );
                println!("  Run 'zeroclaw skill list' to verify the new tools are available.");
            } else {
                // Check if source is a local .zip file before falling back to directory install
                let source_path = std::path::Path::new(&source);
                let is_local_zip = source_path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
                    && source_path.is_file();

                if is_local_zip {
                    let (dest, files_written) = install_local_zip_source(source_path, &skills_path)
                        .with_context(|| format!("failed to install zip skill from: {source}"))?;
                    println!(
                        "  {} Skill installed from zip: {} ({} files written)",
                        console::style("✓").green().bold(),
                        dest.display(),
                        files_written
                    );
                    println!("  Run 'zeroclaw skill list' to verify the new tools are available.");
                } else {
                    let (dest, files_scanned) = install_local_skill_source(
                        &source,
                        &skills_path,
                        config.skills.allow_scripts,
                    )
                    .with_context(|| format!("failed to install local skill source: {source}"))?;
                    println!(
                        "  {} Skill installed and audited: {} ({} files scanned)",
                        console::style("✓").green().bold(),
                        dest.display(),
                        files_scanned
                    );
                    println!("  Security audit completed successfully.");
                }
            }

            Ok(())
        }
        SkillCommands::Remove { name } => {
            // Reject path traversal attempts
            if name.contains("..") || name.contains('/') || name.contains('\\') {
                anyhow::bail!("Invalid skill name: {name}");
            }

            let skill_path = skills_dir(workspace_dir).join(&name);

            // Verify the resolved path is actually inside the skills directory
            let canonical_skills = skills_dir(workspace_dir)
                .canonicalize()
                .unwrap_or_else(|_| skills_dir(workspace_dir));
            if let Ok(canonical_skill) = skill_path.canonicalize() {
                if !canonical_skill.starts_with(&canonical_skills) {
                    anyhow::bail!("Skill path escapes skills directory: {name}");
                }
            }

            if !skill_path.exists() {
                anyhow::bail!("Skill not found: {name}");
            }

            std::fs::remove_dir_all(&skill_path)?;
            println!(
                "  {} Skill '{}' removed.",
                console::style("✓").green().bold(),
                name
            );
            Ok(())
        }

        SkillCommands::Templates => {
            println!("  Available skill templates:\n");
            println!(
                "  {:<20} {:<12} {}",
                console::style("NAME").bold(),
                console::style("LANGUAGE").bold(),
                console::style("DESCRIPTION").bold(),
            );
            println!("  {}", "─".repeat(72));
            for tmpl in templates::ALL {
                println!(
                    "  {:<20} {:<12} {}",
                    console::style(tmpl.name).cyan(),
                    tmpl.language,
                    tmpl.description,
                );
            }
            println!();
            println!("  Usage:");
            println!("    zeroclaw skill new <name> --template <template-name>");
            println!();
            println!("  Example:");
            println!(
                "    zeroclaw skill new my_weather --template {}",
                console::style("weather_lookup").cyan()
            );
            Ok(())
        }
    }
}
