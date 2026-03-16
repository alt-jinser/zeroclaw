use anyhow::bail;
use clap::Subcommand;
use dialoguer::{Input, Password};
use serde::{Deserialize, Serialize};
use tracing::warn;
use zeroclaw::{auth, security, Config};

const PROFILE_MISMATCH_PREFIX: &str = "Pending login profile mismatch:";

/// Check if a pending OAuth login is stale (older than 24 hours).
fn read_auth_input(prompt: &str) -> anyhow::Result<String> {
    let input = Password::new()
        .with_prompt(prompt)
        .allow_empty_password(false)
        .interact()?;
    Ok(input.trim().to_string())
}

fn read_plain_input(prompt: &str) -> anyhow::Result<String> {
    let input: String = Input::new().with_prompt(prompt).interact_text()?;
    Ok(input.trim().to_string())
}

fn format_expiry(profile: &auth::profiles::AuthProfile) -> String {
    match profile
        .token_set
        .as_ref()
        .and_then(|token_set| token_set.expires_at)
    {
        Some(ts) => {
            let now = chrono::Utc::now();
            if ts <= now {
                format!("expired at {}", ts.to_rfc3339())
            } else {
                let mins = (ts - now).num_minutes();
                format!("expires in {mins}m ({})", ts.to_rfc3339())
            }
        }
        None => "n/a".to_string(),
    }
}

fn is_pending_login_stale(pending: &PendingOAuthLogin) -> bool {
    if let Ok(created) = chrono::DateTime::parse_from_rfc3339(&pending.created_at) {
        let age = chrono::Utc::now().signed_duration_since(created);
        age > chrono::Duration::hours(24)
    } else {
        // If we can't parse the timestamp, consider it stale
        true
    }
}

fn load_pending_oauth_login(
    config: &Config,
    provider: &str,
) -> anyhow::Result<Option<PendingOAuthLogin>> {
    let path = pending_oauth_login_path(config, provider);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path)?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let persisted: PendingOAuthLoginFile = serde_json::from_slice(&bytes)?;
    let secret_store = pending_oauth_secret_store(config);
    let code_verifier = if let Some(encrypted) = persisted.encrypted_code_verifier {
        secret_store.decrypt(&encrypted)?
    } else if let Some(plaintext) = persisted.code_verifier {
        plaintext
    } else {
        bail!("Pending {} login is missing code verifier", provider);
    };

    let pending = PendingOAuthLogin {
        provider: persisted.provider.unwrap_or_else(|| provider.to_string()),
        profile: persisted.profile,
        code_verifier,
        state: persisted.state,
        created_at: persisted.created_at,
    };

    // Auto-cleanup if stale (older than 24 hours)
    if is_pending_login_stale(&pending) {
        println!("ℹ️  Removing stale pending auth file (older than 24h)");
        let _ = std::fs::remove_file(&path);
        return Ok(None);
    }

    Ok(Some(pending))
}

fn extract_openai_account_id_for_profile(access_token: &str) -> Option<String> {
    let account_id = auth::openai_oauth::extract_account_id_from_jwt(access_token);
    if account_id.is_none() {
        warn!(
            "Could not extract OpenAI account id from OAuth access token; \
             requests may fail until re-authentication."
        );
    }
    account_id
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingOAuthLoginFile {
    #[serde(default)]
    provider: Option<String>,
    profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code_verifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    encrypted_code_verifier: Option<String>,
    state: String,
    created_at: String,
}

fn pending_oauth_secret_store(config: &Config) -> security::secrets::SecretStore {
    security::secrets::SecretStore::new(&auth::state_dir_from_config(config), true)
}

fn clear_pending_oauth_login(config: &Config, provider: &str) {
    let path = pending_oauth_login_path(config, provider);
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&path) {
        let _ = file.set_len(0);
        let _ = file.sync_all();
    }
    let _ = std::fs::remove_file(path);
}

#[cfg(unix)]
fn set_owner_only_permissions(path: &std::path::Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &std::path::Path) -> anyhow::Result<()> {
    Ok(())
}

/// Generic pending OAuth login state, shared across providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingOAuthLogin {
    provider: String,
    profile: String,
    code_verifier: String,
    state: String,
    created_at: String,
}

fn save_pending_oauth_login(config: &Config, pending: &PendingOAuthLogin) -> anyhow::Result<()> {
    let path = pending_oauth_login_path(config, &pending.provider);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let secret_store = pending_oauth_secret_store(config);
    let encrypted_code_verifier = secret_store.encrypt(&pending.code_verifier)?;
    let persisted = PendingOAuthLoginFile {
        provider: Some(pending.provider.clone()),
        profile: pending.profile.clone(),
        code_verifier: None,
        encrypted_code_verifier: Some(encrypted_code_verifier),
        state: pending.state.clone(),
        created_at: pending.created_at.clone(),
    };
    let tmp = path.with_extension(format!(
        "tmp.{}.{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let json = serde_json::to_vec_pretty(&persisted)?;
    std::fs::write(&tmp, json)?;
    set_owner_only_permissions(&tmp)?;
    std::fs::rename(tmp, &path)?;
    set_owner_only_permissions(&path)?;
    Ok(())
}

fn pending_oauth_login_path(config: &Config, provider: &str) -> std::path::PathBuf {
    let filename = format!("auth-{}-pending.json", provider);
    auth::state_dir_from_config(config).join(filename)
}

#[derive(Subcommand, Debug)]
pub(crate) enum AuthCommands {
    /// Login with OAuth (OpenAI Codex or Gemini)
    Login {
        /// Provider (`openai-codex` or `gemini`)
        #[arg(long)]
        provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
        /// Use OAuth device-code flow
        #[arg(long)]
        device_code: bool,
    },
    /// Complete OAuth by pasting redirect URL or auth code
    PasteRedirect {
        /// Provider (`openai-codex`)
        #[arg(long)]
        provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
        /// Full redirect URL or raw OAuth code
        #[arg(long)]
        input: Option<String>,
    },
    /// Paste setup token / auth token (for Anthropic subscription auth)
    PasteToken {
        /// Provider (`anthropic`)
        #[arg(long)]
        provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
        /// Token value (if omitted, read interactively)
        #[arg(long)]
        token: Option<String>,
        /// Auth kind override (`authorization` or `api-key`)
        #[arg(long)]
        auth_kind: Option<String>,
    },
    /// Alias for `paste-token` (interactive by default)
    SetupToken {
        /// Provider (`anthropic`)
        #[arg(long)]
        provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
    },
    /// Refresh OpenAI Codex access token using refresh token
    Refresh {
        /// Provider (`openai-codex`)
        #[arg(long)]
        provider: String,
        /// Profile name or profile id
        #[arg(long)]
        profile: Option<String>,
    },
    /// Remove auth profile
    Logout {
        /// Provider
        #[arg(long)]
        provider: String,
        /// Profile name (default: default)
        #[arg(long, default_value = "default")]
        profile: String,
    },
    /// Set active profile for a provider
    Use {
        /// Provider
        #[arg(long)]
        provider: String,
        /// Profile name or full profile id
        #[arg(long)]
        profile: String,
    },
    /// List auth profiles
    List,
    /// Show auth status with active profile and token expiry info
    Status,
}

#[derive(clap::Args, Debug)]
pub(crate) struct AuthArgs {
    #[command(subcommand)]
    auth_command: AuthCommands,
}

pub(crate) async fn run(args: AuthArgs, config: &Config) -> anyhow::Result<()> {
    let auth_service = auth::AuthService::from_config(config);

    match args.auth_command {
        AuthCommands::Login {
            provider,
            profile,
            device_code,
        } => {
            let provider = auth::normalize_provider(&provider)?;
            let client = reqwest::Client::new();

            match provider.as_str() {
                "gemini" => {
                    // Gemini OAuth flow
                    if device_code {
                        match auth::gemini_oauth::start_device_code_flow(&client).await {
                            Ok(device) => {
                                println!("Google/Gemini device-code login started.");
                                println!("Visit: {}", device.verification_uri);
                                println!("Code:  {}", device.user_code);
                                if let Some(uri_complete) = &device.verification_uri_complete {
                                    println!("Fast link: {uri_complete}");
                                }

                                let token_set =
                                    auth::gemini_oauth::poll_device_code_tokens(&client, &device)
                                        .await?;
                                let account_id = token_set.id_token.as_deref().and_then(
                                    auth::gemini_oauth::extract_account_email_from_id_token,
                                );

                                auth_service
                                    .store_gemini_tokens(&profile, token_set, account_id, true)
                                    .await?;

                                println!("Saved profile {profile}");
                                println!("Active profile for gemini: {profile}");
                                return Ok(());
                            }
                            Err(e) => {
                                let err_msg = e.to_string();
                                if err_msg.contains("403")
                                    || err_msg.contains("Forbidden")
                                    || err_msg.contains("Cloudflare")
                                {
                                    println!(
                                        "ℹ️  Device-code flow is blocked by Cloudflare protection."
                                    );
                                    println!("   This is normal for server environments.");
                                    println!("   Switching to browser authorization flow...");
                                } else if err_msg.contains("invalid_client") {
                                    println!("⚠️  OAuth client configuration error: {}", err_msg);
                                    println!("   Check your GEMINI_OAUTH_CLIENT_ID and GEMINI_OAUTH_CLIENT_SECRET");
                                } else {
                                    println!("ℹ️  Device-code flow unavailable: {}", err_msg);
                                    println!("   Falling back to browser flow.");
                                }
                            }
                        }
                    }

                    let pkce = auth::gemini_oauth::generate_pkce_state();
                    let authorize_url = auth::gemini_oauth::build_authorize_url(&pkce)?;

                    // Save pending login for paste-redirect fallback
                    let pending = PendingOAuthLogin {
                        provider: "gemini".to_string(),
                        profile: profile.clone(),
                        code_verifier: pkce.code_verifier.clone(),
                        state: pkce.state.clone(),
                        created_at: chrono::Utc::now().to_rfc3339(),
                    };
                    save_pending_oauth_login(config, &pending)?;

                    println!("Open this URL in your browser and authorize access:");
                    println!("{authorize_url}");
                    println!();

                    let code = match auth::gemini_oauth::receive_loopback_code(
                        &pkce.state,
                        std::time::Duration::from_secs(180),
                    )
                    .await
                    {
                        Ok(code) => {
                            clear_pending_oauth_login(config, "gemini");
                            code
                        }
                        Err(e) => {
                            println!("Callback capture failed: {e}");
                            println!(
                                "Run `zeroclaw auth paste-redirect --provider gemini --profile {profile}`"
                            );
                            return Ok(());
                        }
                    };

                    let token_set =
                        auth::gemini_oauth::exchange_code_for_tokens(&client, &code, &pkce).await?;
                    let account_id = token_set
                        .id_token
                        .as_deref()
                        .and_then(auth::gemini_oauth::extract_account_email_from_id_token);

                    auth_service
                        .store_gemini_tokens(&profile, token_set, account_id, true)
                        .await?;

                    println!("Saved profile {profile}");
                    println!("Active profile for gemini: {profile}");
                    Ok(())
                }
                "openai-codex" => {
                    // OpenAI Codex OAuth flow
                    if device_code {
                        match auth::openai_oauth::start_device_code_flow(&client).await {
                            Ok(device) => {
                                println!("OpenAI device-code login started.");
                                println!("Visit: {}", device.verification_uri);
                                println!("Code:  {}", device.user_code);
                                if let Some(uri_complete) = &device.verification_uri_complete {
                                    println!("Fast link: {uri_complete}");
                                }
                                if let Some(message) = &device.message {
                                    println!("{message}");
                                }

                                let token_set =
                                    auth::openai_oauth::poll_device_code_tokens(&client, &device)
                                        .await?;
                                let account_id =
                                    extract_openai_account_id_for_profile(&token_set.access_token);

                                auth_service
                                    .store_openai_tokens(&profile, token_set, account_id, true)
                                    .await?;
                                clear_pending_oauth_login(config, "openai");

                                println!("Saved profile {profile}");
                                println!("Active profile for openai-codex: {profile}");
                                return Ok(());
                            }
                            Err(e) => {
                                let err_msg = e.to_string();
                                if err_msg.contains("403")
                                    || err_msg.contains("Forbidden")
                                    || err_msg.contains("Cloudflare")
                                {
                                    println!(
                                        "ℹ️  Device-code flow is blocked by Cloudflare protection."
                                    );
                                    println!("   This is normal for server environments.");
                                    println!("   Switching to browser authorization flow...");
                                } else {
                                    println!("ℹ️  Device-code flow unavailable: {}", err_msg);
                                    println!("   Falling back to browser flow.");
                                }
                            }
                        }
                    }

                    let pkce = auth::openai_oauth::generate_pkce_state();
                    let pending = PendingOAuthLogin {
                        provider: "openai".to_string(),
                        profile: profile.clone(),
                        code_verifier: pkce.code_verifier.clone(),
                        state: pkce.state.clone(),
                        created_at: chrono::Utc::now().to_rfc3339(),
                    };
                    save_pending_oauth_login(config, &pending)?;

                    let authorize_url = auth::openai_oauth::build_authorize_url(&pkce);
                    println!("Open this URL in your browser and authorize access:");
                    println!("{authorize_url}");
                    println!();
                    println!("Waiting for callback at http://localhost:1455/auth/callback ...");

                    let code = match auth::openai_oauth::receive_loopback_code(
                        &pkce.state,
                        std::time::Duration::from_secs(180),
                    )
                    .await
                    {
                        Ok(code) => code,
                        Err(e) => {
                            println!("Callback capture failed: {e}");
                            println!(
                                "Run `zeroclaw auth paste-redirect --provider openai-codex --profile {profile}`"
                            );
                            return Ok(());
                        }
                    };

                    let token_set =
                        auth::openai_oauth::exchange_code_for_tokens(&client, &code, &pkce).await?;
                    let account_id = extract_openai_account_id_for_profile(&token_set.access_token);

                    auth_service
                        .store_openai_tokens(&profile, token_set, account_id, true)
                        .await?;
                    clear_pending_oauth_login(config, "openai");

                    println!("Saved profile {profile}");
                    println!("Active profile for openai-codex: {profile}");
                    Ok(())
                }
                _ => {
                    bail!(
                        "`auth login` supports --provider openai-codex or gemini, got: {provider}"
                    );
                }
            }
        }

        AuthCommands::PasteRedirect {
            provider,
            profile,
            input,
        } => {
            let provider = auth::normalize_provider(&provider)?;

            match provider.as_str() {
                "openai-codex" => {
                    let result = async {
                        let pending =
                            load_pending_oauth_login(config, "openai")?.ok_or_else(|| {
                                anyhow::anyhow!(
                                    "No pending OpenAI login found.\n\n\
                                💡 Please start the login flow first:\n   \
                                zeroclaw auth login --provider openai-codex --profile {}\n\n\
                                Then paste the callback URL or code here.",
                                    profile
                                )
                            })?;

                        if pending.profile != profile {
                            bail!(
                                "{} pending={}, requested={}",
                                PROFILE_MISMATCH_PREFIX,
                                pending.profile,
                                profile
                            );
                        }

                        let redirect_input = match input {
                            Some(value) => value,
                            None => read_plain_input("Paste redirect URL or OAuth code")?,
                        };

                        let code = auth::openai_oauth::parse_code_from_redirect(
                            &redirect_input,
                            Some(&pending.state),
                        )?;

                        let pkce = auth::openai_oauth::PkceState {
                            code_verifier: pending.code_verifier.clone(),
                            code_challenge: String::new(),
                            state: pending.state.clone(),
                        };

                        let client = reqwest::Client::new();
                        let token_set =
                            auth::openai_oauth::exchange_code_for_tokens(&client, &code, &pkce)
                                .await?;
                        let account_id =
                            extract_openai_account_id_for_profile(&token_set.access_token);

                        auth_service
                            .store_openai_tokens(&profile, token_set, account_id, true)
                            .await?;
                        clear_pending_oauth_login(config, "openai");

                        println!("Saved profile {profile}");
                        println!("Active profile for openai-codex: {profile}");
                        Ok(())
                    }
                    .await;

                    if let Err(e) = result {
                        // Cleanup pending file on error
                        if e.to_string().starts_with(PROFILE_MISMATCH_PREFIX) {
                            clear_pending_oauth_login(config, "openai");
                            eprintln!("❌ {}", e);
                            eprintln!(
                                "\n💡 Tip: A previous login attempt was for a different profile."
                            );
                            eprintln!("   The pending auth file has been cleared.");
                            eprintln!("   Please start fresh with:");
                            eprintln!(
                                "   zeroclaw auth login --provider openai-codex --profile {}",
                                profile
                            );
                            std::process::exit(1);
                        }
                        return Err(e);
                    }
                }
                "gemini" => {
                    let result = async {
                        let pending =
                            load_pending_oauth_login(config, "gemini")?.ok_or_else(|| {
                                anyhow::anyhow!(
                                    "No pending Gemini login found.\n\n\
                                💡 Please start the login flow first:\n   \
                                zeroclaw auth login --provider gemini --profile {}\n\n\
                                Then paste the callback URL or code here.",
                                    profile
                                )
                            })?;

                        if pending.profile != profile {
                            bail!(
                                "{} pending={}, requested={}",
                                PROFILE_MISMATCH_PREFIX,
                                pending.profile,
                                profile
                            );
                        }

                        let redirect_input = match input {
                            Some(value) => value,
                            None => read_plain_input("Paste redirect URL or OAuth code")?,
                        };

                        let code = auth::gemini_oauth::parse_code_from_redirect(
                            &redirect_input,
                            Some(&pending.state),
                        )?;

                        let pkce = auth::gemini_oauth::PkceState {
                            code_verifier: pending.code_verifier.clone(),
                            code_challenge: String::new(),
                            state: pending.state.clone(),
                        };

                        let client = reqwest::Client::new();
                        let token_set =
                            auth::gemini_oauth::exchange_code_for_tokens(&client, &code, &pkce)
                                .await?;
                        let account_id = token_set
                            .id_token
                            .as_deref()
                            .and_then(auth::gemini_oauth::extract_account_email_from_id_token);

                        auth_service
                            .store_gemini_tokens(&profile, token_set, account_id, true)
                            .await?;
                        clear_pending_oauth_login(config, "gemini");

                        println!("Saved profile {profile}");
                        println!("Active profile for gemini: {profile}");
                        Ok(())
                    }
                    .await;

                    if let Err(e) = result {
                        // Cleanup pending file on error
                        if e.to_string().starts_with(PROFILE_MISMATCH_PREFIX) {
                            clear_pending_oauth_login(config, "gemini");
                            eprintln!("❌ {}", e);
                            eprintln!(
                                "\n💡 Tip: A previous login attempt was for a different profile."
                            );
                            eprintln!("   The pending auth file has been cleared.");
                            eprintln!("   Please start fresh with:");
                            eprintln!(
                                "   zeroclaw auth login --provider gemini --profile {}",
                                profile
                            );
                            std::process::exit(1);
                        }
                        return Err(e);
                    }
                }
                _ => {
                    bail!("`auth paste-redirect` supports --provider openai-codex or gemini");
                }
            }
            Ok(())
        }

        AuthCommands::PasteToken {
            provider,
            profile,
            token,
            auth_kind,
        } => {
            let provider = auth::normalize_provider(&provider)?;
            let token = match token {
                Some(token) => token.trim().to_string(),
                None => read_auth_input("Paste token")?,
            };
            if token.is_empty() {
                bail!("Token cannot be empty");
            }

            let kind = auth::anthropic_token::detect_auth_kind(&token, auth_kind.as_deref());
            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                "auth_kind".to_string(),
                kind.as_metadata_value().to_string(),
            );

            auth_service
                .store_provider_token(&provider, &profile, &token, metadata, true)
                .await?;
            println!("Saved profile {profile}");
            println!("Active profile for {provider}: {profile}");
            Ok(())
        }

        AuthCommands::SetupToken { provider, profile } => {
            let provider = auth::normalize_provider(&provider)?;
            let token = read_auth_input("Paste token")?;
            if token.is_empty() {
                bail!("Token cannot be empty");
            }

            let kind = auth::anthropic_token::detect_auth_kind(&token, Some("authorization"));
            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                "auth_kind".to_string(),
                kind.as_metadata_value().to_string(),
            );

            auth_service
                .store_provider_token(&provider, &profile, &token, metadata, true)
                .await?;
            println!("Saved profile {profile}");
            println!("Active profile for {provider}: {profile}");
            Ok(())
        }

        AuthCommands::Refresh { provider, profile } => {
            let provider = auth::normalize_provider(&provider)?;

            match provider.as_str() {
                "openai-codex" => {
                    match auth_service
                        .get_valid_openai_access_token(profile.as_deref())
                        .await?
                    {
                        Some(_) => {
                            println!("OpenAI Codex token is valid (refresh completed if needed).");
                            Ok(())
                        }
                        None => {
                            bail!(
                                "No OpenAI Codex auth profile found. Run `zeroclaw auth login --provider openai-codex`."
                            )
                        }
                    }
                }
                "gemini" => {
                    match auth_service
                        .get_valid_gemini_access_token(profile.as_deref())
                        .await?
                    {
                        Some(_) => {
                            let profile_name = profile.as_deref().unwrap_or("default");
                            println!("✓ Gemini token refreshed successfully");
                            println!("  Profile: gemini:{}", profile_name);
                            Ok(())
                        }
                        None => {
                            bail!(
                                "No Gemini auth profile found. Run `zeroclaw auth login --provider gemini`."
                            )
                        }
                    }
                }
                _ => bail!("`auth refresh` supports --provider openai-codex or gemini"),
            }
        }

        AuthCommands::Logout { provider, profile } => {
            let provider = auth::normalize_provider(&provider)?;
            let removed = auth_service.remove_profile(&provider, &profile).await?;
            if removed {
                println!("Removed auth profile {provider}:{profile}");
            } else {
                println!("Auth profile not found: {provider}:{profile}");
            }
            Ok(())
        }

        AuthCommands::Use { provider, profile } => {
            let provider = auth::normalize_provider(&provider)?;
            auth_service.set_active_profile(&provider, &profile).await?;
            println!("Active profile for {provider}: {profile}");
            Ok(())
        }

        AuthCommands::List => {
            let data = auth_service.load_profiles().await?;
            if data.profiles.is_empty() {
                println!("No auth profiles configured.");
                return Ok(());
            }

            for (id, profile) in &data.profiles {
                let active = data
                    .active_profiles
                    .get(&profile.provider)
                    .is_some_and(|active_id| active_id == id);
                let marker = if active { "*" } else { " " };
                println!("{marker} {id}");
            }

            Ok(())
        }

        AuthCommands::Status => {
            let data = auth_service.load_profiles().await?;
            if data.profiles.is_empty() {
                println!("No auth profiles configured.");
                return Ok(());
            }

            for (id, profile) in &data.profiles {
                let active = data
                    .active_profiles
                    .get(&profile.provider)
                    .is_some_and(|active_id| active_id == id);
                let marker = if active { "*" } else { " " };
                println!(
                    "{} {} kind={:?} account={} expires={}",
                    marker,
                    id,
                    profile.kind,
                    security::redact(profile.account_id.as_deref().unwrap_or("unknown")),
                    format_expiry(profile)
                );
            }

            println!();
            println!("Active profiles:");
            for (provider, profile_id) in &data.active_profiles {
                println!("  {provider}: {profile_id}");
            }

            Ok(())
        }
    }
}
