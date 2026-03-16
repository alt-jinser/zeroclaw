use std::collections::HashMap;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use tokio::fs;
use tokio::sync::{Mutex, MutexGuard};
use tokio::test;
use tokio_stream::wrappers::ReadDirStream;
use tokio_stream::StreamExt;
use zeroclaw::config::schema::CostEnforcementMode;
use zeroclaw::config::schema::{
    default_lark_draft_update_interval_ms, default_lark_max_draft_edits, BlueBubblesConfig,
    DingTalkConfig, GoalLoopConfig, LarkReceiveMode, MattermostConfig, McpConfig,
    ModelProviderConfig, NodeControlConfig, ProviderApiMode, QQConfig, QQEnvironment,
    QQReceiveMode, SignalConfig, WhatsAppConfig,
};
use zeroclaw::config::{
    resolve_default_model_id, AckReactionChannelsConfig, AckReactionChatType, AckReactionConfig,
    AckReactionRuleAction, AckReactionRuleConfig, AckReactionStrategy, AgentConfig,
    AgentLoadBalanceStrategy, AgentsIpcConfig, AutonomyConfig, BrowserComputerUseConfig,
    BrowserConfig, ChannelsConfig, CommandContextRuleAction, CommandContextRuleConfig,
    ComposioConfig, Config, CoordinationConfig, CostConfig, CronConfig, DelegateAgentConfig,
    DiscordConfig, EconomicConfig, FeishuConfig, GatewayConfig, GroupReplyMode, HardwareConfig,
    HeartbeatConfig, HooksConfig, HttpRequestConfig, HttpRequestCredentialProfile, IMessageConfig,
    IdentityConfig, LarkConfig, MatrixConfig, MemoryConfig, ModelRouteConfig, MultimodalConfig,
    NextcloudTalkConfig, NonCliNaturalLanguageApprovalMode, ObservabilityConfig,
    OtpChallengeDelivery, OtpMethod, OutboundLeakGuardAction, PeripheralBoardConfig,
    PeripheralsConfig, PluginsConfig, ProgressMode, ProviderConfig, ProxyConfig, ProxyScope,
    QueryClassificationConfig, ReliabilityConfig, ResearchPhaseConfig, RuntimeConfig,
    SchedulerConfig, SecretsConfig, SecurityConfig, SecurityRoleConfig, SkillsConfig,
    SkillsPromptInjectionMode, SlackConfig, StorageConfig, StreamMode, TelegramConfig,
    TranscriptionConfig, TunnelConfig, WasmCapabilityEscalationMode, WasmConfig,
    WasmModuleHashPolicy, WebFetchConfig, WebSearchConfig, WebhookConfig,
};
use zeroclaw::security::AutonomyLevel;

// ── Defaults ─────────────────────────────────────────────

#[test]
async fn http_request_config_default_has_correct_values() {
    let cfg = HttpRequestConfig::default();
    assert_eq!(cfg.timeout_secs, 30);
    assert_eq!(cfg.max_response_size, 1_000_000);
    assert!(!cfg.enabled);
    assert!(cfg.allowed_domains.is_empty());
    assert!(cfg.credential_profiles.is_empty());
}

#[test]
async fn config_default_has_sane_values() {
    let c = Config::default();
    assert_eq!(c.default_provider.as_deref(), Some("openrouter"));
    assert!(c.default_model.as_deref().unwrap().contains("claude"));
    assert!((c.default_temperature - 0.7).abs() < f64::EPSILON);
    assert!(c.api_key.is_none());
    assert!(!c.skills.open_skills_enabled);
    assert!(!c.skills.allow_scripts);
    assert_eq!(
        c.skills.prompt_injection_mode,
        SkillsPromptInjectionMode::Compact
    );
    assert!(c.workspace_dir.to_string_lossy().contains("workspace"));
    assert!(c.config_path.to_string_lossy().contains("config.toml"));
}

#[test]
async fn wasm_config_default_has_correct_values() {
    let cfg = WasmConfig::default();
    assert!(cfg.enabled, "WASM tools should be enabled by default");
    assert_eq!(cfg.memory_limit_mb, 64);
    assert_eq!(cfg.fuel_limit, 1_000_000_000);
    assert_eq!(cfg.registry_url, "https://zeromarket.vercel.app/api");
}

#[test]
async fn wasm_config_invalid_values_rejected() {
    let mut c = Config::default();

    // memory_limit_mb = 0
    c.wasm.memory_limit_mb = 0;
    assert!(c.validate().is_err(), "memory_limit_mb=0 should fail");

    // memory_limit_mb = 257
    c.wasm = WasmConfig::default();
    c.wasm.memory_limit_mb = 257;
    assert!(c.validate().is_err(), "memory_limit_mb=257 should fail");

    // fuel_limit = 0
    c.wasm = WasmConfig::default();
    c.wasm.fuel_limit = 0;
    assert!(c.validate().is_err(), "fuel_limit=0 should fail");

    // empty registry_url
    c.wasm = WasmConfig::default();
    c.wasm.registry_url = String::new();
    assert!(c.validate().is_err(), "empty registry_url should fail");

    // http:// instead of https://
    c.wasm = WasmConfig::default();
    c.wasm.registry_url = "http://example.com".to_string();
    assert!(c.validate().is_err(), "http registry_url should fail");

    // bare "https://"
    c.wasm = WasmConfig::default();
    c.wasm.registry_url = "https://".to_string();
    assert!(c.validate().is_err(), "https:// without host should fail");

    // port-only, no hostname
    c.wasm = WasmConfig::default();
    c.wasm.registry_url = "https://:443".to_string();
    assert!(c.validate().is_err(), "https://:443 should fail");

    // query-only, no hostname
    c.wasm = WasmConfig::default();
    c.wasm.registry_url = "https://?q=1".to_string();
    assert!(c.validate().is_err(), "https://?q=1 should fail");
}

#[test]
async fn config_debug_redacts_sensitive_values() {
    let mut config = Config::default();
    config.workspace_dir = PathBuf::from("/tmp/workspace");
    config.config_path = PathBuf::from("/tmp/config.toml");
    config.api_key = Some("root-credential".into());
    config.storage.provider.config.db_url = Some("postgres://user:pw@host/db".into());
    config.browser.computer_use.api_key = Some("browser-credential".into());
    config.gateway.paired_tokens = vec!["zc_0123456789abcdef".into()];
    config.channels_config.telegram = Some(TelegramConfig {
        bot_token: "telegram-credential".into(),
        allowed_users: Vec::new(),
        stream_mode: StreamMode::Off,
        draft_update_interval_ms: 1000,
        interrupt_on_new_message: false,
        mention_only: false,
        progress_mode: ProgressMode::default(),
        ack_enabled: true,
        group_reply: None,
        base_url: None,
    });
    config.agents.insert(
        "worker".into(),
        DelegateAgentConfig {
            provider: "openrouter".into(),
            model: "model-test".into(),
            system_prompt: None,
            api_key: Some("agent-credential".into()),
            enabled: true,
            capabilities: Vec::new(),
            priority: 0,
            temperature: None,
            max_depth: 3,
            agentic: false,
            allowed_tools: Vec::new(),
            max_iterations: 10,
        },
    );

    let debug_output = format!("{config:?}");
    assert!(debug_output.contains("***REDACTED***"));

    for (idx, secret) in [
        "root-credential",
        "postgres://user:pw@host/db",
        "browser-credential",
        "zc_0123456789abcdef",
        "telegram-credential",
        "agent-credential",
    ]
    .into_iter()
    .enumerate()
    {
        assert!(
            !debug_output.contains(secret),
            "debug output leaked secret value at index {idx}"
        );
    }

    assert!(!debug_output.contains("paired_tokens"));
    assert!(!debug_output.contains("bot_token"));
    assert!(!debug_output.contains("db_url"));
}

#[test]
async fn bluebubbles_debug_redacts_server_url_userinfo() {
    let cfg = BlueBubblesConfig {
        server_url: "https://alice:super-secret@example.com:1234/api/v1".to_string(),
        password: "channel-password".to_string(),
        allowed_senders: vec!["*".to_string()],
        webhook_secret: Some("hook-secret".to_string()),
        ignore_senders: vec![],
    };

    let debug_output = format!("{cfg:?}");
    assert!(debug_output.contains("https://[REDACTED]@example.com:1234/api/v1"));
    assert!(!debug_output.contains("alice:super-secret"));
    assert!(!debug_output.contains("channel-password"));
    assert!(!debug_output.contains("hook-secret"));
}

#[test]
async fn config_schema_export_contains_expected_contract_shape() {
    let schema = schemars::schema_for!(Config);
    let schema_json = serde_json::to_value(&schema).expect("schema should serialize to json");

    assert_eq!(
        schema_json
            .get("$schema")
            .and_then(serde_json::Value::as_str),
        Some("https://json-schema.org/draft/2020-12/schema")
    );

    let properties = schema_json
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .expect("schema should expose top-level properties");

    assert!(properties.contains_key("default_provider"));
    assert!(properties.contains_key("skills"));
    assert!(properties.contains_key("gateway"));
    assert!(properties.contains_key("channels_config"));
    assert!(!properties.contains_key("workspace_dir"));
    assert!(!properties.contains_key("config_path"));

    assert!(
        schema_json
            .get("$defs")
            .and_then(serde_json::Value::as_object)
            .is_some(),
        "schema should include reusable type definitions"
    );
}

#[cfg(unix)]
#[test]
async fn save_sets_config_permissions_on_new_file() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let config_path = temp.path().join("config.toml");
    let workspace_dir = temp.path().join("workspace");

    let mut config = Config::default();
    config.config_path = config_path.clone();
    config.workspace_dir = workspace_dir;

    config.save().await.expect("save config");

    let mode = std::fs::metadata(&config_path)
        .expect("config metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
async fn observability_config_default() {
    let o = ObservabilityConfig::default();
    assert_eq!(o.backend, "none");
    assert_eq!(o.runtime_trace_mode, "none");
    assert_eq!(o.runtime_trace_path, "state/runtime-trace.jsonl");
    assert_eq!(o.runtime_trace_max_entries, 200);
}

#[test]
async fn autonomy_config_default() {
    let a = AutonomyConfig::default();
    assert_eq!(a.level, AutonomyLevel::Supervised);
    assert!(a.workspace_only);
    assert!(a.allowed_commands.contains(&"git".to_string()));
    assert!(a.allowed_commands.contains(&"mkdir".to_string()));
    assert!(a.allowed_commands.contains(&"touch".to_string()));
    assert!(a.allowed_commands.contains(&"cargo".to_string()));
    assert!(a.forbidden_paths.contains(&"/etc".to_string()));
    assert_eq!(a.max_actions_per_hour, 100);
    assert_eq!(a.max_cost_per_day_cents, 1000);
    assert!(a.require_approval_for_medium_risk);
    assert!(a.block_high_risk_commands);
    assert!(a.shell_env_passthrough.is_empty());
    assert!(a.command_context_rules.is_empty());
    assert!(!a.allow_sensitive_file_reads);
    assert!(!a.allow_sensitive_file_writes);
    assert!(a.non_cli_excluded_tools.contains(&"shell".to_string()));
    assert!(a.non_cli_excluded_tools.contains(&"process".to_string()));
    assert!(a.non_cli_excluded_tools.contains(&"delegate".to_string()));
}

#[test]
async fn autonomy_config_serde_defaults_non_cli_excluded_tools() {
    let raw = r#"
level = "supervised"
workspace_only = true
allowed_commands = ["git"]
forbidden_paths = ["/etc"]
max_actions_per_hour = 20
max_cost_per_day_cents = 500
require_approval_for_medium_risk = true
block_high_risk_commands = true
shell_env_passthrough = []
auto_approve = ["file_read"]
always_ask = []
allowed_roots = []
"#;
    let parsed: AutonomyConfig = toml::from_str(raw).unwrap();
    assert!(
        !parsed.allow_sensitive_file_reads,
        "Missing allow_sensitive_file_reads must default to false"
    );
    assert!(
        !parsed.allow_sensitive_file_writes,
        "Missing allow_sensitive_file_writes must default to false"
    );
    assert!(
        parsed.command_context_rules.is_empty(),
        "Missing command_context_rules must default to empty"
    );
    assert!(parsed.non_cli_excluded_tools.contains(&"shell".to_string()));
    assert!(parsed
        .non_cli_excluded_tools
        .contains(&"process".to_string()));
    assert!(parsed
        .non_cli_excluded_tools
        .contains(&"browser".to_string()));
}

#[test]
async fn config_validate_rejects_invalid_command_context_rule_command() {
    let mut cfg = Config::default();
    cfg.autonomy.command_context_rules = vec![CommandContextRuleConfig {
        command: "curl;rm".into(),
        action: CommandContextRuleAction::Allow,
        allowed_domains: vec![],
        allowed_path_prefixes: vec![],
        denied_path_prefixes: vec![],
        allow_high_risk: false,
    }];
    let err = cfg.validate().unwrap_err();
    assert!(err
        .to_string()
        .contains("autonomy.command_context_rules[0].command"));
}

#[test]
async fn config_validate_rejects_empty_command_context_rule_domain() {
    let mut cfg = Config::default();
    cfg.autonomy.command_context_rules = vec![CommandContextRuleConfig {
        command: "curl".into(),
        action: CommandContextRuleAction::Allow,
        allowed_domains: vec!["   ".into()],
        allowed_path_prefixes: vec![],
        denied_path_prefixes: vec![],
        allow_high_risk: true,
    }];
    let err = cfg.validate().unwrap_err();
    assert!(err
        .to_string()
        .contains("autonomy.command_context_rules[0].allowed_domains[0]"));
}

#[test]
async fn autonomy_command_context_rule_supports_require_approval_action() {
    let raw = r#"
level = "supervised"
workspace_only = true
allowed_commands = ["ls", "rm"]
forbidden_paths = ["/etc"]
max_actions_per_hour = 20
max_cost_per_day_cents = 500
require_approval_for_medium_risk = true
block_high_risk_commands = true
shell_env_passthrough = []
auto_approve = ["shell"]
always_ask = []
allowed_roots = []

[[command_context_rules]]
command = "rm"
action = "require_approval"
"#;
    let parsed: AutonomyConfig = toml::from_str(raw).expect("autonomy config should parse");
    assert_eq!(parsed.command_context_rules.len(), 1);
    assert_eq!(
        parsed.command_context_rules[0].action,
        CommandContextRuleAction::RequireApproval
    );
}

#[test]
async fn config_validate_rejects_duplicate_non_cli_excluded_tools() {
    let mut cfg = Config::default();
    cfg.autonomy.non_cli_excluded_tools = vec!["shell".into(), "shell".into()];
    let err = cfg.validate().unwrap_err();
    assert!(err
        .to_string()
        .contains("autonomy.non_cli_excluded_tools contains duplicate entry"));
}

#[test]
async fn runtime_config_default() {
    let r = RuntimeConfig::default();
    assert_eq!(r.kind, "native");
    assert_eq!(r.docker.image, "alpine:3.20");
    assert_eq!(r.docker.network, "none");
    assert_eq!(r.docker.memory_limit_mb, Some(512));
    assert_eq!(r.docker.cpu_limit, Some(1.0));
    assert!(r.docker.read_only_rootfs);
    assert!(r.docker.mount_workspace);
    assert_eq!(r.wasm.tools_dir, "tools/wasm");
    assert_eq!(r.wasm.fuel_limit, 1_000_000);
    assert_eq!(r.wasm.memory_limit_mb, 64);
    assert_eq!(r.wasm.max_module_size_mb, 50);
    assert!(!r.wasm.allow_workspace_read);
    assert!(!r.wasm.allow_workspace_write);
    assert!(r.wasm.allowed_hosts.is_empty());
    assert!(r.wasm.security.require_workspace_relative_tools_dir);
    assert!(r.wasm.security.reject_symlink_modules);
    assert!(r.wasm.security.reject_symlink_tools_dir);
    assert!(r.wasm.security.strict_host_validation);
    assert_eq!(
        r.wasm.security.capability_escalation_mode,
        WasmCapabilityEscalationMode::Deny
    );
    assert_eq!(
        r.wasm.security.module_hash_policy,
        WasmModuleHashPolicy::Warn
    );
    assert!(r.wasm.security.module_sha256.is_empty());
}

#[test]
async fn heartbeat_config_default() {
    let h = HeartbeatConfig::default();
    assert!(!h.enabled);
    assert_eq!(h.interval_minutes, 30);
    assert!(h.message.is_none());
    assert!(h.target.is_none());
    assert!(h.to.is_none());
}

#[test]
async fn heartbeat_config_parses_delivery_aliases() {
    let raw = r#"
enabled = true
interval_minutes = 10
message = "Ping"
channel = "telegram"
recipient = "42"
"#;
    let parsed: HeartbeatConfig = toml::from_str(raw).unwrap();
    assert!(parsed.enabled);
    assert_eq!(parsed.interval_minutes, 10);
    assert_eq!(parsed.message.as_deref(), Some("Ping"));
    assert_eq!(parsed.target.as_deref(), Some("telegram"));
    assert_eq!(parsed.to.as_deref(), Some("42"));
}

#[test]
async fn cron_config_default() {
    let c = CronConfig::default();
    assert!(c.enabled);
    assert_eq!(c.max_run_history, 50);
}

#[test]
async fn cron_config_serde_roundtrip() {
    let c = CronConfig {
        enabled: false,
        max_run_history: 100,
    };
    let json = serde_json::to_string(&c).unwrap();
    let parsed: CronConfig = serde_json::from_str(&json).unwrap();
    assert!(!parsed.enabled);
    assert_eq!(parsed.max_run_history, 100);
}

#[test]
async fn config_defaults_cron_when_section_missing() {
    let toml_str = r#"
workspace_dir = "/tmp/workspace"
config_path = "/tmp/config.toml"
default_temperature = 0.7
"#;

    let parsed: Config = toml::from_str(toml_str).unwrap();
    assert!(parsed.cron.enabled);
    assert_eq!(parsed.cron.max_run_history, 50);
}

#[test]
async fn memory_config_default_hygiene_settings() {
    let m = MemoryConfig::default();
    assert_eq!(m.backend, "sqlite");
    assert!(m.auto_save);
    assert!(m.hygiene_enabled);
    assert_eq!(m.archive_after_days, 7);
    assert_eq!(m.purge_after_days, 30);
    assert_eq!(m.conversation_retention_days, 30);
    assert!(m.sqlite_open_timeout_secs.is_none());
}

#[test]
async fn storage_provider_config_defaults() {
    let storage = StorageConfig::default();
    assert!(storage.provider.config.provider.is_empty());
    assert!(storage.provider.config.db_url.is_none());
    assert_eq!(storage.provider.config.schema, "public");
    assert_eq!(storage.provider.config.table, "memories");
    assert!(storage.provider.config.connect_timeout_secs.is_none());
}

#[test]
async fn channels_config_default() {
    let c = ChannelsConfig::default();
    assert!(c.cli);
    assert!(c.telegram.is_none());
    assert!(c.discord.is_none());
}

#[test]
async fn channels_config_accepts_onebot_alias_with_ws_url() {
    let toml = r#"
cli = true

[onebot]
ws_url = "ws://127.0.0.1:3001"
access_token = "onebot-token"
allowed_users = ["10001"]
"#;

    let parsed: ChannelsConfig =
        toml::from_str(toml).expect("config should accept onebot alias for napcat");
    let napcat = parsed
        .napcat
        .expect("channels_config.onebot should map to napcat config");

    assert_eq!(napcat.websocket_url, "ws://127.0.0.1:3001");
    assert_eq!(napcat.access_token.as_deref(), Some("onebot-token"));
    assert_eq!(napcat.allowed_users, vec!["10001"]);
}

#[test]
async fn channels_config_napcat_still_accepts_ws_url_alias() {
    let toml = r#"
cli = true

[napcat]
ws_url = "ws://127.0.0.1:3002"
"#;

    let parsed: ChannelsConfig =
        toml::from_str(toml).expect("napcat config should accept ws_url as websocket alias");
    let napcat = parsed
        .napcat
        .expect("channels_config.napcat should be present");

    assert_eq!(napcat.websocket_url, "ws://127.0.0.1:3002");
    assert!(napcat.access_token.is_none());
}

// ── Serde round-trip ─────────────────────────────────────

#[test]
async fn config_toml_roundtrip() {
    let config = Config {
        workspace_dir: PathBuf::from("/tmp/test/workspace"),
        config_path: PathBuf::from("/tmp/test/config.toml"),
        api_key: Some("sk-test-key".into()),
        api_url: None,
        default_provider: Some("openrouter".into()),
        provider_api: None,
        default_model: Some("gpt-4o".into()),
        model_providers: HashMap::new(),
        provider: ProviderConfig::default(),
        default_temperature: 0.5,
        observability: ObservabilityConfig {
            backend: "log".into(),
            ..ObservabilityConfig::default()
        },
        autonomy: AutonomyConfig {
            level: AutonomyLevel::Full,
            workspace_only: false,
            allowed_commands: vec!["docker".into()],
            command_context_rules: vec![],
            forbidden_paths: vec!["/secret".into()],
            max_actions_per_hour: 50,
            max_cost_per_day_cents: 1000,
            require_approval_for_medium_risk: false,
            block_high_risk_commands: true,
            shell_env_passthrough: vec!["DATABASE_URL".into()],
            allow_sensitive_file_reads: false,
            allow_sensitive_file_writes: false,
            auto_approve: vec!["file_read".into()],
            always_ask: vec![],
            allowed_roots: vec![],
            non_cli_excluded_tools: vec![],
            non_cli_approval_approvers: vec![],
            non_cli_natural_language_approval_mode:
                NonCliNaturalLanguageApprovalMode::RequestConfirm,
            non_cli_natural_language_approval_mode_by_channel: HashMap::new(),
        },
        security: SecurityConfig::default(),
        runtime: RuntimeConfig {
            kind: "docker".into(),
            ..RuntimeConfig::default()
        },
        research: ResearchPhaseConfig::default(),
        reliability: ReliabilityConfig::default(),
        scheduler: SchedulerConfig::default(),
        coordination: CoordinationConfig::default(),
        skills: SkillsConfig::default(),
        plugins: PluginsConfig::default(),
        model_routes: Vec::new(),
        embedding_routes: Vec::new(),
        query_classification: QueryClassificationConfig::default(),
        heartbeat: HeartbeatConfig {
            enabled: true,
            interval_minutes: 15,
            message: Some("Check London time".into()),
            target: Some("telegram".into()),
            to: Some("123456".into()),
        },
        cron: CronConfig::default(),
        goal_loop: GoalLoopConfig::default(),
        channels_config: ChannelsConfig {
            cli: true,
            acp: None,
            telegram: Some(TelegramConfig {
                bot_token: "123:ABC".into(),
                allowed_users: vec!["user1".into()],
                stream_mode: StreamMode::default(),
                draft_update_interval_ms: 1000,
                interrupt_on_new_message: false,
                mention_only: false,
                progress_mode: ProgressMode::default(),
                ack_enabled: true,
                group_reply: None,
                base_url: None,
            }),
            discord: None,
            slack: None,
            mattermost: None,
            webhook: None,
            imessage: None,
            matrix: None,
            signal: None,
            whatsapp: None,
            linq: None,
            github: None,
            bluebubbles: None,
            wati: None,
            nextcloud_talk: None,
            email: None,
            irc: None,
            lark: None,
            feishu: None,
            dingtalk: None,
            napcat: None,
            qq: None,
            nostr: None,
            clawdtalk: None,
            ack_reaction: AckReactionChannelsConfig::default(),
            message_timeout_secs: 300,
        },
        memory: MemoryConfig::default(),
        storage: StorageConfig::default(),
        tunnel: TunnelConfig::default(),
        gateway: GatewayConfig::default(),
        composio: ComposioConfig::default(),
        secrets: SecretsConfig::default(),
        browser: BrowserConfig::default(),
        http_request: HttpRequestConfig::default(),
        multimodal: MultimodalConfig::default(),
        web_fetch: WebFetchConfig::default(),
        web_search: WebSearchConfig::default(),
        proxy: ProxyConfig::default(),
        agent: AgentConfig::default(),
        identity: IdentityConfig::default(),
        cost: CostConfig::default(),
        economic: EconomicConfig::default(),
        peripherals: PeripheralsConfig::default(),
        agents: HashMap::new(),
        hooks: HooksConfig::default(),
        hardware: HardwareConfig::default(),
        transcription: TranscriptionConfig::default(),
        agents_ipc: AgentsIpcConfig::default(),
        mcp: McpConfig::default(),
        model_support_vision: None,
        wasm: WasmConfig::default(),
    };

    let toml_str = toml::to_string_pretty(&config).unwrap();
    let parsed: Config = toml::from_str(&toml_str).unwrap();

    assert_eq!(parsed.api_key, config.api_key);
    assert_eq!(parsed.default_provider, config.default_provider);
    assert_eq!(parsed.default_model, config.default_model);
    assert!((parsed.default_temperature - config.default_temperature).abs() < f64::EPSILON);
    assert_eq!(parsed.observability.backend, "log");
    assert_eq!(parsed.observability.runtime_trace_mode, "none");
    assert_eq!(parsed.autonomy.level, AutonomyLevel::Full);
    assert!(!parsed.autonomy.workspace_only);
    assert_eq!(parsed.runtime.kind, "docker");
    assert!(parsed.heartbeat.enabled);
    assert_eq!(parsed.heartbeat.interval_minutes, 15);
    assert_eq!(
        parsed.heartbeat.message.as_deref(),
        Some("Check London time")
    );
    assert_eq!(parsed.heartbeat.target.as_deref(), Some("telegram"));
    assert_eq!(parsed.heartbeat.to.as_deref(), Some("123456"));
    assert!(parsed.channels_config.telegram.is_some());
    assert_eq!(
        parsed.channels_config.telegram.unwrap().bot_token,
        "123:ABC"
    );
}

#[test]
async fn config_minimal_toml_uses_defaults() {
    let minimal = r#"
workspace_dir = "/tmp/ws"
config_path = "/tmp/config.toml"
default_temperature = 0.7
"#;
    let parsed: Config = toml::from_str(minimal).unwrap();
    assert!(parsed.api_key.is_none());
    assert!(parsed.default_provider.is_none());
    assert_eq!(parsed.observability.backend, "none");
    assert_eq!(parsed.observability.runtime_trace_mode, "none");
    assert_eq!(parsed.autonomy.level, AutonomyLevel::Supervised);
    assert_eq!(parsed.runtime.kind, "native");
    assert!(!parsed.heartbeat.enabled);
    assert!(parsed.channels_config.cli);
    assert!(parsed.memory.hygiene_enabled);
    assert_eq!(parsed.memory.archive_after_days, 7);
    assert_eq!(parsed.memory.purge_after_days, 30);
    assert_eq!(parsed.memory.conversation_retention_days, 30);
}

#[test]
async fn storage_provider_dburl_alias_deserializes() {
    let raw = r#"
default_temperature = 0.7

[storage.provider.config]
provider = "postgres"
dbURL = "postgres://postgres:postgres@localhost:5432/zeroclaw"
schema = "public"
table = "memories"
connect_timeout_secs = 12
"#;

    let parsed: Config = toml::from_str(raw).unwrap();
    assert_eq!(parsed.storage.provider.config.provider, "postgres");
    assert_eq!(
        parsed.storage.provider.config.db_url.as_deref(),
        Some("postgres://postgres:postgres@localhost:5432/zeroclaw")
    );
    assert_eq!(parsed.storage.provider.config.schema, "public");
    assert_eq!(parsed.storage.provider.config.table, "memories");
    assert_eq!(
        parsed.storage.provider.config.connect_timeout_secs,
        Some(12)
    );
}

#[test]
async fn runtime_reasoning_enabled_deserializes() {
    let raw = r#"
default_temperature = 0.7

[runtime]
reasoning_enabled = false
"#;

    let parsed: Config = toml::from_str(raw).unwrap();
    assert_eq!(parsed.runtime.reasoning_enabled, Some(false));
}

#[test]
async fn runtime_wasm_deserializes() {
    let raw = r#"
default_temperature = 0.7

[runtime]
kind = "wasm"

[runtime.wasm]
tools_dir = "skills/wasm"
fuel_limit = 500000
memory_limit_mb = 32
max_module_size_mb = 8
allow_workspace_read = true
allow_workspace_write = false
allowed_hosts = ["api.example.com", "cdn.example.com:443"]

[runtime.wasm.security]
require_workspace_relative_tools_dir = false
reject_symlink_modules = false
reject_symlink_tools_dir = false
strict_host_validation = false
capability_escalation_mode = "clamp"
module_hash_policy = "enforce"

[runtime.wasm.security.module_sha256]
calc = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
"#;

    let parsed: Config = toml::from_str(raw).unwrap();
    assert_eq!(parsed.runtime.kind, "wasm");
    assert_eq!(parsed.runtime.wasm.tools_dir, "skills/wasm");
    assert_eq!(parsed.runtime.wasm.fuel_limit, 500_000);
    assert_eq!(parsed.runtime.wasm.memory_limit_mb, 32);
    assert_eq!(parsed.runtime.wasm.max_module_size_mb, 8);
    assert!(parsed.runtime.wasm.allow_workspace_read);
    assert!(!parsed.runtime.wasm.allow_workspace_write);
    assert_eq!(
        parsed.runtime.wasm.allowed_hosts,
        vec!["api.example.com", "cdn.example.com:443"]
    );
    assert!(
        !parsed
            .runtime
            .wasm
            .security
            .require_workspace_relative_tools_dir
    );
    assert!(!parsed.runtime.wasm.security.reject_symlink_modules);
    assert!(!parsed.runtime.wasm.security.reject_symlink_tools_dir);
    assert!(!parsed.runtime.wasm.security.strict_host_validation);
    assert_eq!(
        parsed.runtime.wasm.security.capability_escalation_mode,
        WasmCapabilityEscalationMode::Clamp
    );
    assert_eq!(
        parsed.runtime.wasm.security.module_hash_policy,
        WasmModuleHashPolicy::Enforce
    );
    assert_eq!(
        parsed.runtime.wasm.security.module_sha256.get("calc"),
        Some(&"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string())
    );
}

#[test]
async fn runtime_wasm_dev_template_deserializes() {
    let raw = include_str!("../dev/config.wasm.dev.toml");
    let parsed: Config = toml::from_str(raw).expect("dev wasm template should parse");

    assert_eq!(parsed.runtime.kind, "wasm");
    assert!(parsed.runtime.wasm.allow_workspace_read);
    assert!(parsed.runtime.wasm.allow_workspace_write);
    assert_eq!(
        parsed.runtime.wasm.security.capability_escalation_mode,
        WasmCapabilityEscalationMode::Clamp
    );
}

#[test]
async fn runtime_wasm_staging_template_deserializes() {
    let raw = include_str!("../dev/config.wasm.staging.toml");
    let parsed: Config = toml::from_str(raw).expect("staging wasm template should parse");

    assert_eq!(parsed.runtime.kind, "wasm");
    assert!(parsed.runtime.wasm.allow_workspace_read);
    assert!(!parsed.runtime.wasm.allow_workspace_write);
    assert_eq!(
        parsed.runtime.wasm.security.capability_escalation_mode,
        WasmCapabilityEscalationMode::Deny
    );
}

#[test]
async fn runtime_wasm_prod_template_deserializes() {
    let raw = include_str!("../dev/config.wasm.prod.toml");
    let parsed: Config = toml::from_str(raw).expect("prod wasm template should parse");

    assert_eq!(parsed.runtime.kind, "wasm");
    assert!(!parsed.runtime.wasm.allow_workspace_read);
    assert!(!parsed.runtime.wasm.allow_workspace_write);
    assert!(parsed.runtime.wasm.allowed_hosts.is_empty());
    assert_eq!(
        parsed.runtime.wasm.security.capability_escalation_mode,
        WasmCapabilityEscalationMode::Deny
    );
}

#[test]
async fn model_support_vision_deserializes() {
    let raw = r#"
default_temperature = 0.7
model_support_vision = true
"#;

    let parsed: Config = toml::from_str(raw).unwrap();
    assert_eq!(parsed.model_support_vision, Some(true));

    // Default (omitted) should be None
    let raw_no_vision = r#"
default_temperature = 0.7
"#;
    let parsed2: Config = toml::from_str(raw_no_vision).unwrap();
    assert_eq!(parsed2.model_support_vision, None);
}

#[test]
async fn provider_reasoning_level_deserializes() {
    let raw = r#"
default_temperature = 0.7

[provider]
reasoning_level = "high"
"#;

    let parsed: Config = toml::from_str(raw).unwrap();
    assert_eq!(parsed.provider.reasoning_level.as_deref(), Some("high"));
    assert_eq!(
        parsed.effective_provider_reasoning_level().as_deref(),
        Some("high")
    );
}

#[test]
async fn runtime_reasoning_level_alias_deserializes() {
    let raw = r#"
default_temperature = 0.7

[runtime]
reasoning_level = "xhigh"
"#;

    let parsed: Config = toml::from_str(raw).unwrap();
    assert_eq!(parsed.runtime.reasoning_level.as_deref(), Some("xhigh"));
    assert_eq!(
        parsed.effective_provider_reasoning_level().as_deref(),
        Some("xhigh")
    );
}

#[test]
async fn provider_reasoning_level_wins_over_runtime_alias() {
    let raw = r#"
default_temperature = 0.7

[provider]
reasoning_level = "medium"

[runtime]
reasoning_level = "high"
"#;

    let parsed: Config = toml::from_str(raw).unwrap();
    assert_eq!(
        parsed.effective_provider_reasoning_level().as_deref(),
        Some("medium")
    );
}

#[test]
async fn agent_config_defaults() {
    let cfg = AgentConfig::default();
    assert!(cfg.compact_context);
    assert_eq!(cfg.max_tool_iterations, 20);
    assert_eq!(cfg.max_history_messages, 50);
    assert!(!cfg.parallel_tools);
    assert_eq!(cfg.tool_dispatcher, "auto");
    assert!(cfg.allowed_tools.is_empty());
    assert!(cfg.denied_tools.is_empty());
}

#[test]
async fn agent_config_deserializes() {
    let raw = r#"
default_temperature = 0.7
[agent]
compact_context = true
max_tool_iterations = 20
max_history_messages = 80
parallel_tools = true
tool_dispatcher = "xml"
allowed_tools = ["delegate", "task_plan"]
denied_tools = ["shell"]
"#;
    let parsed: Config = toml::from_str(raw).unwrap();
    assert!(parsed.agent.compact_context);
    assert_eq!(parsed.agent.max_tool_iterations, 20);
    assert_eq!(parsed.agent.max_history_messages, 80);
    assert!(parsed.agent.parallel_tools);
    assert_eq!(parsed.agent.tool_dispatcher, "xml");
    assert_eq!(
        parsed.agent.allowed_tools,
        vec!["delegate".to_string(), "task_plan".to_string()]
    );
    assert_eq!(parsed.agent.denied_tools, vec!["shell".to_string()]);
}

#[tokio::test]
async fn config_save_atomic_cleanup() {
    let dir = std::env::temp_dir().join(format!("zeroclaw_test_config_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).await.unwrap();

    let config_path = dir.join("config.toml");
    let mut config = Config::default();
    config.workspace_dir = dir.join("workspace");
    config.config_path = config_path.clone();
    config.default_model = Some("model-a".into());
    config.save().await.unwrap();
    assert!(config_path.exists());

    config.default_model = Some("model-b".into());
    config.save().await.unwrap();

    let contents = tokio::fs::read_to_string(&config_path).await.unwrap();
    assert!(contents.contains("model-b"));

    let names: Vec<String> = ReadDirStream::new(fs::read_dir(&dir).await.unwrap())
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .collect()
        .await;
    assert!(!names.iter().any(|name| name.contains(".tmp-")));
    assert!(!names.iter().any(|name| name.ends_with(".bak")));

    let _ = fs::remove_dir_all(&dir).await;
}

// ── Telegram / Discord config ────────────────────────────

#[test]
async fn telegram_config_serde() {
    let tc = TelegramConfig {
        bot_token: "123:XYZ".into(),
        allowed_users: vec!["alice".into(), "bob".into()],
        stream_mode: StreamMode::Partial,
        draft_update_interval_ms: 500,
        interrupt_on_new_message: true,
        mention_only: false,
        progress_mode: ProgressMode::default(),
        ack_enabled: true,
        group_reply: None,
        base_url: None,
    };
    let json = serde_json::to_string(&tc).unwrap();
    let parsed: TelegramConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.bot_token, "123:XYZ");
    assert_eq!(parsed.allowed_users.len(), 2);
    assert_eq!(parsed.stream_mode, StreamMode::Partial);
    assert_eq!(parsed.draft_update_interval_ms, 500);
    assert!(parsed.interrupt_on_new_message);
}

#[test]
async fn telegram_config_defaults_stream_off() {
    let json = r#"{"bot_token":"tok","allowed_users":[]}"#;
    let parsed: TelegramConfig = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.stream_mode, StreamMode::Off);
    assert_eq!(parsed.progress_mode, ProgressMode::Compact);
    assert_eq!(parsed.draft_update_interval_ms, 1000);
    assert!(!parsed.interrupt_on_new_message);
    assert!(parsed.base_url.is_none());
    assert_eq!(
        parsed.effective_group_reply_mode(),
        GroupReplyMode::AllMessages
    );
    assert!(parsed.group_reply_allowed_sender_ids().is_empty());
}

#[test]
async fn telegram_config_deserializes_stream_mode_on() {
    let json = r#"{"bot_token":"tok","allowed_users":[],"stream_mode":"on"}"#;
    let parsed: TelegramConfig = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.stream_mode, StreamMode::On);
}

#[test]
async fn telegram_config_custom_base_url() {
    let json = r#"{"bot_token":"tok","allowed_users":[],"base_url":"https://tapi.bale.ai"}"#;
    let parsed: TelegramConfig = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.base_url, Some("https://tapi.bale.ai".to_string()));
}

#[test]
async fn progress_mode_deserializes_variants() {
    let verbose: ProgressMode = serde_json::from_str(r#""verbose""#).unwrap();
    let compact: ProgressMode = serde_json::from_str(r#""compact""#).unwrap();
    let off: ProgressMode = serde_json::from_str(r#""off""#).unwrap();

    assert_eq!(verbose, ProgressMode::Verbose);
    assert_eq!(compact, ProgressMode::Compact);
    assert_eq!(off, ProgressMode::Off);
}

#[test]
async fn telegram_config_deserializes_progress_mode_verbose() {
    let json = r#"{"bot_token":"tok","allowed_users":[],"progress_mode":"verbose"}"#;
    let parsed: TelegramConfig = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.progress_mode, ProgressMode::Verbose);
}

#[test]
async fn telegram_config_deserializes_progress_mode_off() {
    let json = r#"{"bot_token":"tok","allowed_users":[],"progress_mode":"off"}"#;
    let parsed: TelegramConfig = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.progress_mode, ProgressMode::Off);
}

#[test]
async fn telegram_group_reply_config_overrides_legacy_mention_only() {
    let json = r#"{
            "bot_token":"tok",
            "allowed_users":["*"],
            "mention_only":false,
            "group_reply":{
                "mode":"mention_only",
                "allowed_sender_ids":["1001","1002"]
            }
        }"#;

    let parsed: TelegramConfig = serde_json::from_str(json).unwrap();
    assert_eq!(
        parsed.effective_group_reply_mode(),
        GroupReplyMode::MentionOnly
    );
    assert_eq!(
        parsed.group_reply_allowed_sender_ids(),
        vec!["1001".to_string(), "1002".to_string()]
    );
}

#[test]
async fn discord_config_serde() {
    let dc = DiscordConfig {
        bot_token: "discord-token".into(),
        guild_id: Some("12345".into()),
        allowed_users: vec![],
        listen_to_bots: false,
        mention_only: false,
        group_reply: None,
    };
    let json = serde_json::to_string(&dc).unwrap();
    let parsed: DiscordConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.bot_token, "discord-token");
    assert_eq!(parsed.guild_id.as_deref(), Some("12345"));
}

#[test]
async fn discord_config_optional_guild() {
    let dc = DiscordConfig {
        bot_token: "tok".into(),
        guild_id: None,
        allowed_users: vec![],
        listen_to_bots: false,
        mention_only: false,
        group_reply: None,
    };
    let json = serde_json::to_string(&dc).unwrap();
    let parsed: DiscordConfig = serde_json::from_str(&json).unwrap();
    assert!(parsed.guild_id.is_none());
}

#[test]
async fn discord_group_reply_mode_falls_back_to_legacy_mention_only() {
    let json = r#"{
            "bot_token":"tok",
            "mention_only":true
        }"#;
    let parsed: DiscordConfig = serde_json::from_str(json).unwrap();
    assert_eq!(
        parsed.effective_group_reply_mode(),
        GroupReplyMode::MentionOnly
    );
    assert!(parsed.group_reply_allowed_sender_ids().is_empty());
}

#[test]
async fn discord_group_reply_mode_overrides_legacy_mention_only() {
    let json = r#"{
            "bot_token":"tok",
            "mention_only":true,
            "group_reply":{
                "mode":"all_messages",
                "allowed_sender_ids":["111"]
            }
        }"#;
    let parsed: DiscordConfig = serde_json::from_str(json).unwrap();
    assert_eq!(
        parsed.effective_group_reply_mode(),
        GroupReplyMode::AllMessages
    );
    assert_eq!(
        parsed.group_reply_allowed_sender_ids(),
        vec!["111".to_string()]
    );
}

// ── iMessage / Matrix config ────────────────────────────

#[test]
async fn imessage_config_serde() {
    let ic = IMessageConfig {
        allowed_contacts: vec!["+1234567890".into(), "user@icloud.com".into()],
    };
    let json = serde_json::to_string(&ic).unwrap();
    let parsed: IMessageConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.allowed_contacts.len(), 2);
    assert_eq!(parsed.allowed_contacts[0], "+1234567890");
}

#[test]
async fn imessage_config_empty_contacts() {
    let ic = IMessageConfig {
        allowed_contacts: vec![],
    };
    let json = serde_json::to_string(&ic).unwrap();
    let parsed: IMessageConfig = serde_json::from_str(&json).unwrap();
    assert!(parsed.allowed_contacts.is_empty());
}

#[test]
async fn imessage_config_wildcard() {
    let ic = IMessageConfig {
        allowed_contacts: vec!["*".into()],
    };
    let toml_str = toml::to_string(&ic).unwrap();
    let parsed: IMessageConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.allowed_contacts, vec!["*"]);
}

#[test]
async fn matrix_config_serde() {
    let mc = MatrixConfig {
        homeserver: "https://matrix.org".into(),
        access_token: "syt_token_abc".into(),
        user_id: Some("@bot:matrix.org".into()),
        device_id: Some("DEVICE123".into()),
        room_id: "!room123:matrix.org".into(),
        allowed_users: vec!["@user:matrix.org".into()],
        mention_only: false,
    };
    let json = serde_json::to_string(&mc).unwrap();
    let parsed: MatrixConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.homeserver, "https://matrix.org");
    assert_eq!(parsed.access_token, "syt_token_abc");
    assert_eq!(parsed.user_id.as_deref(), Some("@bot:matrix.org"));
    assert_eq!(parsed.device_id.as_deref(), Some("DEVICE123"));
    assert_eq!(parsed.room_id, "!room123:matrix.org");
    assert_eq!(parsed.allowed_users.len(), 1);
}

#[test]
async fn matrix_config_toml_roundtrip() {
    let mc = MatrixConfig {
        homeserver: "https://synapse.local:8448".into(),
        access_token: "tok".into(),
        user_id: None,
        device_id: None,
        room_id: "!abc:synapse.local".into(),
        allowed_users: vec!["@admin:synapse.local".into(), "*".into()],
        mention_only: true,
    };
    let toml_str = toml::to_string(&mc).unwrap();
    let parsed: MatrixConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.homeserver, "https://synapse.local:8448");
    assert_eq!(parsed.allowed_users.len(), 2);
}

#[test]
async fn matrix_config_backward_compatible_without_session_hints() {
    let toml = r#"
homeserver = "https://matrix.org"
access_token = "tok"
room_id = "!ops:matrix.org"
allowed_users = ["@ops:matrix.org"]
"#;

    let parsed: MatrixConfig = toml::from_str(toml).unwrap();
    assert_eq!(parsed.homeserver, "https://matrix.org");
    assert!(parsed.user_id.is_none());
    assert!(parsed.device_id.is_none());
    assert!(!parsed.mention_only);
}

#[test]
async fn signal_config_serde() {
    let sc = SignalConfig {
        http_url: "http://127.0.0.1:8686".into(),
        account: "+1234567890".into(),
        group_id: Some("group123".into()),
        allowed_from: vec!["+1111111111".into()],
        ignore_attachments: true,
        ignore_stories: false,
    };
    let json = serde_json::to_string(&sc).unwrap();
    let parsed: SignalConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.http_url, "http://127.0.0.1:8686");
    assert_eq!(parsed.account, "+1234567890");
    assert_eq!(parsed.group_id.as_deref(), Some("group123"));
    assert_eq!(parsed.allowed_from.len(), 1);
    assert!(parsed.ignore_attachments);
    assert!(!parsed.ignore_stories);
}

#[test]
async fn signal_config_toml_roundtrip() {
    let sc = SignalConfig {
        http_url: "http://localhost:8080".into(),
        account: "+9876543210".into(),
        group_id: None,
        allowed_from: vec!["*".into()],
        ignore_attachments: false,
        ignore_stories: true,
    };
    let toml_str = toml::to_string(&sc).unwrap();
    let parsed: SignalConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.http_url, "http://localhost:8080");
    assert_eq!(parsed.account, "+9876543210");
    assert!(parsed.group_id.is_none());
    assert!(parsed.ignore_stories);
}

#[test]
async fn signal_config_defaults() {
    let json = r#"{"http_url":"http://127.0.0.1:8686","account":"+1234567890"}"#;
    let parsed: SignalConfig = serde_json::from_str(json).unwrap();
    assert!(parsed.group_id.is_none());
    assert!(parsed.allowed_from.is_empty());
    assert!(!parsed.ignore_attachments);
    assert!(!parsed.ignore_stories);
}

#[test]
async fn channels_config_with_imessage_and_matrix() {
    let c = ChannelsConfig {
        cli: true,
        acp: None,
        telegram: None,
        discord: None,
        slack: None,
        mattermost: None,
        webhook: None,
        imessage: Some(IMessageConfig {
            allowed_contacts: vec!["+1".into()],
        }),
        matrix: Some(MatrixConfig {
            homeserver: "https://m.org".into(),
            access_token: "tok".into(),
            user_id: None,
            device_id: None,
            room_id: "!r:m".into(),
            allowed_users: vec!["@u:m".into()],
            mention_only: false,
        }),
        signal: None,
        whatsapp: None,
        linq: None,
        github: None,
        bluebubbles: None,
        wati: None,
        nextcloud_talk: None,
        email: None,
        irc: None,
        lark: None,
        feishu: None,
        dingtalk: None,
        napcat: None,
        qq: None,
        nostr: None,
        clawdtalk: None,
        ack_reaction: AckReactionChannelsConfig::default(),
        message_timeout_secs: 300,
    };
    let toml_str = toml::to_string_pretty(&c).unwrap();
    let parsed: ChannelsConfig = toml::from_str(&toml_str).unwrap();
    assert!(parsed.imessage.is_some());
    assert!(parsed.matrix.is_some());
    assert_eq!(parsed.imessage.unwrap().allowed_contacts, vec!["+1"]);
    assert_eq!(parsed.matrix.unwrap().homeserver, "https://m.org");
}

#[test]
async fn channels_config_default_has_no_imessage_matrix() {
    let c = ChannelsConfig::default();
    assert!(c.imessage.is_none());
    assert!(c.matrix.is_none());
}

#[test]
async fn channels_ack_reaction_config_roundtrip() {
    let c = ChannelsConfig {
        ack_reaction: AckReactionChannelsConfig {
            telegram: Some(AckReactionConfig {
                enabled: true,
                strategy: AckReactionStrategy::First,
                sample_rate: 0.8,
                emojis: vec!["✅".into(), "👍".into()],
                rules: vec![AckReactionRuleConfig {
                    enabled: true,
                    contains_any: vec!["deploy".into()],
                    contains_all: vec!["ok".into()],
                    contains_none: vec!["dry-run".into()],
                    regex_any: vec![r"deploy\s+ok".into()],
                    regex_all: Vec::new(),
                    regex_none: vec![r"panic|fatal".into()],
                    sender_ids: vec!["u123".into()],
                    chat_ids: vec!["-100200300".into()],
                    chat_types: vec![AckReactionChatType::Group],
                    locale_any: vec!["en".into()],
                    action: AckReactionRuleAction::React,
                    sample_rate: Some(0.5),
                    strategy: Some(AckReactionStrategy::Random),
                    emojis: vec!["🚀".into()],
                }],
            }),
            discord: None,
            lark: None,
            feishu: None,
        },
        ..ChannelsConfig::default()
    };

    let toml_str = toml::to_string_pretty(&c).unwrap();
    let parsed: ChannelsConfig = toml::from_str(&toml_str).unwrap();
    let telegram = parsed.ack_reaction.telegram.expect("telegram ack config");
    assert!(telegram.enabled);
    assert_eq!(telegram.strategy, AckReactionStrategy::First);
    assert_eq!(telegram.sample_rate, 0.8);
    assert_eq!(telegram.emojis, vec!["✅", "👍"]);
    assert_eq!(telegram.rules.len(), 1);
    let first_rule = &telegram.rules[0];
    assert_eq!(first_rule.contains_any, vec!["deploy"]);
    assert_eq!(first_rule.contains_none, vec!["dry-run"]);
    assert_eq!(first_rule.regex_any, vec![r"deploy\s+ok"]);
    assert_eq!(first_rule.chat_ids, vec!["-100200300"]);
    assert_eq!(first_rule.action, AckReactionRuleAction::React);
    assert_eq!(first_rule.sample_rate, Some(0.5));
    assert_eq!(first_rule.chat_types, vec![AckReactionChatType::Group]);
}

#[test]
async fn channels_ack_reaction_defaults_empty() {
    let parsed: ChannelsConfig = toml::from_str("cli = true").unwrap();
    assert!(parsed.ack_reaction.telegram.is_none());
    assert!(parsed.ack_reaction.discord.is_none());
    assert!(parsed.ack_reaction.lark.is_none());
    assert!(parsed.ack_reaction.feishu.is_none());
}

// ── Edge cases: serde(default) for allowed_users ─────────

#[test]
async fn discord_config_deserializes_without_allowed_users() {
    // Old configs won't have allowed_users — serde(default) should fill vec![]
    let json = r#"{"bot_token":"tok","guild_id":"123"}"#;
    let parsed: DiscordConfig = serde_json::from_str(json).unwrap();
    assert!(parsed.allowed_users.is_empty());
}

#[test]
async fn discord_config_deserializes_with_allowed_users() {
    let json = r#"{"bot_token":"tok","guild_id":"123","allowed_users":["111","222"]}"#;
    let parsed: DiscordConfig = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.allowed_users, vec!["111", "222"]);
}

#[test]
async fn slack_config_deserializes_without_allowed_users() {
    let json = r#"{"bot_token":"xoxb-tok"}"#;
    let parsed: SlackConfig = serde_json::from_str(json).unwrap();
    assert!(parsed.channel_ids.is_empty());
    assert!(parsed.allowed_users.is_empty());
    assert_eq!(
        parsed.effective_group_reply_mode(),
        GroupReplyMode::AllMessages
    );
}

#[test]
async fn slack_config_deserializes_with_allowed_users() {
    let json = r#"{"bot_token":"xoxb-tok","allowed_users":["U111"]}"#;
    let parsed: SlackConfig = serde_json::from_str(json).unwrap();
    assert!(parsed.channel_ids.is_empty());
    assert_eq!(parsed.allowed_users, vec!["U111"]);
}

#[test]
async fn discord_config_toml_backward_compat() {
    let toml_str = r#"
bot_token = "tok"
guild_id = "123"
"#;
    let parsed: DiscordConfig = toml::from_str(toml_str).unwrap();
    assert!(parsed.allowed_users.is_empty());
    assert_eq!(parsed.bot_token, "tok");
}

#[test]
async fn slack_config_toml_backward_compat() {
    let toml_str = r#"
bot_token = "xoxb-tok"
channel_id = "C123"
"#;
    let parsed: SlackConfig = toml::from_str(toml_str).unwrap();
    assert!(parsed.channel_ids.is_empty());
    assert!(parsed.allowed_users.is_empty());
    assert_eq!(parsed.channel_id.as_deref(), Some("C123"));
    assert_eq!(
        parsed.effective_group_reply_mode(),
        GroupReplyMode::AllMessages
    );
}

#[test]
async fn slack_group_reply_config_supports_sender_overrides() {
    let json = r#"{
            "bot_token":"xoxb-tok",
            "group_reply":{
                "mode":"mention_only",
                "allowed_sender_ids":["U111"]
            }
        }"#;
    let parsed: SlackConfig = serde_json::from_str(json).unwrap();
    assert_eq!(
        parsed.effective_group_reply_mode(),
        GroupReplyMode::MentionOnly
    );
    assert_eq!(
        parsed.group_reply_allowed_sender_ids(),
        vec!["U111".to_string()]
    );
}

#[test]
async fn channels_slack_group_reply_toml_nested_table_deserializes() {
    let toml_str = r#"
cli = true

[slack]
bot_token = "xoxb-tok"
app_token = "xapp-tok"
channel_id = "C123"
allowed_users = ["*"]

[slack.group_reply]
mode = "mention_only"
allowed_sender_ids = ["U111", "U222"]
"#;
    let parsed: ChannelsConfig = toml::from_str(toml_str).unwrap();
    let slack = parsed.slack.expect("slack config should exist");
    assert_eq!(
        slack.effective_group_reply_mode(),
        GroupReplyMode::MentionOnly
    );
    assert_eq!(
        slack.group_reply_allowed_sender_ids(),
        vec!["U111".to_string(), "U222".to_string()]
    );
}

#[test]
async fn mattermost_group_reply_mode_falls_back_to_legacy_mention_only() {
    let json = r#"{
            "url":"https://mm.example.com",
            "bot_token":"token",
            "mention_only":true
        }"#;
    let parsed: MattermostConfig = serde_json::from_str(json).unwrap();
    assert_eq!(
        parsed.effective_group_reply_mode(),
        GroupReplyMode::MentionOnly
    );
}

#[test]
async fn mattermost_group_reply_mode_overrides_legacy_mention_only() {
    let json = r#"{
            "url":"https://mm.example.com",
            "bot_token":"token",
            "mention_only":true,
            "group_reply":{
                "mode":"all_messages",
                "allowed_sender_ids":["u1","u2"]
            }
        }"#;
    let parsed: MattermostConfig = serde_json::from_str(json).unwrap();
    assert_eq!(
        parsed.effective_group_reply_mode(),
        GroupReplyMode::AllMessages
    );
    assert_eq!(
        parsed.group_reply_allowed_sender_ids(),
        vec!["u1".to_string(), "u2".to_string()]
    );
}

#[test]
async fn webhook_config_with_secret() {
    let json = r#"{"port":8080,"secret":"my-secret-key"}"#;
    let parsed: WebhookConfig = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.secret.as_deref(), Some("my-secret-key"));
}

#[test]
async fn webhook_config_without_secret() {
    let json = r#"{"port":8080}"#;
    let parsed: WebhookConfig = serde_json::from_str(json).unwrap();
    assert!(parsed.secret.is_none());
    assert_eq!(parsed.port, 8080);
}

// ── WhatsApp config ──────────────────────────────────────

#[test]
async fn whatsapp_config_serde() {
    let wc = WhatsAppConfig {
        access_token: Some("EAABx...".into()),
        phone_number_id: Some("123456789".into()),
        verify_token: Some("my-verify-token".into()),
        app_secret: None,
        session_path: None,
        pair_phone: None,
        pair_code: None,
        allowed_numbers: vec!["+1234567890".into(), "+9876543210".into()],
    };
    let json = serde_json::to_string(&wc).unwrap();
    let parsed: WhatsAppConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.access_token, Some("EAABx...".into()));
    assert_eq!(parsed.phone_number_id, Some("123456789".into()));
    assert_eq!(parsed.verify_token, Some("my-verify-token".into()));
    assert_eq!(parsed.allowed_numbers.len(), 2);
}

#[test]
async fn whatsapp_config_toml_roundtrip() {
    let wc = WhatsAppConfig {
        access_token: Some("tok".into()),
        phone_number_id: Some("12345".into()),
        verify_token: Some("verify".into()),
        app_secret: Some("secret123".into()),
        session_path: None,
        pair_phone: None,
        pair_code: None,
        allowed_numbers: vec!["+1".into()],
    };
    let toml_str = toml::to_string(&wc).unwrap();
    let parsed: WhatsAppConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.phone_number_id, Some("12345".into()));
    assert_eq!(parsed.allowed_numbers, vec!["+1"]);
}

#[test]
async fn whatsapp_config_deserializes_without_allowed_numbers() {
    let json = r#"{"access_token":"tok","phone_number_id":"123","verify_token":"ver"}"#;
    let parsed: WhatsAppConfig = serde_json::from_str(json).unwrap();
    assert!(parsed.allowed_numbers.is_empty());
}

#[test]
async fn whatsapp_config_wildcard_allowed() {
    let wc = WhatsAppConfig {
        access_token: Some("tok".into()),
        phone_number_id: Some("123".into()),
        verify_token: Some("ver".into()),
        app_secret: None,
        session_path: None,
        pair_phone: None,
        pair_code: None,
        allowed_numbers: vec!["*".into()],
    };
    let toml_str = toml::to_string(&wc).unwrap();
    let parsed: WhatsAppConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.allowed_numbers, vec!["*"]);
}

#[test]
async fn whatsapp_config_backend_type_cloud_precedence_when_ambiguous() {
    let wc = WhatsAppConfig {
        access_token: Some("tok".into()),
        phone_number_id: Some("123".into()),
        verify_token: Some("ver".into()),
        app_secret: None,
        session_path: Some("~/.zeroclaw/state/whatsapp-web/session.db".into()),
        pair_phone: None,
        pair_code: None,
        allowed_numbers: vec!["+1".into()],
    };
    assert!(wc.is_ambiguous_config());
    assert_eq!(wc.backend_type(), "cloud");
}

#[test]
async fn whatsapp_config_backend_type_web() {
    let wc = WhatsAppConfig {
        access_token: None,
        phone_number_id: None,
        verify_token: None,
        app_secret: None,
        session_path: Some("~/.zeroclaw/state/whatsapp-web/session.db".into()),
        pair_phone: None,
        pair_code: None,
        allowed_numbers: vec![],
    };
    assert!(!wc.is_ambiguous_config());
    assert_eq!(wc.backend_type(), "web");
}

#[test]
async fn channels_config_with_whatsapp() {
    let c = ChannelsConfig {
        cli: true,
        acp: None,
        telegram: None,
        discord: None,
        slack: None,
        mattermost: None,
        webhook: None,
        imessage: None,
        matrix: None,
        signal: None,
        whatsapp: Some(WhatsAppConfig {
            access_token: Some("tok".into()),
            phone_number_id: Some("123".into()),
            verify_token: Some("ver".into()),
            app_secret: None,
            session_path: None,
            pair_phone: None,
            pair_code: None,
            allowed_numbers: vec!["+1".into()],
        }),
        linq: None,
        github: None,
        bluebubbles: None,
        wati: None,
        nextcloud_talk: None,
        email: None,
        irc: None,
        lark: None,
        feishu: None,
        dingtalk: None,
        napcat: None,
        qq: None,
        nostr: None,
        clawdtalk: None,
        ack_reaction: AckReactionChannelsConfig::default(),
        message_timeout_secs: 300,
    };
    let toml_str = toml::to_string_pretty(&c).unwrap();
    let parsed: ChannelsConfig = toml::from_str(&toml_str).unwrap();
    assert!(parsed.whatsapp.is_some());
    let wa = parsed.whatsapp.unwrap();
    assert_eq!(wa.phone_number_id, Some("123".into()));
    assert_eq!(wa.allowed_numbers, vec!["+1"]);
}

#[test]
async fn channels_config_default_has_no_whatsapp() {
    let c = ChannelsConfig::default();
    assert!(c.whatsapp.is_none());
}

#[test]
async fn channels_config_default_has_no_nextcloud_talk() {
    let c = ChannelsConfig::default();
    assert!(c.nextcloud_talk.is_none());
}

// ══════════════════════════════════════════════════════════
// SECURITY CHECKLIST TESTS — Gateway config
// ══════════════════════════════════════════════════════════

#[test]
async fn checklist_gateway_default_requires_pairing() {
    let g = GatewayConfig::default();
    assert!(g.require_pairing, "Pairing must be required by default");
}

#[test]
async fn checklist_gateway_default_blocks_public_bind() {
    let g = GatewayConfig::default();
    assert!(
        !g.allow_public_bind,
        "Public bind must be blocked by default"
    );
}

#[test]
async fn checklist_gateway_default_no_tokens() {
    let g = GatewayConfig::default();
    assert!(
        g.paired_tokens.is_empty(),
        "No pre-paired tokens by default"
    );
    assert_eq!(g.pair_rate_limit_per_minute, 10);
    assert_eq!(g.webhook_rate_limit_per_minute, 60);
    assert!(!g.trust_forwarded_headers);
    assert_eq!(g.rate_limit_max_keys, 10_000);
    assert_eq!(g.idempotency_ttl_secs, 300);
    assert_eq!(g.idempotency_max_keys, 10_000);
    assert!(!g.node_control.enabled);
    assert!(g.node_control.auth_token.is_none());
    assert!(g.node_control.allowed_node_ids.is_empty());
}

#[test]
async fn checklist_gateway_cli_default_host_is_localhost() {
    // The CLI default for --host is 127.0.0.1 (checked in main.rs)
    // Here we verify the config default matches
    let c = Config::default();
    assert!(
        c.gateway.require_pairing,
        "Config default must require pairing"
    );
    assert!(
        !c.gateway.allow_public_bind,
        "Config default must block public bind"
    );
}

#[test]
async fn checklist_gateway_serde_roundtrip() {
    let g = GatewayConfig {
        port: 42617,
        host: "127.0.0.1".into(),
        require_pairing: true,
        allow_public_bind: false,
        paired_tokens: vec!["zc_test_token".into()],
        pair_rate_limit_per_minute: 12,
        webhook_rate_limit_per_minute: 80,
        trust_forwarded_headers: true,
        rate_limit_max_keys: 2048,
        idempotency_ttl_secs: 600,
        idempotency_max_keys: 4096,
        node_control: NodeControlConfig {
            enabled: true,
            auth_token: Some("node-token".into()),
            allowed_node_ids: vec!["node-1".into(), "node-2".into()],
        },
    };
    let toml_str = toml::to_string(&g).unwrap();
    let parsed: GatewayConfig = toml::from_str(&toml_str).unwrap();
    assert!(parsed.require_pairing);
    assert!(!parsed.allow_public_bind);
    assert_eq!(parsed.paired_tokens, vec!["zc_test_token"]);
    assert_eq!(parsed.pair_rate_limit_per_minute, 12);
    assert_eq!(parsed.webhook_rate_limit_per_minute, 80);
    assert!(parsed.trust_forwarded_headers);
    assert_eq!(parsed.rate_limit_max_keys, 2048);
    assert_eq!(parsed.idempotency_ttl_secs, 600);
    assert_eq!(parsed.idempotency_max_keys, 4096);
    assert!(parsed.node_control.enabled);
    assert_eq!(
        parsed.node_control.auth_token.as_deref(),
        Some("node-token")
    );
    assert_eq!(
        parsed.node_control.allowed_node_ids,
        vec!["node-1", "node-2"]
    );
}

#[test]
async fn checklist_gateway_backward_compat_no_gateway_section() {
    // Old configs without [gateway] should get secure defaults
    let minimal = r#"
workspace_dir = "/tmp/ws"
config_path = "/tmp/config.toml"
default_temperature = 0.7
"#;
    let parsed: Config = toml::from_str(minimal).unwrap();
    assert!(
        parsed.gateway.require_pairing,
        "Missing [gateway] must default to require_pairing=true"
    );
    assert!(
        !parsed.gateway.allow_public_bind,
        "Missing [gateway] must default to allow_public_bind=false"
    );
}

#[test]
async fn checklist_autonomy_default_is_workspace_scoped() {
    let a = AutonomyConfig::default();
    // Public contract: `/mnt` is blocked by default for safer host isolation.
    // Rollback path remains explicit user override via `autonomy.forbidden_paths`.
    assert!(a.workspace_only, "Default autonomy must be workspace_only");
    assert!(
        a.forbidden_paths.contains(&"/etc".to_string()),
        "Must block /etc"
    );
    assert!(
        a.forbidden_paths.contains(&"/proc".to_string()),
        "Must block /proc"
    );
    assert!(
        a.forbidden_paths.contains(&"/mnt".to_string()),
        "Must block /mnt"
    );
    assert!(
        a.forbidden_paths.contains(&"~/.ssh".to_string()),
        "Must block ~/.ssh"
    );
}

// ══════════════════════════════════════════════════════════
// COMPOSIO CONFIG TESTS
// ══════════════════════════════════════════════════════════

#[test]
async fn composio_config_default_disabled() {
    let c = ComposioConfig::default();
    assert!(!c.enabled, "Composio must be disabled by default");
    assert!(c.api_key.is_none(), "No API key by default");
    assert_eq!(c.entity_id, "default");
}

#[test]
async fn composio_config_serde_roundtrip() {
    let c = ComposioConfig {
        enabled: true,
        api_key: Some("comp-key-123".into()),
        entity_id: "user42".into(),
    };
    let toml_str = toml::to_string(&c).unwrap();
    let parsed: ComposioConfig = toml::from_str(&toml_str).unwrap();
    assert!(parsed.enabled);
    assert_eq!(parsed.api_key.as_deref(), Some("comp-key-123"));
    assert_eq!(parsed.entity_id, "user42");
}

#[test]
async fn composio_config_backward_compat_missing_section() {
    let minimal = r#"
workspace_dir = "/tmp/ws"
config_path = "/tmp/config.toml"
default_temperature = 0.7
"#;
    let parsed: Config = toml::from_str(minimal).unwrap();
    assert!(
        !parsed.composio.enabled,
        "Missing [composio] must default to disabled"
    );
    assert!(parsed.composio.api_key.is_none());
}

#[test]
async fn composio_config_partial_toml() {
    let toml_str = r"
enabled = true
";
    let parsed: ComposioConfig = toml::from_str(toml_str).unwrap();
    assert!(parsed.enabled);
    assert!(parsed.api_key.is_none());
    assert_eq!(parsed.entity_id, "default");
}

#[test]
async fn composio_config_enable_alias_supported() {
    let toml_str = r"
enable = true
";
    let parsed: ComposioConfig = toml::from_str(toml_str).unwrap();
    assert!(parsed.enabled);
    assert!(parsed.api_key.is_none());
    assert_eq!(parsed.entity_id, "default");
}

#[test]
async fn config_default_has_composio_and_secrets() {
    let c = Config::default();
    assert!(!c.composio.enabled);
    assert!(c.composio.api_key.is_none());
    assert!(c.secrets.encrypt);
    assert!(!c.browser.enabled);
    assert!(c.browser.allowed_domains.is_empty());
}

#[test]
async fn browser_config_default_disabled() {
    let b = BrowserConfig::default();
    assert!(!b.enabled);
    assert!(b.allowed_domains.is_empty());
    assert_eq!(b.backend, "agent_browser");
    assert!(b.auto_backend_priority.is_empty());
    assert_eq!(b.agent_browser_command, "agent-browser");
    assert!(b.agent_browser_extra_args.is_empty());
    assert_eq!(b.agent_browser_timeout_ms, 30_000);
    assert!(b.native_headless);
    assert_eq!(b.native_webdriver_url, "http://127.0.0.1:9515");
    assert!(b.native_chrome_path.is_none());
    assert_eq!(b.computer_use.endpoint, "http://127.0.0.1:8787/v1/actions");
    assert_eq!(b.computer_use.timeout_ms, 15_000);
    assert!(!b.computer_use.allow_remote_endpoint);
    assert!(b.computer_use.window_allowlist.is_empty());
    assert!(b.computer_use.max_coordinate_x.is_none());
    assert!(b.computer_use.max_coordinate_y.is_none());
}

#[test]
async fn browser_config_serde_roundtrip() {
    let b = BrowserConfig {
        enabled: true,
        allowed_domains: vec!["example.com".into(), "docs.example.com".into()],
        browser_open: "chrome".into(),
        session_name: None,
        backend: "auto".into(),
        auto_backend_priority: vec!["rust_native".into(), "agent_browser".into()],
        agent_browser_command: "/usr/local/bin/agent-browser".into(),
        agent_browser_extra_args: vec!["--sandbox".into(), "--trace".into()],
        agent_browser_timeout_ms: 45_000,
        native_headless: false,
        native_webdriver_url: "http://localhost:4444".into(),
        native_chrome_path: Some("/usr/bin/chromium".into()),
        computer_use: BrowserComputerUseConfig {
            endpoint: "https://computer-use.example.com/v1/actions".into(),
            api_key: Some("test-token".into()),
            timeout_ms: 8_000,
            allow_remote_endpoint: true,
            window_allowlist: vec!["Chrome".into(), "Visual Studio Code".into()],
            max_coordinate_x: Some(3840),
            max_coordinate_y: Some(2160),
        },
    };
    let toml_str = toml::to_string(&b).unwrap();
    let parsed: BrowserConfig = toml::from_str(&toml_str).unwrap();
    assert!(parsed.enabled);
    assert_eq!(parsed.allowed_domains.len(), 2);
    assert_eq!(parsed.allowed_domains[0], "example.com");
    assert_eq!(parsed.backend, "auto");
    assert_eq!(
        parsed.auto_backend_priority,
        vec!["rust_native".to_string(), "agent_browser".to_string()]
    );
    assert_eq!(parsed.agent_browser_command, "/usr/local/bin/agent-browser");
    assert_eq!(
        parsed.agent_browser_extra_args,
        vec!["--sandbox".to_string(), "--trace".to_string()]
    );
    assert_eq!(parsed.agent_browser_timeout_ms, 45_000);
    assert!(!parsed.native_headless);
    assert_eq!(parsed.native_webdriver_url, "http://localhost:4444");
    assert_eq!(
        parsed.native_chrome_path.as_deref(),
        Some("/usr/bin/chromium")
    );
    assert_eq!(
        parsed.computer_use.endpoint,
        "https://computer-use.example.com/v1/actions"
    );
    assert_eq!(parsed.computer_use.api_key.as_deref(), Some("test-token"));
    assert_eq!(parsed.computer_use.timeout_ms, 8_000);
    assert!(parsed.computer_use.allow_remote_endpoint);
    assert_eq!(parsed.computer_use.window_allowlist.len(), 2);
    assert_eq!(parsed.computer_use.max_coordinate_x, Some(3840));
    assert_eq!(parsed.computer_use.max_coordinate_y, Some(2160));
}

#[test]
async fn browser_config_backward_compat_missing_section() {
    let minimal = r#"
workspace_dir = "/tmp/ws"
config_path = "/tmp/config.toml"
default_temperature = 0.7
"#;
    let parsed: Config = toml::from_str(minimal).unwrap();
    assert!(!parsed.browser.enabled);
    assert!(parsed.browser.allowed_domains.is_empty());
}

#[test]
async fn web_search_config_default_extended_fields() {
    let ws = WebSearchConfig::default();
    assert_eq!(ws.provider, "duckduckgo");
    assert!(ws.fallback_providers.is_empty());
    assert_eq!(ws.retries_per_provider, 0);
    assert_eq!(ws.retry_backoff_ms, 250);
    assert!(ws.domain_filter.is_empty());
    assert!(ws.language_filter.is_empty());
    assert!(ws.country.is_none());
    assert!(ws.recency_filter.is_none());
    assert!(ws.max_tokens.is_none());
    assert!(ws.max_tokens_per_page.is_none());
    assert_eq!(ws.exa_search_type, "auto");
    assert!(!ws.exa_include_text);
    assert!(ws.jina_site_filters.is_empty());
}

#[test]
async fn config_validate_rejects_unknown_browser_open_value() {
    let mut config = Config::default();
    config.browser.browser_open = "safari".into();

    let error = config
        .validate()
        .expect_err("expected browser.browser_open validation failure");
    assert!(error.to_string().contains("browser.browser_open"));
}

#[test]
async fn config_validate_rejects_unknown_browser_backend_value() {
    let mut config = Config::default();
    config.browser.backend = "playwright".into();

    let error = config
        .validate()
        .expect_err("expected browser.backend validation failure");
    assert!(error.to_string().contains("browser.backend"));
}

#[test]
async fn config_validate_rejects_invalid_auto_backend_priority_value() {
    let mut config = Config::default();
    config.browser.backend = "auto".into();
    config.browser.auto_backend_priority = vec!["auto".into()];

    let error = config
        .validate()
        .expect_err("expected browser.auto_backend_priority validation failure");
    assert!(error
        .to_string()
        .contains("browser.auto_backend_priority[0]"));
}

#[test]
async fn config_validate_accepts_web_search_ddg_alias() {
    let mut config = Config::default();
    config.web_search.provider = "ddg".into();
    config.web_search.fallback_providers = vec!["jina".into()];

    config
        .validate()
        .expect("ddg alias should be accepted for web_search.provider");
}

#[test]
async fn config_validate_rejects_unknown_web_search_provider() {
    let mut config = Config::default();
    config.web_search.provider = "serpapi".into();

    let error = config
        .validate()
        .expect_err("expected web_search.provider validation failure");
    assert!(error.to_string().contains("web_search.provider"));
}

#[test]
async fn config_validate_rejects_unknown_web_search_fallback_provider() {
    let mut config = Config::default();
    config.web_search.fallback_providers = vec!["serpapi".into()];

    let error = config
        .validate()
        .expect_err("expected web_search.fallback_providers validation failure");
    assert!(error
        .to_string()
        .contains("web_search.fallback_providers[0]"));
}

#[test]
async fn config_validate_rejects_invalid_web_search_exa_search_type() {
    let mut config = Config::default();
    config.web_search.exa_search_type = "semantic".into();

    let error = config
        .validate()
        .expect_err("expected web_search.exa_search_type validation failure");
    assert!(error.to_string().contains("web_search.exa_search_type"));
}

#[test]
async fn config_validate_rejects_web_search_out_of_range_values() {
    let mut config = Config::default();
    config.web_search.max_results = 11;

    let error = config
        .validate()
        .expect_err("expected web_search.max_results validation failure");
    assert!(error.to_string().contains("web_search.max_results"));
}

#[test]
async fn config_validate_rejects_web_search_excessive_retries() {
    let mut config = Config::default();
    config.web_search.retries_per_provider = 6;

    let error = config
        .validate()
        .expect_err("expected web_search.retries_per_provider validation failure");
    assert!(error
        .to_string()
        .contains("web_search.retries_per_provider"));
}

// ── Environment variable overrides (Docker support) ─────────

async fn env_override_lock() -> MutexGuard<'static, ()> {
    static ENV_OVERRIDE_TEST_LOCK: Mutex<()> = Mutex::const_new(());
    ENV_OVERRIDE_TEST_LOCK.lock().await
}

fn clear_proxy_env_test_vars() {
    for key in [
        "ZEROCLAW_PROXY_ENABLED",
        "ZEROCLAW_HTTP_PROXY",
        "ZEROCLAW_HTTPS_PROXY",
        "ZEROCLAW_ALL_PROXY",
        "ZEROCLAW_NO_PROXY",
        "ZEROCLAW_PROXY_SCOPE",
        "ZEROCLAW_PROXY_SERVICES",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
    ] {
        std::env::remove_var(key);
    }
}

#[test]
async fn env_override_api_key() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();
    assert!(config.api_key.is_none());

    std::env::set_var("ZEROCLAW_API_KEY", "sk-test-env-key");
    config.apply_env_overrides();
    assert_eq!(config.api_key.as_deref(), Some("sk-test-env-key"));

    std::env::remove_var("ZEROCLAW_API_KEY");
}

#[test]
async fn env_override_api_key_fallback() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();

    std::env::remove_var("ZEROCLAW_API_KEY");
    std::env::set_var("API_KEY", "sk-fallback-key");
    config.apply_env_overrides();
    assert_eq!(config.api_key.as_deref(), Some("sk-fallback-key"));

    std::env::remove_var("API_KEY");
}

#[test]
async fn env_override_api_key_generic_does_not_override_config() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();
    config.api_key = Some("sk-config-key".to_string());

    std::env::remove_var("ZEROCLAW_API_KEY");
    std::env::set_var("API_KEY", "sk-generic-env-key");
    config.apply_env_overrides();
    // Generic API_KEY must NOT override an existing config key
    assert_eq!(config.api_key.as_deref(), Some("sk-config-key"));

    std::env::remove_var("API_KEY");
}

#[test]
async fn env_override_zeroclaw_api_key_overrides_config() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();
    config.api_key = Some("sk-config-key".to_string());

    std::env::set_var("ZEROCLAW_API_KEY", "sk-explicit-env-key");
    config.apply_env_overrides();
    // ZEROCLAW_API_KEY should always win, even over config
    assert_eq!(config.api_key.as_deref(), Some("sk-explicit-env-key"));

    std::env::remove_var("ZEROCLAW_API_KEY");
}

#[test]
async fn env_override_provider() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();

    std::env::set_var("ZEROCLAW_PROVIDER", "anthropic");
    config.apply_env_overrides();
    assert_eq!(config.default_provider.as_deref(), Some("anthropic"));

    std::env::remove_var("ZEROCLAW_PROVIDER");
}

#[test]
async fn env_override_model_provider_alias() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();

    std::env::remove_var("ZEROCLAW_PROVIDER");
    std::env::set_var("ZEROCLAW_MODEL_PROVIDER", "openai-codex");
    config.apply_env_overrides();
    assert_eq!(config.default_provider.as_deref(), Some("openai-codex"));

    std::env::remove_var("ZEROCLAW_MODEL_PROVIDER");
}

#[test]
async fn toml_supports_model_provider_and_model_alias_fields() {
    let raw = r#"
default_temperature = 0.7
model_provider = "sub2api"
model = "gpt-5.3-codex"

[model_providers.sub2api]
name = "sub2api"
base_url = "https://api.tonsof.blue/v1"
wire_api = "responses"
model = "gpt-5.3-codex"
api_key = "profile-key"
requires_openai_auth = true
"#;

    let parsed: Config = toml::from_str(raw).expect("config should parse");
    assert_eq!(parsed.default_provider.as_deref(), Some("sub2api"));
    assert_eq!(parsed.default_model.as_deref(), Some("gpt-5.3-codex"));
    let profile = parsed
        .model_providers
        .get("sub2api")
        .expect("profile should exist");
    assert_eq!(profile.wire_api.as_deref(), Some("responses"));
    assert_eq!(profile.default_model.as_deref(), Some("gpt-5.3-codex"));
    assert_eq!(profile.api_key.as_deref(), Some("profile-key"));
    assert!(profile.requires_openai_auth);
}

#[test]
async fn env_override_open_skills_enabled_and_dir() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();
    assert!(!config.skills.open_skills_enabled);
    assert!(!config.skills.allow_scripts);
    assert!(config.skills.open_skills_dir.is_none());
    assert_eq!(
        config.skills.prompt_injection_mode,
        SkillsPromptInjectionMode::Compact
    );

    std::env::set_var("ZEROCLAW_OPEN_SKILLS_ENABLED", "true");
    std::env::set_var("ZEROCLAW_OPEN_SKILLS_DIR", "/tmp/open-skills");
    std::env::set_var("ZEROCLAW_SKILLS_ALLOW_SCRIPTS", "yes");
    std::env::set_var("ZEROCLAW_SKILLS_PROMPT_MODE", "compact");
    config.apply_env_overrides();

    assert!(config.skills.open_skills_enabled);
    assert!(config.skills.allow_scripts);
    assert_eq!(
        config.skills.open_skills_dir.as_deref(),
        Some("/tmp/open-skills")
    );
    assert_eq!(
        config.skills.prompt_injection_mode,
        SkillsPromptInjectionMode::Compact
    );

    std::env::remove_var("ZEROCLAW_OPEN_SKILLS_ENABLED");
    std::env::remove_var("ZEROCLAW_OPEN_SKILLS_DIR");
    std::env::remove_var("ZEROCLAW_SKILLS_ALLOW_SCRIPTS");
    std::env::remove_var("ZEROCLAW_SKILLS_PROMPT_MODE");
}

#[test]
async fn env_override_open_skills_enabled_invalid_value_keeps_existing_value() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();
    config.skills.open_skills_enabled = true;
    config.skills.allow_scripts = true;
    config.skills.prompt_injection_mode = SkillsPromptInjectionMode::Compact;

    std::env::set_var("ZEROCLAW_OPEN_SKILLS_ENABLED", "maybe");
    std::env::set_var("ZEROCLAW_SKILLS_ALLOW_SCRIPTS", "maybe");
    std::env::set_var("ZEROCLAW_SKILLS_PROMPT_MODE", "invalid");
    config.apply_env_overrides();

    assert!(config.skills.open_skills_enabled);
    assert!(config.skills.allow_scripts);
    assert_eq!(
        config.skills.prompt_injection_mode,
        SkillsPromptInjectionMode::Compact
    );
    std::env::remove_var("ZEROCLAW_OPEN_SKILLS_ENABLED");
    std::env::remove_var("ZEROCLAW_SKILLS_ALLOW_SCRIPTS");
    std::env::remove_var("ZEROCLAW_SKILLS_PROMPT_MODE");
}

#[test]
async fn env_override_provider_fallback() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();

    std::env::remove_var("ZEROCLAW_PROVIDER");
    std::env::set_var("PROVIDER", "openai");
    config.apply_env_overrides();
    assert_eq!(config.default_provider.as_deref(), Some("openai"));

    std::env::remove_var("PROVIDER");
}

#[test]
async fn env_override_provider_fallback_does_not_replace_non_default_provider() {
    let _env_guard = env_override_lock().await;
    let mut config = Config {
        default_provider: Some("custom:https://proxy.example.com/v1".to_string()),
        ..Config::default()
    };

    std::env::remove_var("ZEROCLAW_PROVIDER");
    std::env::set_var("PROVIDER", "openrouter");
    config.apply_env_overrides();
    assert_eq!(
        config.default_provider.as_deref(),
        Some("custom:https://proxy.example.com/v1")
    );

    std::env::remove_var("PROVIDER");
}

#[test]
async fn env_override_zero_claw_provider_overrides_non_default_provider() {
    let _env_guard = env_override_lock().await;
    let mut config = Config {
        default_provider: Some("custom:https://proxy.example.com/v1".to_string()),
        ..Config::default()
    };

    std::env::set_var("ZEROCLAW_PROVIDER", "openrouter");
    std::env::set_var("PROVIDER", "anthropic");
    config.apply_env_overrides();
    assert_eq!(config.default_provider.as_deref(), Some("openrouter"));

    std::env::remove_var("ZEROCLAW_PROVIDER");
    std::env::remove_var("PROVIDER");
}

#[test]
async fn provider_api_requires_custom_default_provider() {
    let mut config = Config::default();
    config.default_provider = Some("openai".to_string());
    config.provider_api = Some(ProviderApiMode::OpenAiResponses);

    let err = config
        .validate()
        .expect_err("provider_api should be rejected for non-custom provider");
    assert!(err
        .to_string()
        .contains("provider_api is only valid when default_provider uses the custom:<url> format"));
}

#[test]
async fn provider_api_invalid_value_is_rejected() {
    let toml = r#"
default_provider = "custom:https://example.com/v1"
default_model = "gpt-4o"
default_temperature = 0.7
provider_api = "not-a-real-mode"
"#;
    let parsed = toml::from_str::<Config>(toml);
    assert!(
        parsed.is_err(),
        "invalid provider_api should fail to deserialize"
    );
}

#[test]
async fn model_route_max_tokens_must_be_positive_when_set() {
    let mut config = Config::default();
    config.model_routes = vec![ModelRouteConfig {
        hint: "reasoning".to_string(),
        provider: "openrouter".to_string(),
        model: "anthropic/claude-sonnet-4.6".to_string(),
        max_tokens: Some(0),
        api_key: None,
        transport: None,
    }];

    let err = config
        .validate()
        .expect_err("model route max_tokens=0 should be rejected");
    assert!(err
        .to_string()
        .contains("model_routes[0].max_tokens must be greater than 0"));
}

#[test]
async fn default_model_hint_requires_matching_model_route() {
    let mut config = Config::default();
    config.default_model = Some("hint:reasoning".to_string());
    config.model_routes = vec![ModelRouteConfig {
        hint: "fast".to_string(),
        provider: "openrouter".to_string(),
        model: "openai/gpt-5.2".to_string(),
        max_tokens: None,
        api_key: None,
        transport: None,
    }];

    let err = config
        .validate()
        .expect_err("default_model hint without matching route should fail");
    assert!(err
        .to_string()
        .contains("default_model uses hint 'reasoning'"));
}

#[test]
async fn default_model_hint_accepts_matching_model_route() {
    let mut config = Config::default();
    config.default_model = Some("hint:reasoning".to_string());
    config.model_routes = vec![ModelRouteConfig {
        hint: "reasoning".to_string(),
        provider: "openrouter".to_string(),
        model: "openai/gpt-5.2".to_string(),
        max_tokens: None,
        api_key: None,
        transport: None,
    }];

    let result = config.validate();
    assert!(
        result.is_ok(),
        "matching default hint route should validate"
    );
}

#[test]
async fn default_model_hint_accepts_matching_model_route_with_whitespace() {
    let mut config = Config::default();
    config.default_model = Some("hint: reasoning ".to_string());
    config.model_routes = vec![ModelRouteConfig {
        hint: " reasoning ".to_string(),
        provider: "openrouter".to_string(),
        model: "openai/gpt-5.2".to_string(),
        max_tokens: None,
        api_key: None,
        transport: None,
    }];

    let result = config.validate();
    assert!(
        result.is_ok(),
        "trimmed default hint should match trimmed route hint"
    );
}

#[test]
async fn provider_transport_normalizes_aliases() {
    let mut config = Config::default();
    config.provider.transport = Some("WS".to_string());
    assert_eq!(
        config.effective_provider_transport().as_deref(),
        Some("websocket")
    );
}

#[test]
async fn provider_transport_invalid_is_rejected() {
    let mut config = Config::default();
    config.provider.transport = Some("udp".to_string());
    let err = config
        .validate()
        .expect_err("provider.transport should reject invalid values");
    assert!(err
        .to_string()
        .contains("provider.transport must be one of: auto, websocket, sse"));
}

#[test]
async fn model_route_transport_invalid_is_rejected() {
    let mut config = Config::default();
    config.model_routes = vec![ModelRouteConfig {
        hint: "reasoning".to_string(),
        provider: "openrouter".to_string(),
        model: "anthropic/claude-sonnet-4.6".to_string(),
        max_tokens: None,
        api_key: None,
        transport: Some("udp".to_string()),
    }];

    let err = config
        .validate()
        .expect_err("model_routes[].transport should reject invalid values");
    assert!(err
        .to_string()
        .contains("model_routes[0].transport must be one of: auto, websocket, sse"));
}

#[test]
async fn env_override_glm_api_key_for_regional_aliases() {
    let _env_guard = env_override_lock().await;
    let mut config = Config {
        default_provider: Some("glm-cn".to_string()),
        ..Config::default()
    };

    std::env::set_var("GLM_API_KEY", "glm-regional-key");
    config.apply_env_overrides();
    assert_eq!(config.api_key.as_deref(), Some("glm-regional-key"));

    std::env::remove_var("GLM_API_KEY");
}

#[test]
async fn env_override_zeroclaw_api_key_beats_glm_api_key_for_regional_aliases() {
    let _env_guard = env_override_lock().await;
    let mut config = Config {
        default_provider: Some("glm-cn".to_string()),
        ..Config::default()
    };

    std::env::set_var("ZEROCLAW_API_KEY", "sk-explicit-env-key");
    std::env::set_var("GLM_API_KEY", "glm-regional-key");
    config.apply_env_overrides();
    assert_eq!(config.api_key.as_deref(), Some("sk-explicit-env-key"));

    std::env::remove_var("ZEROCLAW_API_KEY");
    std::env::remove_var("GLM_API_KEY");
}

#[test]
async fn env_override_zai_api_key_for_regional_aliases() {
    let _env_guard = env_override_lock().await;
    let mut config = Config {
        default_provider: Some("zai-cn".to_string()),
        ..Config::default()
    };

    std::env::set_var("ZAI_API_KEY", "zai-regional-key");
    config.apply_env_overrides();
    assert_eq!(config.api_key.as_deref(), Some("zai-regional-key"));

    std::env::remove_var("ZAI_API_KEY");
}

#[test]
async fn env_override_zeroclaw_api_key_beats_zai_api_key_for_regional_aliases() {
    let _env_guard = env_override_lock().await;
    let mut config = Config {
        default_provider: Some("zai-cn".to_string()),
        ..Config::default()
    };

    std::env::set_var("ZEROCLAW_API_KEY", "sk-explicit-env-key");
    std::env::set_var("ZAI_API_KEY", "zai-regional-key");
    config.apply_env_overrides();
    assert_eq!(config.api_key.as_deref(), Some("sk-explicit-env-key"));

    std::env::remove_var("ZEROCLAW_API_KEY");
    std::env::remove_var("ZAI_API_KEY");
}

#[test]
async fn env_override_model() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();

    std::env::set_var("ZEROCLAW_MODEL", "gpt-4o");
    config.apply_env_overrides();
    assert_eq!(config.default_model.as_deref(), Some("gpt-4o"));

    std::env::remove_var("ZEROCLAW_MODEL");
}

#[test]
async fn resolve_default_model_id_prefers_configured_model() {
    let resolved =
        resolve_default_model_id(Some("  anthropic/claude-opus-4.6  "), Some("openrouter"));
    assert_eq!(resolved, "anthropic/claude-opus-4.6");
}

#[test]
async fn resolve_default_model_id_uses_provider_specific_fallback() {
    let openai = resolve_default_model_id(None, Some("openai"));
    assert_eq!(openai, "gpt-5.2");

    let stepfun = resolve_default_model_id(None, Some("stepfun"));
    assert_eq!(stepfun, "step-3.5-flash");

    let bedrock = resolve_default_model_id(None, Some("aws-bedrock"));
    assert_eq!(bedrock, "anthropic.claude-sonnet-4-5-20250929-v1:0");

    let ai21 = resolve_default_model_id(None, Some("ai21"));
    assert_eq!(ai21, "jamba-1.5-large");

    let huggingface = resolve_default_model_id(None, Some("huggingface"));
    assert_eq!(huggingface, "meta-llama/Llama-3.3-70B-Instruct");
}

#[test]
async fn resolve_default_model_id_handles_special_provider_aliases() {
    let qwen_coding_plan = resolve_default_model_id(None, Some("qwen-coding-plan"));
    assert_eq!(qwen_coding_plan, "qwen3-coder-plus");

    let google_alias = resolve_default_model_id(None, Some("google-gemini"));
    assert_eq!(google_alias, "gemini-2.5-pro");

    let step_alias = resolve_default_model_id(None, Some("step"));
    assert_eq!(step_alias, "step-3.5-flash");

    let step_ai_alias = resolve_default_model_id(None, Some("step-ai"));
    assert_eq!(step_ai_alias, "step-3.5-flash");

    let samba_nova_alias = resolve_default_model_id(None, Some("samba-nova"));
    assert_eq!(samba_nova_alias, "Meta-Llama-3.3-70B-Instruct");

    let hf_alias = resolve_default_model_id(None, Some("hf"));
    assert_eq!(hf_alias, "meta-llama/Llama-3.3-70B-Instruct");
}

#[test]
async fn model_provider_profile_maps_to_custom_endpoint() {
    let _env_guard = env_override_lock().await;
    let mut config = Config {
        default_provider: Some("sub2api".to_string()),
        model_providers: HashMap::from([(
            "sub2api".to_string(),
            ModelProviderConfig {
                name: Some("sub2api".to_string()),
                base_url: Some("https://api.tonsof.blue/v1".to_string()),
                auth_header: None,
                wire_api: None,
                default_model: None,
                api_key: None,
                requires_openai_auth: false,
            },
        )]),
        ..Config::default()
    };

    config.apply_env_overrides();
    assert_eq!(
        config.default_provider.as_deref(),
        Some("custom:https://api.tonsof.blue/v1")
    );
    assert_eq!(
        config.api_url.as_deref(),
        Some("https://api.tonsof.blue/v1")
    );
}

#[test]
async fn model_provider_profile_surfaces_custom_auth_header_for_matching_custom_provider() {
    let _env_guard = env_override_lock().await;
    let mut config = Config {
            default_provider: Some("azure".to_string()),
            model_providers: HashMap::from([(
                "azure".to_string(),
                ModelProviderConfig {
                    name: Some("azure".to_string()),
                    base_url: Some(
                        "https://resource.openai.azure.com/openai/deployments/my-model/chat/completions?api-version=2024-02-01"
                            .to_string(),
                    ),
                    auth_header: Some("api-key".to_string()),
                    wire_api: None,
                    default_model: None,
                    api_key: None,
                    requires_openai_auth: false,
                },
            )]),
            ..Config::default()
        };

    config.apply_env_overrides();
    assert_eq!(
            config.default_provider.as_deref(),
            Some(
                "custom:https://resource.openai.azure.com/openai/deployments/my-model/chat/completions?api-version=2024-02-01"
            )
        );
    assert_eq!(
        config.effective_custom_provider_auth_header().as_deref(),
        Some("api-key")
    );
}

#[test]
async fn model_provider_profile_custom_auth_header_requires_url_match() {
    let _env_guard = env_override_lock().await;
    let mut config = Config {
            default_provider: Some("azure".to_string()),
            model_providers: HashMap::from([(
                "azure".to_string(),
                ModelProviderConfig {
                    name: Some("azure".to_string()),
                    base_url: Some(
                        "https://resource.openai.azure.com/openai/deployments/other-model/chat/completions?api-version=2024-02-01"
                            .to_string(),
                    ),
                    auth_header: Some("api-key".to_string()),
                    wire_api: None,
                    default_model: None,
                    api_key: None,
                    requires_openai_auth: false,
                },
            )]),
            ..Config::default()
        };

    config.apply_env_overrides();
    config.default_provider = Some(
            "custom:https://resource.openai.azure.com/openai/deployments/my-model/chat/completions?api-version=2024-02-01"
                .to_string(),
        );
    assert!(config.effective_custom_provider_auth_header().is_none());
}

#[test]
async fn model_provider_profile_custom_auth_header_matches_slash_before_query() {
    let _env_guard = env_override_lock().await;
    let config = Config {
            default_provider: Some(
                "custom:https://resource.openai.azure.com/openai/deployments/my-model/chat/completions?api-version=2024-02-01"
                    .to_string(),
            ),
            model_providers: HashMap::from([(
                "azure".to_string(),
                ModelProviderConfig {
                    name: Some("azure".to_string()),
                    base_url: Some(
                        "https://resource.openai.azure.com/openai/deployments/my-model/chat/completions/?api-version=2024-02-01"
                            .to_string(),
                    ),
                    auth_header: Some("api-key".to_string()),
                    wire_api: None,
                    default_model: None,
                    api_key: None,
                    requires_openai_auth: false,
                },
            )]),
            ..Config::default()
        };

    assert_eq!(
        config.effective_custom_provider_auth_header().as_deref(),
        Some("api-key")
    );
}

#[test]
async fn model_provider_profile_responses_uses_openai_codex_and_openai_key() {
    let _env_guard = env_override_lock().await;
    let mut config = Config {
        default_provider: Some("sub2api".to_string()),
        model_providers: HashMap::from([(
            "sub2api".to_string(),
            ModelProviderConfig {
                name: Some("sub2api".to_string()),
                base_url: Some("https://api.tonsof.blue".to_string()),
                auth_header: None,
                wire_api: Some("responses".to_string()),
                default_model: None,
                api_key: None,
                requires_openai_auth: true,
            },
        )]),
        api_key: None,
        ..Config::default()
    };

    std::env::set_var("OPENAI_API_KEY", "sk-test-codex-key");
    config.apply_env_overrides();
    std::env::remove_var("OPENAI_API_KEY");

    assert_eq!(config.default_provider.as_deref(), Some("openai-codex"));
    assert_eq!(config.api_url.as_deref(), Some("https://api.tonsof.blue"));
    assert_eq!(config.api_key.as_deref(), Some("sk-test-codex-key"));
}

#[test]
async fn validate_ollama_cloud_model_requires_remote_api_url() {
    let _env_guard = env_override_lock().await;
    let config = Config {
        default_provider: Some("ollama".to_string()),
        default_model: Some("glm-5:cloud".to_string()),
        api_url: None,
        api_key: Some("ollama-key".to_string()),
        ..Config::default()
    };

    let error = config.validate().expect_err("expected validation to fail");
    assert!(error.to_string().contains(
        "default_model uses ':cloud' with provider 'ollama', but api_url is local or unset"
    ));
}

#[test]
async fn validate_ollama_cloud_model_accepts_remote_endpoint_and_env_key() {
    let _env_guard = env_override_lock().await;
    let config = Config {
        default_provider: Some("ollama".to_string()),
        default_model: Some("glm-5:cloud".to_string()),
        api_url: Some("https://ollama.com/api".to_string()),
        api_key: None,
        ..Config::default()
    };

    std::env::set_var("OLLAMA_API_KEY", "ollama-env-key");
    let result = config.validate();
    std::env::remove_var("OLLAMA_API_KEY");

    assert!(result.is_ok(), "expected validation to pass: {result:?}");
}

#[test]
async fn validate_rejects_unknown_model_provider_wire_api() {
    let _env_guard = env_override_lock().await;
    let config = Config {
        default_provider: Some("sub2api".to_string()),
        model_providers: HashMap::from([(
            "sub2api".to_string(),
            ModelProviderConfig {
                name: Some("sub2api".to_string()),
                base_url: Some("https://api.tonsof.blue/v1".to_string()),
                auth_header: None,
                wire_api: Some("ws".to_string()),
                default_model: None,
                api_key: None,
                requires_openai_auth: false,
            },
        )]),
        ..Config::default()
    };

    let error = config.validate().expect_err("expected validation failure");
    assert!(error
        .to_string()
        .contains("wire_api must be one of: responses, chat_completions"));
}

#[test]
async fn validate_rejects_invalid_model_provider_auth_header() {
    let _env_guard = env_override_lock().await;
    let config = Config {
        default_provider: Some("sub2api".to_string()),
        model_providers: HashMap::from([(
            "sub2api".to_string(),
            ModelProviderConfig {
                name: Some("sub2api".to_string()),
                base_url: Some("https://api.tonsof.blue/v1".to_string()),
                auth_header: Some("not a header".to_string()),
                wire_api: None,
                default_model: None,
                api_key: None,
                requires_openai_auth: false,
            },
        )]),
        ..Config::default()
    };

    let error = config.validate().expect_err("expected validation failure");
    assert!(error.to_string().contains("auth_header is invalid"));
}

#[test]
async fn validate_rejects_conflicting_model_provider_auth_headers_for_same_base_url() {
    let _env_guard = env_override_lock().await;
    let config = Config {
            default_provider: Some(
                "custom:https://resource.openai.azure.com/openai/deployments/my-model/chat/completions?api-version=2024-02-01"
                    .to_string(),
            ),
            model_providers: HashMap::from([
                (
                    "azure_a".to_string(),
                    ModelProviderConfig {
                        name: Some("openai".to_string()),
                        base_url: Some(
                            "https://resource.openai.azure.com/openai/deployments/my-model/chat/completions?api-version=2024-02-01"
                                .to_string(),
                        ),
                        auth_header: Some("api-key".to_string()),
                        wire_api: None,
                        default_model: None,
                        api_key: None,
                        requires_openai_auth: false,
                    },
                ),
                (
                    "azure_b".to_string(),
                    ModelProviderConfig {
                        name: Some("openai".to_string()),
                        base_url: Some(
                            "https://resource.openai.azure.com/openai/deployments/my-model/chat/completions/?api-version=2024-02-01"
                                .to_string(),
                        ),
                        auth_header: Some("x-api-key".to_string()),
                        wire_api: None,
                        default_model: None,
                        api_key: None,
                        requires_openai_auth: false,
                    },
                ),
            ]),
            ..Config::default()
        };

    let error = config.validate().expect_err("expected validation failure");
    assert!(error.to_string().contains("conflicting auth_header values"));
}

#[test]
async fn model_provider_profile_uses_profile_api_key_when_global_is_missing() {
    let _env_guard = env_override_lock().await;
    let mut config = Config {
        default_provider: Some("sub2api".to_string()),
        api_key: None,
        model_providers: HashMap::from([(
            "sub2api".to_string(),
            ModelProviderConfig {
                name: Some("sub2api".to_string()),
                base_url: Some("https://api.tonsof.blue/v1".to_string()),
                auth_header: None,
                wire_api: None,
                default_model: None,
                api_key: Some("profile-api-key".to_string()),
                requires_openai_auth: false,
            },
        )]),
        ..Config::default()
    };

    config.apply_env_overrides();
    assert_eq!(config.api_key.as_deref(), Some("profile-api-key"));
}

#[test]
async fn env_override_model_fallback() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();

    std::env::remove_var("ZEROCLAW_MODEL");
    std::env::set_var("MODEL", "anthropic/claude-3.5-sonnet");
    config.apply_env_overrides();
    assert_eq!(
        config.default_model.as_deref(),
        Some("anthropic/claude-3.5-sonnet")
    );

    std::env::remove_var("MODEL");
}

#[test]
async fn env_override_workspace() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();

    std::env::set_var("ZEROCLAW_WORKSPACE", "/custom/workspace");
    config.apply_env_overrides();
    assert_eq!(config.workspace_dir, PathBuf::from("/custom/workspace"));

    std::env::remove_var("ZEROCLAW_WORKSPACE");
}

#[test]
async fn load_or_init_workspace_override_uses_workspace_root_for_config() {
    let _env_guard = env_override_lock().await;
    let temp_home =
        std::env::temp_dir().join(format!("zeroclaw_test_home_{}", uuid::Uuid::new_v4()));
    let workspace_dir = temp_home.join("profile-a");

    let original_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", &temp_home);
    std::env::set_var("ZEROCLAW_WORKSPACE", &workspace_dir);

    let config = Config::load_or_init().await.unwrap();

    assert_eq!(config.workspace_dir, workspace_dir.join("workspace"));
    assert_eq!(config.config_path, workspace_dir.join("config.toml"));
    assert!(workspace_dir.join("config.toml").exists());

    std::env::remove_var("ZEROCLAW_WORKSPACE");
    if let Some(home) = original_home {
        std::env::set_var("HOME", home);
    } else {
        std::env::remove_var("HOME");
    }
    let _ = fs::remove_dir_all(temp_home).await;
}

#[test]
async fn load_or_init_workspace_suffix_uses_legacy_config_layout() {
    let _env_guard = env_override_lock().await;
    let temp_home =
        std::env::temp_dir().join(format!("zeroclaw_test_home_{}", uuid::Uuid::new_v4()));
    let workspace_dir = temp_home.join("workspace");
    let legacy_config_path = temp_home.join(".zeroclaw").join("config.toml");

    let original_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", &temp_home);
    std::env::set_var("ZEROCLAW_WORKSPACE", &workspace_dir);

    let config = Config::load_or_init().await.unwrap();

    assert_eq!(config.workspace_dir, workspace_dir);
    assert_eq!(config.config_path, legacy_config_path);
    assert!(config.config_path.exists());

    std::env::remove_var("ZEROCLAW_WORKSPACE");
    if let Some(home) = original_home {
        std::env::set_var("HOME", home);
    } else {
        std::env::remove_var("HOME");
    }
    let _ = fs::remove_dir_all(temp_home).await;
}

#[test]
async fn load_or_init_workspace_override_keeps_existing_legacy_config() {
    let _env_guard = env_override_lock().await;
    let temp_home =
        std::env::temp_dir().join(format!("zeroclaw_test_home_{}", uuid::Uuid::new_v4()));
    let workspace_dir = temp_home.join("custom-workspace");
    let legacy_config_dir = temp_home.join(".zeroclaw");
    let legacy_config_path = legacy_config_dir.join("config.toml");

    fs::create_dir_all(&legacy_config_dir).await.unwrap();
    fs::write(
        &legacy_config_path,
        r#"default_temperature = 0.7
default_model = "legacy-model"
"#,
    )
    .await
    .unwrap();

    let original_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", &temp_home);
    std::env::set_var("ZEROCLAW_WORKSPACE", &workspace_dir);

    let config = Config::load_or_init().await.unwrap();

    assert_eq!(config.workspace_dir, workspace_dir);
    assert_eq!(config.config_path, legacy_config_path);
    assert_eq!(config.default_model.as_deref(), Some("legacy-model"));

    std::env::remove_var("ZEROCLAW_WORKSPACE");
    if let Some(home) = original_home {
        std::env::set_var("HOME", home);
    } else {
        std::env::remove_var("HOME");
    }
    let _ = fs::remove_dir_all(temp_home).await;
}

#[test]
async fn env_override_empty_values_ignored() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();
    let original_provider = config.default_provider.clone();

    std::env::set_var("ZEROCLAW_PROVIDER", "");
    config.apply_env_overrides();
    assert_eq!(config.default_provider, original_provider);

    std::env::remove_var("ZEROCLAW_PROVIDER");
}

#[test]
async fn env_override_gateway_port() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();
    assert_eq!(config.gateway.port, 42617);

    std::env::set_var("ZEROCLAW_GATEWAY_PORT", "8080");
    config.apply_env_overrides();
    assert_eq!(config.gateway.port, 8080);

    std::env::remove_var("ZEROCLAW_GATEWAY_PORT");
}

#[test]
async fn env_override_port_fallback() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();

    std::env::remove_var("ZEROCLAW_GATEWAY_PORT");
    std::env::set_var("PORT", "9000");
    config.apply_env_overrides();
    assert_eq!(config.gateway.port, 9000);

    std::env::remove_var("PORT");
}

#[test]
async fn env_override_gateway_host() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();
    assert_eq!(config.gateway.host, "127.0.0.1");

    std::env::set_var("ZEROCLAW_GATEWAY_HOST", "0.0.0.0");
    config.apply_env_overrides();
    assert_eq!(config.gateway.host, "0.0.0.0");

    std::env::remove_var("ZEROCLAW_GATEWAY_HOST");
}

#[test]
async fn env_override_host_fallback() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();

    std::env::remove_var("ZEROCLAW_GATEWAY_HOST");
    std::env::set_var("HOST", "0.0.0.0");
    config.apply_env_overrides();
    assert_eq!(config.gateway.host, "0.0.0.0");

    std::env::remove_var("HOST");
}

#[test]
async fn env_override_temperature() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();

    std::env::set_var("ZEROCLAW_TEMPERATURE", "0.5");
    config.apply_env_overrides();
    assert!((config.default_temperature - 0.5).abs() < f64::EPSILON);

    std::env::remove_var("ZEROCLAW_TEMPERATURE");
}

#[test]
async fn env_override_temperature_out_of_range_ignored() {
    let _env_guard = env_override_lock().await;
    // Clean up any leftover env vars from other tests
    std::env::remove_var("ZEROCLAW_TEMPERATURE");

    let mut config = Config::default();
    let original_temp = config.default_temperature;

    // Temperature > 2.0 should be ignored
    std::env::set_var("ZEROCLAW_TEMPERATURE", "3.0");
    config.apply_env_overrides();
    assert!(
        (config.default_temperature - original_temp).abs() < f64::EPSILON,
        "Temperature 3.0 should be ignored (out of range)"
    );

    std::env::remove_var("ZEROCLAW_TEMPERATURE");
}

#[test]
async fn env_override_reasoning_enabled() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();
    assert_eq!(config.runtime.reasoning_enabled, None);

    std::env::set_var("ZEROCLAW_REASONING_ENABLED", "false");
    config.apply_env_overrides();
    assert_eq!(config.runtime.reasoning_enabled, Some(false));

    std::env::set_var("ZEROCLAW_REASONING_ENABLED", "true");
    config.apply_env_overrides();
    assert_eq!(config.runtime.reasoning_enabled, Some(true));

    std::env::remove_var("ZEROCLAW_REASONING_ENABLED");
}

#[test]
async fn env_override_reasoning_invalid_value_ignored() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();
    config.runtime.reasoning_enabled = Some(false);

    std::env::set_var("ZEROCLAW_REASONING_ENABLED", "maybe");
    config.apply_env_overrides();
    assert_eq!(config.runtime.reasoning_enabled, Some(false));

    std::env::remove_var("ZEROCLAW_REASONING_ENABLED");
}

#[test]
async fn env_override_reasoning_level_alias() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();
    assert_eq!(config.runtime.reasoning_level, None);

    std::env::set_var("ZEROCLAW_REASONING_LEVEL", "xhigh");
    config.apply_env_overrides();
    assert_eq!(config.runtime.reasoning_level.as_deref(), Some("xhigh"));
    assert_eq!(
        config.effective_provider_reasoning_level().as_deref(),
        Some("xhigh")
    );

    std::env::remove_var("ZEROCLAW_REASONING_LEVEL");
}

#[test]
async fn env_override_reasoning_level_alias_invalid_ignored() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();
    config.runtime.reasoning_level = Some("medium".to_string());

    std::env::set_var("ZEROCLAW_REASONING_LEVEL", "invalid");
    config.apply_env_overrides();
    assert_eq!(config.runtime.reasoning_level.as_deref(), Some("medium"));

    std::env::remove_var("ZEROCLAW_REASONING_LEVEL");
}

#[test]
async fn env_override_provider_transport_normalizes_zeroclaw_alias() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();

    std::env::remove_var("PROVIDER_TRANSPORT");
    std::env::set_var("ZEROCLAW_PROVIDER_TRANSPORT", "WS");
    config.apply_env_overrides();
    assert_eq!(config.provider.transport.as_deref(), Some("websocket"));

    std::env::remove_var("ZEROCLAW_PROVIDER_TRANSPORT");
}

#[test]
async fn env_override_provider_transport_normalizes_legacy_alias() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();

    std::env::remove_var("ZEROCLAW_PROVIDER_TRANSPORT");
    std::env::set_var("PROVIDER_TRANSPORT", "HTTP");
    config.apply_env_overrides();
    assert_eq!(config.provider.transport.as_deref(), Some("sse"));

    std::env::remove_var("PROVIDER_TRANSPORT");
}

#[test]
async fn env_override_provider_transport_invalid_zeroclaw_does_not_override_existing() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();
    config.provider.transport = Some("sse".to_string());

    std::env::remove_var("PROVIDER_TRANSPORT");
    std::env::set_var("ZEROCLAW_PROVIDER_TRANSPORT", "udp");
    config.apply_env_overrides();
    assert_eq!(config.provider.transport.as_deref(), Some("sse"));

    std::env::remove_var("ZEROCLAW_PROVIDER_TRANSPORT");
}

#[test]
async fn env_override_provider_transport_invalid_legacy_does_not_override_existing() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();
    config.provider.transport = Some("auto".to_string());

    std::env::remove_var("ZEROCLAW_PROVIDER_TRANSPORT");
    std::env::set_var("PROVIDER_TRANSPORT", "udp");
    config.apply_env_overrides();
    assert_eq!(config.provider.transport.as_deref(), Some("auto"));

    std::env::remove_var("PROVIDER_TRANSPORT");
}

#[test]
async fn env_override_model_support_vision() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();
    assert_eq!(config.model_support_vision, None);

    std::env::set_var("ZEROCLAW_MODEL_SUPPORT_VISION", "true");
    config.apply_env_overrides();
    assert_eq!(config.model_support_vision, Some(true));

    std::env::set_var("ZEROCLAW_MODEL_SUPPORT_VISION", "false");
    config.apply_env_overrides();
    assert_eq!(config.model_support_vision, Some(false));

    std::env::set_var("ZEROCLAW_MODEL_SUPPORT_VISION", "maybe");
    config.model_support_vision = Some(true);
    config.apply_env_overrides();
    assert_eq!(config.model_support_vision, Some(true));

    std::env::remove_var("ZEROCLAW_MODEL_SUPPORT_VISION");
}

#[test]
async fn env_override_invalid_port_ignored() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();
    let original_port = config.gateway.port;

    std::env::set_var("PORT", "not_a_number");
    config.apply_env_overrides();
    assert_eq!(config.gateway.port, original_port);

    std::env::remove_var("PORT");
}

#[test]
async fn env_override_web_search_config() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();

    std::env::set_var("WEB_SEARCH_ENABLED", "false");
    std::env::set_var("WEB_SEARCH_PROVIDER", "brave");
    std::env::set_var("WEB_SEARCH_MAX_RESULTS", "7");
    std::env::set_var("WEB_SEARCH_TIMEOUT_SECS", "20");
    std::env::set_var("WEB_SEARCH_FALLBACK_PROVIDERS", "tavily,firecrawl");
    std::env::set_var("WEB_SEARCH_RETRIES_PER_PROVIDER", "2");
    std::env::set_var("WEB_SEARCH_RETRY_BACKOFF_MS", "400");
    std::env::set_var("WEB_SEARCH_DOMAIN_FILTER", "docs.rs,github.com");
    std::env::set_var("WEB_SEARCH_LANGUAGE_FILTER", "en,zh");
    std::env::set_var("WEB_SEARCH_COUNTRY", "US");
    std::env::set_var("WEB_SEARCH_RECENCY_FILTER", "day");
    std::env::set_var("WEB_SEARCH_MAX_TOKENS", "4096");
    std::env::set_var("WEB_SEARCH_MAX_TOKENS_PER_PAGE", "1024");
    std::env::set_var("WEB_SEARCH_EXA_SEARCH_TYPE", "neural");
    std::env::set_var("WEB_SEARCH_EXA_INCLUDE_TEXT", "true");
    std::env::set_var("WEB_SEARCH_JINA_SITE_FILTERS", "arxiv.org,openai.com");
    std::env::set_var("BRAVE_API_KEY", "brave-test-key");
    std::env::set_var("PERPLEXITY_API_KEY", "perplexity-test-key");
    std::env::set_var("EXA_API_KEY", "exa-test-key");
    std::env::set_var("JINA_API_KEY", "jina-test-key");

    config.apply_env_overrides();

    assert!(!config.web_search.enabled);
    assert_eq!(config.web_search.provider, "brave");
    assert_eq!(config.web_search.max_results, 7);
    assert_eq!(config.web_search.timeout_secs, 20);
    assert_eq!(
        config.web_search.fallback_providers,
        vec!["tavily".to_string(), "firecrawl".to_string()]
    );
    assert_eq!(config.web_search.retries_per_provider, 2);
    assert_eq!(config.web_search.retry_backoff_ms, 400);
    assert_eq!(
        config.web_search.domain_filter,
        vec!["docs.rs".to_string(), "github.com".to_string()]
    );
    assert_eq!(
        config.web_search.language_filter,
        vec!["en".to_string(), "zh".to_string()]
    );
    assert_eq!(config.web_search.country.as_deref(), Some("US"));
    assert_eq!(config.web_search.recency_filter.as_deref(), Some("day"));
    assert_eq!(config.web_search.max_tokens, Some(4096));
    assert_eq!(config.web_search.max_tokens_per_page, Some(1024));
    assert_eq!(config.web_search.exa_search_type, "neural");
    assert!(config.web_search.exa_include_text);
    assert_eq!(
        config.web_search.jina_site_filters,
        vec!["arxiv.org".to_string(), "openai.com".to_string()]
    );
    assert_eq!(
        config.web_search.brave_api_key.as_deref(),
        Some("brave-test-key")
    );
    assert_eq!(
        config.web_search.perplexity_api_key.as_deref(),
        Some("perplexity-test-key")
    );
    assert_eq!(
        config.web_search.exa_api_key.as_deref(),
        Some("exa-test-key")
    );
    assert_eq!(
        config.web_search.jina_api_key.as_deref(),
        Some("jina-test-key")
    );

    std::env::remove_var("WEB_SEARCH_ENABLED");
    std::env::remove_var("WEB_SEARCH_PROVIDER");
    std::env::remove_var("WEB_SEARCH_MAX_RESULTS");
    std::env::remove_var("WEB_SEARCH_TIMEOUT_SECS");
    std::env::remove_var("WEB_SEARCH_FALLBACK_PROVIDERS");
    std::env::remove_var("WEB_SEARCH_RETRIES_PER_PROVIDER");
    std::env::remove_var("WEB_SEARCH_RETRY_BACKOFF_MS");
    std::env::remove_var("WEB_SEARCH_DOMAIN_FILTER");
    std::env::remove_var("WEB_SEARCH_LANGUAGE_FILTER");
    std::env::remove_var("WEB_SEARCH_COUNTRY");
    std::env::remove_var("WEB_SEARCH_RECENCY_FILTER");
    std::env::remove_var("WEB_SEARCH_MAX_TOKENS");
    std::env::remove_var("WEB_SEARCH_MAX_TOKENS_PER_PAGE");
    std::env::remove_var("WEB_SEARCH_EXA_SEARCH_TYPE");
    std::env::remove_var("WEB_SEARCH_EXA_INCLUDE_TEXT");
    std::env::remove_var("WEB_SEARCH_JINA_SITE_FILTERS");
    std::env::remove_var("BRAVE_API_KEY");
    std::env::remove_var("PERPLEXITY_API_KEY");
    std::env::remove_var("EXA_API_KEY");
    std::env::remove_var("JINA_API_KEY");
}

#[test]
async fn env_override_web_search_invalid_values_ignored() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();
    let original_max_results = config.web_search.max_results;
    let original_timeout = config.web_search.timeout_secs;

    std::env::set_var("WEB_SEARCH_MAX_RESULTS", "99");
    std::env::set_var("WEB_SEARCH_TIMEOUT_SECS", "0");

    config.apply_env_overrides();

    assert_eq!(config.web_search.max_results, original_max_results);
    assert_eq!(config.web_search.timeout_secs, original_timeout);

    std::env::remove_var("WEB_SEARCH_MAX_RESULTS");
    std::env::remove_var("WEB_SEARCH_TIMEOUT_SECS");
}

#[test]
async fn env_override_url_access_policy() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();

    std::env::set_var("URL_ACCESS_REQUIRE_FIRST_VISIT_APPROVAL", "true");
    std::env::set_var("URL_ACCESS_ENFORCE_DOMAIN_ALLOWLIST", "1");
    std::env::set_var("URL_ACCESS_DOMAIN_ALLOWLIST", "docs.rs,github.com");
    std::env::set_var(
        "URL_ACCESS_DOMAIN_BLOCKLIST",
        "evil.example,*.tracking.local",
    );
    std::env::set_var("URL_ACCESS_APPROVED_DOMAINS", "rust-lang.org");

    config.apply_env_overrides();

    assert!(config.security.url_access.require_first_visit_approval);
    assert!(config.security.url_access.enforce_domain_allowlist);
    assert_eq!(
        config.security.url_access.domain_allowlist,
        vec!["docs.rs".to_string(), "github.com".to_string()]
    );
    assert_eq!(
        config.security.url_access.domain_blocklist,
        vec!["evil.example".to_string(), "*.tracking.local".to_string()]
    );
    assert_eq!(
        config.security.url_access.approved_domains,
        vec!["rust-lang.org".to_string()]
    );

    std::env::remove_var("URL_ACCESS_REQUIRE_FIRST_VISIT_APPROVAL");
    std::env::remove_var("URL_ACCESS_ENFORCE_DOMAIN_ALLOWLIST");
    std::env::remove_var("URL_ACCESS_DOMAIN_ALLOWLIST");
    std::env::remove_var("URL_ACCESS_DOMAIN_BLOCKLIST");
    std::env::remove_var("URL_ACCESS_APPROVED_DOMAINS");
}

#[test]
async fn env_override_storage_provider_config() {
    let _env_guard = env_override_lock().await;
    let mut config = Config::default();

    std::env::set_var("ZEROCLAW_STORAGE_PROVIDER", "postgres");
    std::env::set_var("ZEROCLAW_STORAGE_DB_URL", "postgres://example/db");
    std::env::set_var("ZEROCLAW_STORAGE_CONNECT_TIMEOUT_SECS", "15");

    config.apply_env_overrides();

    assert_eq!(config.storage.provider.config.provider, "postgres");
    assert_eq!(
        config.storage.provider.config.db_url.as_deref(),
        Some("postgres://example/db")
    );
    assert_eq!(
        config.storage.provider.config.connect_timeout_secs,
        Some(15)
    );

    std::env::remove_var("ZEROCLAW_STORAGE_PROVIDER");
    std::env::remove_var("ZEROCLAW_STORAGE_DB_URL");
    std::env::remove_var("ZEROCLAW_STORAGE_CONNECT_TIMEOUT_SECS");
}

#[test]
async fn proxy_config_scope_services_requires_entries_when_enabled() {
    let proxy = ProxyConfig {
        enabled: true,
        http_proxy: Some("http://127.0.0.1:7890".into()),
        https_proxy: None,
        all_proxy: None,
        no_proxy: Vec::new(),
        scope: ProxyScope::Services,
        services: Vec::new(),
    };

    let error = proxy.validate().unwrap_err().to_string();
    assert!(error.contains("proxy.scope='services'"));
}

#[test]
async fn env_override_proxy_scope_services() {
    let _env_guard = env_override_lock().await;
    clear_proxy_env_test_vars();

    let mut config = Config::default();
    std::env::set_var("ZEROCLAW_PROXY_ENABLED", "true");
    std::env::set_var("ZEROCLAW_HTTP_PROXY", "http://127.0.0.1:7890");
    std::env::set_var(
        "ZEROCLAW_PROXY_SERVICES",
        "provider.openai, tool.http_request",
    );
    std::env::set_var("ZEROCLAW_PROXY_SCOPE", "services");

    config.apply_env_overrides();

    assert!(config.proxy.enabled);
    assert_eq!(config.proxy.scope, ProxyScope::Services);
    assert_eq!(
        config.proxy.http_proxy.as_deref(),
        Some("http://127.0.0.1:7890")
    );
    assert!(config.proxy.should_apply_to_service("provider.openai"));
    assert!(config.proxy.should_apply_to_service("tool.http_request"));
    assert!(!config.proxy.should_apply_to_service("provider.anthropic"));

    clear_proxy_env_test_vars();
}

#[test]
async fn env_override_proxy_scope_environment_applies_process_env() {
    let _env_guard = env_override_lock().await;
    clear_proxy_env_test_vars();

    let mut config = Config::default();
    std::env::set_var("ZEROCLAW_PROXY_ENABLED", "true");
    std::env::set_var("ZEROCLAW_PROXY_SCOPE", "environment");
    std::env::set_var("ZEROCLAW_HTTP_PROXY", "http://127.0.0.1:7890");
    std::env::set_var("ZEROCLAW_HTTPS_PROXY", "http://127.0.0.1:7891");
    std::env::set_var("ZEROCLAW_NO_PROXY", "localhost,127.0.0.1");

    config.apply_env_overrides();

    assert_eq!(config.proxy.scope, ProxyScope::Environment);
    assert_eq!(
        std::env::var("HTTP_PROXY").ok().as_deref(),
        Some("http://127.0.0.1:7890")
    );
    assert_eq!(
        std::env::var("HTTPS_PROXY").ok().as_deref(),
        Some("http://127.0.0.1:7891")
    );
    assert!(std::env::var("NO_PROXY")
        .ok()
        .is_some_and(|value| value.contains("localhost")));

    clear_proxy_env_test_vars();
}

#[test]
async fn gateway_config_default_values() {
    let g = GatewayConfig::default();
    assert_eq!(g.port, 42617);
    assert_eq!(g.host, "127.0.0.1");
    assert!(g.require_pairing);
    assert!(!g.allow_public_bind);
    assert!(g.paired_tokens.is_empty());
    assert!(!g.trust_forwarded_headers);
    assert_eq!(g.rate_limit_max_keys, 10_000);
    assert_eq!(g.idempotency_max_keys, 10_000);
    assert!(!g.node_control.enabled);
    assert!(g.node_control.auth_token.is_none());
    assert!(g.node_control.allowed_node_ids.is_empty());
}

// ── Peripherals config ───────────────────────────────────────

#[test]
async fn peripherals_config_default_disabled() {
    let p = PeripheralsConfig::default();
    assert!(!p.enabled);
    assert!(p.boards.is_empty());
}

#[test]
async fn peripheral_board_config_defaults() {
    let b = PeripheralBoardConfig::default();
    assert!(b.board.is_empty());
    assert_eq!(b.transport, "serial");
    assert!(b.path.is_none());
    assert_eq!(b.baud, 115_200);
}

#[test]
async fn peripherals_config_toml_roundtrip() {
    let p = PeripheralsConfig {
        enabled: true,
        boards: vec![PeripheralBoardConfig {
            board: "nucleo-f401re".into(),
            transport: "serial".into(),
            path: Some("/dev/ttyACM0".into()),
            baud: 115_200,
        }],
        datasheet_dir: None,
    };
    let toml_str = toml::to_string(&p).unwrap();
    let parsed: PeripheralsConfig = toml::from_str(&toml_str).unwrap();
    assert!(parsed.enabled);
    assert_eq!(parsed.boards.len(), 1);
    assert_eq!(parsed.boards[0].board, "nucleo-f401re");
    assert_eq!(parsed.boards[0].path.as_deref(), Some("/dev/ttyACM0"));
}

#[test]
async fn lark_config_serde() {
    let lc = LarkConfig {
        app_id: "cli_123456".into(),
        app_secret: "secret_abc".into(),
        encrypt_key: Some("encrypt_key".into()),
        verification_token: Some("verify_token".into()),
        allowed_users: vec!["user_123".into(), "user_456".into()],
        mention_only: false,
        group_reply: None,
        use_feishu: true,
        receive_mode: LarkReceiveMode::Websocket,
        port: None,
        draft_update_interval_ms: default_lark_draft_update_interval_ms(),
        max_draft_edits: default_lark_max_draft_edits(),
    };
    let json = serde_json::to_string(&lc).unwrap();
    let parsed: LarkConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.app_id, "cli_123456");
    assert_eq!(parsed.app_secret, "secret_abc");
    assert_eq!(parsed.encrypt_key.as_deref(), Some("encrypt_key"));
    assert_eq!(parsed.verification_token.as_deref(), Some("verify_token"));
    assert_eq!(parsed.allowed_users.len(), 2);
    assert!(parsed.use_feishu);
}

#[test]
async fn lark_config_toml_roundtrip() {
    let lc = LarkConfig {
        app_id: "cli_123456".into(),
        app_secret: "secret_abc".into(),
        encrypt_key: Some("encrypt_key".into()),
        verification_token: Some("verify_token".into()),
        allowed_users: vec!["*".into()],
        mention_only: false,
        group_reply: None,
        use_feishu: false,
        receive_mode: LarkReceiveMode::Webhook,
        port: Some(9898),
        draft_update_interval_ms: default_lark_draft_update_interval_ms(),
        max_draft_edits: default_lark_max_draft_edits(),
    };
    let toml_str = toml::to_string(&lc).unwrap();
    let parsed: LarkConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.app_id, "cli_123456");
    assert_eq!(parsed.app_secret, "secret_abc");
    assert!(!parsed.use_feishu);
}

#[test]
async fn lark_config_deserializes_without_optional_fields() {
    let json = r#"{"app_id":"cli_123","app_secret":"secret"}"#;
    let parsed: LarkConfig = serde_json::from_str(json).unwrap();
    assert!(parsed.encrypt_key.is_none());
    assert!(parsed.verification_token.is_none());
    assert!(parsed.allowed_users.is_empty());
    assert!(!parsed.mention_only);
    assert!(!parsed.use_feishu);
    assert_eq!(
        parsed.effective_group_reply_mode(),
        GroupReplyMode::AllMessages
    );
}

#[test]
async fn lark_config_defaults_to_lark_endpoint() {
    let json = r#"{"app_id":"cli_123","app_secret":"secret"}"#;
    let parsed: LarkConfig = serde_json::from_str(json).unwrap();
    assert!(
        !parsed.use_feishu,
        "use_feishu should default to false (Lark)"
    );
}

#[test]
async fn lark_config_with_wildcard_allowed_users() {
    let json = r#"{"app_id":"cli_123","app_secret":"secret","allowed_users":["*"]}"#;
    let parsed: LarkConfig = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.allowed_users, vec!["*"]);
}

#[test]
async fn lark_group_reply_mode_overrides_legacy_mention_only() {
    let json = r#"{
            "app_id":"cli_123",
            "app_secret":"secret",
            "mention_only":true,
            "group_reply":{
                "mode":"all_messages",
                "allowed_sender_ids":["ou_1"]
            }
        }"#;
    let parsed: LarkConfig = serde_json::from_str(json).unwrap();
    assert_eq!(
        parsed.effective_group_reply_mode(),
        GroupReplyMode::AllMessages
    );
    assert_eq!(
        parsed.group_reply_allowed_sender_ids(),
        vec!["ou_1".to_string()]
    );
}

#[test]
async fn feishu_config_serde() {
    let fc = FeishuConfig {
        app_id: "cli_feishu_123".into(),
        app_secret: "secret_abc".into(),
        encrypt_key: Some("encrypt_key".into()),
        verification_token: Some("verify_token".into()),
        allowed_users: vec!["user_123".into(), "user_456".into()],
        group_reply: None,
        receive_mode: LarkReceiveMode::Websocket,
        port: None,
        draft_update_interval_ms: default_lark_draft_update_interval_ms(),
        max_draft_edits: default_lark_max_draft_edits(),
    };
    let json = serde_json::to_string(&fc).unwrap();
    let parsed: FeishuConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.app_id, "cli_feishu_123");
    assert_eq!(parsed.app_secret, "secret_abc");
    assert_eq!(parsed.encrypt_key.as_deref(), Some("encrypt_key"));
    assert_eq!(parsed.verification_token.as_deref(), Some("verify_token"));
    assert_eq!(parsed.allowed_users.len(), 2);
}

#[test]
async fn feishu_config_toml_roundtrip() {
    let fc = FeishuConfig {
        app_id: "cli_feishu_123".into(),
        app_secret: "secret_abc".into(),
        encrypt_key: Some("encrypt_key".into()),
        verification_token: Some("verify_token".into()),
        allowed_users: vec!["*".into()],
        group_reply: None,
        receive_mode: LarkReceiveMode::Webhook,
        port: Some(9898),
        draft_update_interval_ms: default_lark_draft_update_interval_ms(),
        max_draft_edits: default_lark_max_draft_edits(),
    };
    let toml_str = toml::to_string(&fc).unwrap();
    let parsed: FeishuConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.app_id, "cli_feishu_123");
    assert_eq!(parsed.app_secret, "secret_abc");
    assert_eq!(parsed.receive_mode, LarkReceiveMode::Webhook);
    assert_eq!(parsed.port, Some(9898));
}

#[test]
async fn feishu_config_deserializes_without_optional_fields() {
    let json = r#"{"app_id":"cli_123","app_secret":"secret"}"#;
    let parsed: FeishuConfig = serde_json::from_str(json).unwrap();
    assert!(parsed.encrypt_key.is_none());
    assert!(parsed.verification_token.is_none());
    assert!(parsed.allowed_users.is_empty());
    assert_eq!(parsed.receive_mode, LarkReceiveMode::Websocket);
    assert!(parsed.port.is_none());
    assert_eq!(
        parsed.effective_group_reply_mode(),
        GroupReplyMode::AllMessages
    );
}

#[test]
async fn feishu_group_reply_mode_supports_mention_only() {
    let json = r#"{
            "app_id":"cli_123",
            "app_secret":"secret",
            "group_reply":{
                "mode":"mention_only",
                "allowed_sender_ids":["ou_9"]
            }
        }"#;
    let parsed: FeishuConfig = serde_json::from_str(json).unwrap();
    assert_eq!(
        parsed.effective_group_reply_mode(),
        GroupReplyMode::MentionOnly
    );
    assert_eq!(
        parsed.group_reply_allowed_sender_ids(),
        vec!["ou_9".to_string()]
    );
}

#[test]
async fn qq_config_defaults_to_webhook_receive_mode() {
    let json = r#"{"app_id":"123","app_secret":"secret"}"#;
    let parsed: QQConfig = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.receive_mode, QQReceiveMode::Webhook);
    assert_eq!(parsed.environment, QQEnvironment::Production);
    assert!(parsed.allowed_users.is_empty());
}

#[test]
async fn qq_config_toml_roundtrip_receive_mode() {
    let qc = QQConfig {
        app_id: "123".into(),
        app_secret: "secret".into(),
        allowed_users: vec!["*".into()],
        receive_mode: QQReceiveMode::Websocket,
        environment: QQEnvironment::Sandbox,
    };
    let toml_str = toml::to_string(&qc).unwrap();
    let parsed: QQConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.receive_mode, QQReceiveMode::Websocket);
    assert_eq!(parsed.environment, QQEnvironment::Sandbox);
    assert_eq!(parsed.allowed_users, vec!["*"]);
}

#[test]
async fn dingtalk_config_defaults_allowed_users_to_empty() {
    let json = r#"{"client_id":"ding-app-key","client_secret":"ding-app-secret"}"#;
    let parsed: DingTalkConfig = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.client_id, "ding-app-key");
    assert_eq!(parsed.client_secret, "ding-app-secret");
    assert!(parsed.allowed_users.is_empty());
}

#[test]
async fn dingtalk_config_toml_roundtrip() {
    let dc = DingTalkConfig {
        client_id: "ding-app-key".into(),
        client_secret: "ding-app-secret".into(),
        allowed_users: vec!["*".into(), "staff123".into()],
    };
    let toml_str = toml::to_string(&dc).unwrap();
    let parsed: DingTalkConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.client_id, "ding-app-key");
    assert_eq!(parsed.client_secret, "ding-app-secret");
    assert_eq!(parsed.allowed_users, vec!["*", "staff123"]);
}

#[test]
async fn channels_except_webhook_reports_dingtalk_as_enabled() {
    let mut channels = ChannelsConfig::default();
    channels.dingtalk = Some(DingTalkConfig {
        client_id: "ding-app-key".into(),
        client_secret: "ding-app-secret".into(),
        allowed_users: vec!["*".into()],
    });

    let dingtalk_state = channels
        .channels_except_webhook()
        .iter()
        .find_map(|(handle, enabled)| (handle.name() == "DingTalk").then_some(*enabled));

    assert_eq!(dingtalk_state, Some(true));
}

#[test]
async fn nextcloud_talk_config_serde() {
    let nc = NextcloudTalkConfig {
        base_url: "https://cloud.example.com".into(),
        app_token: "app-token".into(),
        webhook_secret: Some("webhook-secret".into()),
        allowed_users: vec!["user_a".into(), "*".into()],
    };

    let json = serde_json::to_string(&nc).unwrap();
    let parsed: NextcloudTalkConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.base_url, "https://cloud.example.com");
    assert_eq!(parsed.app_token, "app-token");
    assert_eq!(parsed.webhook_secret.as_deref(), Some("webhook-secret"));
    assert_eq!(parsed.allowed_users, vec!["user_a", "*"]);
}

#[test]
async fn nextcloud_talk_config_defaults_optional_fields() {
    let json = r#"{"base_url":"https://cloud.example.com","app_token":"app-token"}"#;
    let parsed: NextcloudTalkConfig = serde_json::from_str(json).unwrap();
    assert!(parsed.webhook_secret.is_none());
    assert!(parsed.allowed_users.is_empty());
}

// ── Config file permission hardening (Unix only) ───────────────

#[cfg(unix)]
#[test]
async fn new_config_file_has_restricted_permissions() {
    let tmp = tempfile::TempDir::new().unwrap();
    let config_path = tmp.path().join("config.toml");

    // Create a config and save it
    let mut config = Config::default();
    config.config_path = config_path.clone();
    config.save().await.unwrap();

    let meta = fs::metadata(&config_path).await.unwrap();
    let mode = meta.permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "New config file should be owner-only (0600), got {mode:o}"
    );
}

#[cfg(unix)]
#[test]
async fn save_restricts_existing_world_readable_config_to_owner_only() {
    let tmp = tempfile::TempDir::new().unwrap();
    let config_path = tmp.path().join("config.toml");

    let mut config = Config::default();
    config.config_path = config_path.clone();
    config.save().await.unwrap();

    // Simulate the regression state observed in issue #1345.
    std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let loose_mode = std::fs::metadata(&config_path)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        loose_mode, 0o644,
        "test setup requires world-readable config"
    );

    config.default_temperature = 0.6;
    config.save().await.unwrap();

    let hardened_mode = std::fs::metadata(&config_path)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        hardened_mode, 0o600,
        "Saving config should restore owner-only permissions (0600)"
    );
}

#[cfg(unix)]
#[test]
async fn world_readable_config_is_detectable() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::TempDir::new().unwrap();
    let config_path = tmp.path().join("config.toml");

    // Create a config file with intentionally loose permissions
    std::fs::write(&config_path, "# test config").unwrap();
    std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o644)).unwrap();

    let meta = std::fs::metadata(&config_path).unwrap();
    let mode = meta.permissions().mode();
    assert!(
        mode & 0o004 != 0,
        "Test setup: file should be world-readable (mode {mode:o})"
    );
}

#[test]
async fn transcription_config_defaults() {
    let tc = TranscriptionConfig::default();
    assert!(!tc.enabled);
    assert!(tc.api_key.is_none());
    assert!(tc.api_url.contains("groq.com"));
    assert_eq!(tc.model, "whisper-large-v3-turbo");
    assert!(tc.language.is_none());
    assert_eq!(tc.max_duration_secs, 120);
}

#[test]
async fn config_roundtrip_with_transcription() {
    let mut config = Config::default();
    config.transcription.enabled = true;
    config.transcription.api_key = Some("transcription-key".into());
    config.transcription.language = Some("en".into());

    let toml_str = toml::to_string_pretty(&config).unwrap();
    let parsed: Config = toml::from_str(&toml_str).unwrap();

    assert!(parsed.transcription.enabled);
    assert_eq!(
        parsed.transcription.api_key.as_deref(),
        Some("transcription-key")
    );
    assert_eq!(parsed.transcription.language.as_deref(), Some("en"));
    assert_eq!(parsed.transcription.model, "whisper-large-v3-turbo");
}

#[test]
async fn config_without_transcription_uses_defaults() {
    let toml_str = r#"
            default_provider = "openrouter"
            default_model = "test-model"
            default_temperature = 0.7
        "#;
    let parsed: Config = toml::from_str(toml_str).unwrap();
    assert!(!parsed.transcription.enabled);
    assert_eq!(parsed.transcription.max_duration_secs, 120);
}

#[test]
async fn security_defaults_are_backward_compatible() {
    let parsed: Config = toml::from_str(
        r#"
default_provider = "openrouter"
default_model = "anthropic/claude-sonnet-4.6"
default_temperature = 0.7
"#,
    )
    .unwrap();

    assert!(parsed.security.otp.enabled);
    assert_eq!(parsed.security.otp.method, OtpMethod::Totp);
    assert_eq!(
        parsed.security.otp.challenge_delivery,
        OtpChallengeDelivery::Dm
    );
    assert_eq!(parsed.security.otp.challenge_timeout_secs, 120);
    assert_eq!(parsed.security.otp.challenge_max_attempts, 3);
    assert!(parsed.security.roles.is_empty());
    assert!(!parsed.security.estop.enabled);
    assert!(parsed.security.estop.require_otp_to_resume);
    assert!(parsed.security.syscall_anomaly.enabled);
    assert!(parsed.security.syscall_anomaly.alert_on_unknown_syscall);
    assert!(!parsed.security.syscall_anomaly.baseline_syscalls.is_empty());
    assert!(parsed.security.url_access.block_private_ip);
    assert!(parsed.security.url_access.allow_cidrs.is_empty());
    assert!(parsed.security.url_access.allow_domains.is_empty());
    assert!(!parsed.security.url_access.allow_loopback);
    assert!(!parsed.security.url_access.require_first_visit_approval);
    assert!(!parsed.security.url_access.enforce_domain_allowlist);
    assert!(parsed.security.url_access.domain_allowlist.is_empty());
    assert!(parsed.security.url_access.domain_blocklist.is_empty());
    assert!(parsed.security.url_access.approved_domains.is_empty());
    assert!(!parsed.security.perplexity_filter.enable_perplexity_filter);
    assert!(parsed.security.outbound_leak_guard.enabled);
    assert_eq!(
        parsed.security.outbound_leak_guard.action,
        OutboundLeakGuardAction::Redact
    );
    assert_eq!(parsed.security.outbound_leak_guard.sensitivity, 0.7);
    assert!(parsed.security.canary_tokens);
}

#[test]
async fn security_toml_parses_otp_and_estop_sections() {
    let parsed: Config = toml::from_str(
        r#"
default_provider = "openrouter"
default_model = "anthropic/claude-sonnet-4.6"
default_temperature = 0.7

[security]
canary_tokens = false

[security.otp]
enabled = true
method = "totp"
token_ttl_secs = 30
cache_valid_secs = 120
gated_actions = ["shell", "browser_open"]
gated_domains = ["*.chase.com", "accounts.google.com"]
gated_domain_categories = ["banking"]
challenge_delivery = "thread"
challenge_timeout_secs = 180
challenge_max_attempts = 4

[[security.roles]]
name = "developer"
description = "Developer role"
allowed_tools = ["shell", "file_read", "file_write"]
denied_tools = ["memory_forget"]
totp_gated = ["shell", "file_write"]
inherits = "operator"
gated_domains = ["*.chase.com"]
gated_domain_categories = ["banking"]

[security.estop]
enabled = true
state_file = "~/.zeroclaw/estop-state.json"
require_otp_to_resume = true

[security.syscall_anomaly]
enabled = true
strict_mode = true
alert_on_unknown_syscall = true
max_denied_events_per_minute = 3
max_total_events_per_minute = 60
max_alerts_per_minute = 10
alert_cooldown_secs = 15
log_path = "syscall-anomalies.log"
baseline_syscalls = ["read", "write", "openat", "close"]

[security.perplexity_filter]
enable_perplexity_filter = true
perplexity_threshold = 16.5
suffix_window_chars = 72
min_prompt_chars = 40
symbol_ratio_threshold = 0.25

[security.outbound_leak_guard]
enabled = true
action = "block"
sensitivity = 0.9
"#,
    )
    .unwrap();

    assert!(parsed.security.otp.enabled);
    assert!(parsed.security.estop.enabled);
    assert!(parsed.security.syscall_anomaly.strict_mode);
    assert_eq!(
        parsed.security.syscall_anomaly.max_denied_events_per_minute,
        3
    );
    assert_eq!(
        parsed.security.syscall_anomaly.max_total_events_per_minute,
        60
    );
    assert_eq!(parsed.security.syscall_anomaly.max_alerts_per_minute, 10);
    assert_eq!(parsed.security.syscall_anomaly.alert_cooldown_secs, 15);
    assert_eq!(parsed.security.syscall_anomaly.baseline_syscalls.len(), 4);
    assert!(parsed.security.perplexity_filter.enable_perplexity_filter);
    assert_eq!(parsed.security.perplexity_filter.perplexity_threshold, 16.5);
    assert_eq!(parsed.security.perplexity_filter.suffix_window_chars, 72);
    assert_eq!(parsed.security.perplexity_filter.min_prompt_chars, 40);
    assert_eq!(
        parsed.security.perplexity_filter.symbol_ratio_threshold,
        0.25
    );
    assert!(parsed.security.outbound_leak_guard.enabled);
    assert_eq!(
        parsed.security.outbound_leak_guard.action,
        OutboundLeakGuardAction::Block
    );
    assert_eq!(parsed.security.outbound_leak_guard.sensitivity, 0.9);
    assert!(!parsed.security.canary_tokens);
    assert_eq!(parsed.security.otp.gated_actions.len(), 2);
    assert_eq!(parsed.security.otp.gated_domains.len(), 2);
    assert_eq!(
        parsed.security.otp.challenge_delivery,
        OtpChallengeDelivery::Thread
    );
    assert_eq!(parsed.security.otp.challenge_timeout_secs, 180);
    assert_eq!(parsed.security.otp.challenge_max_attempts, 4);
    assert_eq!(parsed.security.roles.len(), 1);
    assert_eq!(parsed.security.roles[0].name, "developer");
    parsed.validate().unwrap();
}

#[test]
async fn security_validation_rejects_invalid_domain_glob() {
    let mut config = Config::default();
    config.security.otp.gated_domains = vec!["bad domain.com".into()];

    let err = config.validate().expect_err("expected invalid domain glob");
    assert!(err.to_string().contains("gated_domains"));
}

#[test]
async fn agent_validation_rejects_empty_allowed_tool_entry() {
    let mut config = Config::default();
    config.agent.allowed_tools = vec!["   ".to_string()];

    let err = config
        .validate()
        .expect_err("expected invalid agent allowed_tools entry");
    assert!(err.to_string().contains("agent.allowed_tools"));
}

#[test]
async fn agent_validation_rejects_invalid_allowed_tool_chars() {
    let mut config = Config::default();
    config.agent.allowed_tools = vec!["bad tool".to_string()];

    let err = config
        .validate()
        .expect_err("expected invalid agent allowed_tools chars");
    assert!(err.to_string().contains("agent.allowed_tools"));
}

#[test]
async fn agent_validation_rejects_empty_denied_tool_entry() {
    let mut config = Config::default();
    config.agent.denied_tools = vec!["   ".to_string()];

    let err = config
        .validate()
        .expect_err("expected invalid agent denied_tools entry");
    assert!(err.to_string().contains("agent.denied_tools"));
}

#[test]
async fn agent_validation_rejects_invalid_denied_tool_chars() {
    let mut config = Config::default();
    config.agent.denied_tools = vec!["bad/tool".to_string()];

    let err = config
        .validate()
        .expect_err("expected invalid agent denied_tools chars");
    assert!(err.to_string().contains("agent.denied_tools"));
}

#[test]
async fn security_validation_rejects_invalid_url_access_cidr() {
    let mut config = Config::default();
    config.security.url_access.allow_cidrs = vec!["10.0.0.0".into()];
    let err = config.validate().expect_err("expected invalid CIDR");
    assert!(err.to_string().contains("security.url_access.allow_cidrs"));
}

#[test]
async fn security_validation_rejects_blank_url_access_domain() {
    let mut config = Config::default();
    config.security.url_access.allow_domains = vec!["   ".into()];
    let err = config
        .validate()
        .expect_err("expected invalid URL allow domain");
    assert!(err
        .to_string()
        .contains("security.url_access.allow_domains"));
}

#[test]
async fn security_validation_rejects_blank_url_access_domain_allowlist_entry() {
    let mut config = Config::default();
    config.security.url_access.domain_allowlist = vec!["  ".into()];
    let err = config
        .validate()
        .expect_err("expected invalid URL domain_allowlist entry");
    assert!(err
        .to_string()
        .contains("security.url_access.domain_allowlist"));
}

#[test]
async fn security_validation_rejects_blank_url_access_domain_blocklist_entry() {
    let mut config = Config::default();
    config.security.url_access.domain_blocklist = vec!["  ".into()];
    let err = config
        .validate()
        .expect_err("expected invalid URL domain_blocklist entry");
    assert!(err
        .to_string()
        .contains("security.url_access.domain_blocklist"));
}

#[test]
async fn security_validation_rejects_blank_url_access_approved_domain_entry() {
    let mut config = Config::default();
    config.security.url_access.approved_domains = vec!["  ".into()];
    let err = config
        .validate()
        .expect_err("expected invalid URL approved_domains entry");
    assert!(err
        .to_string()
        .contains("security.url_access.approved_domains"));
}

#[test]
async fn security_validation_requires_allowlist_when_enforcement_enabled() {
    let mut config = Config::default();
    config.security.url_access.enforce_domain_allowlist = true;
    let err = config
        .validate()
        .expect_err("expected allowlist enforcement validation failure");
    assert!(err
        .to_string()
        .contains("security.url_access.enforce_domain_allowlist"));
}

#[test]
async fn reliability_validation_rejects_empty_fallback_api_key_value() {
    let mut config = Config::default();
    config.reliability.fallback_providers = vec!["openrouter".to_string()];
    config
        .reliability
        .fallback_api_keys
        .insert("openrouter".to_string(), "   ".to_string());

    let err = config
        .validate()
        .expect_err("expected fallback_api_keys empty value validation failure");
    assert!(err
        .to_string()
        .contains("reliability.fallback_api_keys.openrouter must not be empty"));
}

#[test]
async fn reliability_validation_rejects_unmapped_fallback_api_key_entry() {
    let mut config = Config::default();
    config.reliability.fallback_providers = vec!["openrouter".to_string()];
    config
        .reliability
        .fallback_api_keys
        .insert("anthropic".to_string(), "sk-ant-test".to_string());

    let err = config
        .validate()
        .expect_err("expected fallback_api_keys mapping validation failure");
    assert!(err
        .to_string()
        .contains("reliability.fallback_api_keys.anthropic has no matching entry"));
}

#[test]
async fn security_validation_rejects_invalid_http_credential_profile_env_var() {
    let mut config = Config::default();
    config.http_request.credential_profiles.insert(
        "github".to_string(),
        HttpRequestCredentialProfile {
            env_var: "NOT VALID".to_string(),
            ..HttpRequestCredentialProfile::default()
        },
    );

    let err = config
        .validate()
        .expect_err("expected invalid http credential env var");
    assert!(err
        .to_string()
        .contains("http_request.credential_profiles.github.env_var"));
}

#[test]
async fn security_validation_rejects_empty_http_credential_profile_header_name() {
    let mut config = Config::default();
    config.http_request.credential_profiles.insert(
        "linear".to_string(),
        HttpRequestCredentialProfile {
            header_name: "   ".to_string(),
            env_var: "LINEAR_API_KEY".to_string(),
            ..HttpRequestCredentialProfile::default()
        },
    );

    let err = config
        .validate()
        .expect_err("expected empty header_name validation failure");
    assert!(err
        .to_string()
        .contains("http_request.credential_profiles.linear.header_name"));
}

#[test]
async fn security_validation_rejects_unknown_domain_category() {
    let mut config = Config::default();
    config.security.otp.gated_domain_categories = vec!["not_real".into()];

    let err = config
        .validate()
        .expect_err("expected unknown domain category");
    assert!(err.to_string().contains("gated_domain_categories"));
}

#[test]
async fn security_validation_rejects_zero_token_ttl() {
    let mut config = Config::default();
    config.security.otp.token_ttl_secs = 0;

    let err = config
        .validate()
        .expect_err("expected ttl validation failure");
    assert!(err.to_string().contains("token_ttl_secs"));
}

#[test]
async fn security_validation_rejects_zero_challenge_timeout() {
    let mut config = Config::default();
    config.security.otp.challenge_timeout_secs = 0;

    let err = config
        .validate()
        .expect_err("expected challenge timeout validation failure");
    assert!(err.to_string().contains("challenge_timeout_secs"));
}

#[test]
async fn security_validation_rejects_zero_challenge_attempts() {
    let mut config = Config::default();
    config.security.otp.challenge_max_attempts = 0;

    let err = config
        .validate()
        .expect_err("expected challenge attempts validation failure");
    assert!(err.to_string().contains("challenge_max_attempts"));
}

#[test]
async fn security_validation_rejects_unknown_role_parent() {
    let mut config = Config::default();
    config.security.roles = vec![SecurityRoleConfig {
        name: "developer".to_string(),
        inherits: Some("missing-parent".to_string()),
        ..SecurityRoleConfig::default()
    }];

    let err = config
        .validate()
        .expect_err("expected unknown role parent validation failure");
    assert!(err.to_string().contains("inherits references unknown role"));
}

#[test]
async fn security_validation_rejects_duplicate_role_name() {
    let mut config = Config::default();
    config.security.roles = vec![
        SecurityRoleConfig {
            name: "developer".to_string(),
            ..SecurityRoleConfig::default()
        },
        SecurityRoleConfig {
            name: "Developer".to_string(),
            ..SecurityRoleConfig::default()
        },
    ];

    let err = config
        .validate()
        .expect_err("expected duplicate role validation failure");
    assert!(err.to_string().contains("duplicate role"));
}

#[test]
async fn security_validation_rejects_zero_syscall_threshold() {
    let mut config = Config::default();
    config.security.syscall_anomaly.max_denied_events_per_minute = 0;

    let err = config
        .validate()
        .expect_err("expected syscall threshold validation failure");
    assert!(err.to_string().contains("max_denied_events_per_minute"));
}

#[test]
async fn security_validation_rejects_invalid_syscall_baseline_name() {
    let mut config = Config::default();
    config.security.syscall_anomaly.baseline_syscalls = vec!["openat".into(), "bad name".into()];

    let err = config
        .validate()
        .expect_err("expected syscall baseline name validation failure");
    assert!(err.to_string().contains("baseline_syscalls"));
}

#[test]
async fn security_validation_rejects_zero_syscall_alert_budget() {
    let mut config = Config::default();
    config.security.syscall_anomaly.max_alerts_per_minute = 0;

    let err = config
        .validate()
        .expect_err("expected syscall alert budget validation failure");
    assert!(err.to_string().contains("max_alerts_per_minute"));
}

#[test]
async fn security_validation_rejects_zero_syscall_cooldown() {
    let mut config = Config::default();
    config.security.syscall_anomaly.alert_cooldown_secs = 0;

    let err = config
        .validate()
        .expect_err("expected syscall cooldown validation failure");
    assert!(err.to_string().contains("alert_cooldown_secs"));
}

#[test]
async fn security_validation_rejects_denied_threshold_above_total_threshold() {
    let mut config = Config::default();
    config.security.syscall_anomaly.max_denied_events_per_minute = 10;
    config.security.syscall_anomaly.max_total_events_per_minute = 5;

    let err = config
        .validate()
        .expect_err("expected syscall threshold ordering validation failure");
    assert!(err
        .to_string()
        .contains("max_denied_events_per_minute must be less than or equal"));
}

#[test]
async fn security_validation_rejects_invalid_perplexity_threshold() {
    let mut config = Config::default();
    config.security.perplexity_filter.perplexity_threshold = 1.0;

    let err = config
        .validate()
        .expect_err("expected perplexity threshold validation failure");
    assert!(err.to_string().contains("perplexity_threshold"));
}

#[test]
async fn security_validation_rejects_invalid_perplexity_symbol_ratio_threshold() {
    let mut config = Config::default();
    config.security.perplexity_filter.symbol_ratio_threshold = 1.5;

    let err = config
        .validate()
        .expect_err("expected perplexity symbol ratio validation failure");
    assert!(err.to_string().contains("symbol_ratio_threshold"));
}

#[test]
async fn security_validation_rejects_invalid_outbound_leak_guard_sensitivity() {
    let mut config = Config::default();
    config.security.outbound_leak_guard.sensitivity = 1.2;

    let err = config
        .validate()
        .expect_err("expected outbound leak guard sensitivity validation failure");
    assert!(err
        .to_string()
        .contains("security.outbound_leak_guard.sensitivity"));
}

#[test]
async fn coordination_config_defaults() {
    let config = Config::default();
    assert!(config.coordination.enabled);
    assert_eq!(config.coordination.lead_agent, "delegate-lead");
    assert_eq!(config.coordination.max_inbox_messages_per_agent, 256);
    assert_eq!(config.coordination.max_dead_letters, 256);
    assert_eq!(config.coordination.max_context_entries, 512);
    assert_eq!(config.coordination.max_seen_message_ids, 4096);
    assert!(config.agent.teams.enabled);
    assert!(config.agent.teams.auto_activate);
    assert_eq!(config.agent.teams.max_agents, 32);
    assert_eq!(
        config.agent.teams.strategy,
        AgentLoadBalanceStrategy::Adaptive
    );
    assert_eq!(config.agent.teams.load_window_secs, 120);
    assert_eq!(config.agent.teams.inflight_penalty, 8);
    assert_eq!(config.agent.teams.recent_selection_penalty, 2);
    assert_eq!(config.agent.teams.recent_failure_penalty, 12);
    assert!(config.agent.subagents.enabled);
    assert!(config.agent.subagents.auto_activate);
    assert_eq!(config.agent.subagents.max_concurrent, 10);
    assert_eq!(
        config.agent.subagents.strategy,
        AgentLoadBalanceStrategy::Adaptive
    );
    assert_eq!(config.agent.subagents.load_window_secs, 180);
    assert_eq!(config.agent.subagents.inflight_penalty, 10);
    assert_eq!(config.agent.subagents.recent_selection_penalty, 3);
    assert_eq!(config.agent.subagents.recent_failure_penalty, 16);
    assert_eq!(config.agent.subagents.queue_wait_ms, 15_000);
    assert_eq!(config.agent.subagents.queue_poll_ms, 200);
}

#[test]
async fn config_roundtrip_with_coordination_section() {
    let mut config = Config::default();
    config.coordination.enabled = true;
    config.coordination.lead_agent = "runtime-lead".into();
    config.coordination.max_inbox_messages_per_agent = 128;
    config.coordination.max_dead_letters = 64;
    config.coordination.max_context_entries = 32;
    config.coordination.max_seen_message_ids = 1024;
    config.agent.teams.enabled = false;
    config.agent.teams.auto_activate = false;
    config.agent.teams.max_agents = 7;
    config.agent.teams.strategy = AgentLoadBalanceStrategy::LeastLoaded;
    config.agent.teams.load_window_secs = 90;
    config.agent.teams.inflight_penalty = 6;
    config.agent.teams.recent_selection_penalty = 1;
    config.agent.teams.recent_failure_penalty = 4;
    config.agent.subagents.enabled = true;
    config.agent.subagents.auto_activate = false;
    config.agent.subagents.max_concurrent = 4;
    config.agent.subagents.strategy = AgentLoadBalanceStrategy::Semantic;
    config.agent.subagents.load_window_secs = 45;
    config.agent.subagents.inflight_penalty = 5;
    config.agent.subagents.recent_selection_penalty = 2;
    config.agent.subagents.recent_failure_penalty = 9;
    config.agent.subagents.queue_wait_ms = 1_000;
    config.agent.subagents.queue_poll_ms = 50;

    let toml_str = toml::to_string_pretty(&config).unwrap();
    let parsed: Config = toml::from_str(&toml_str).unwrap();
    assert!(parsed.coordination.enabled);
    assert_eq!(parsed.coordination.lead_agent, "runtime-lead");
    assert_eq!(parsed.coordination.max_inbox_messages_per_agent, 128);
    assert_eq!(parsed.coordination.max_dead_letters, 64);
    assert_eq!(parsed.coordination.max_context_entries, 32);
    assert_eq!(parsed.coordination.max_seen_message_ids, 1024);
    assert!(!parsed.agent.teams.enabled);
    assert!(!parsed.agent.teams.auto_activate);
    assert_eq!(parsed.agent.teams.max_agents, 7);
    assert_eq!(
        parsed.agent.teams.strategy,
        AgentLoadBalanceStrategy::LeastLoaded
    );
    assert_eq!(parsed.agent.teams.load_window_secs, 90);
    assert_eq!(parsed.agent.teams.inflight_penalty, 6);
    assert_eq!(parsed.agent.teams.recent_selection_penalty, 1);
    assert_eq!(parsed.agent.teams.recent_failure_penalty, 4);
    assert!(parsed.agent.subagents.enabled);
    assert!(!parsed.agent.subagents.auto_activate);
    assert_eq!(parsed.agent.subagents.max_concurrent, 4);
    assert_eq!(
        parsed.agent.subagents.strategy,
        AgentLoadBalanceStrategy::Semantic
    );
    assert_eq!(parsed.agent.subagents.load_window_secs, 45);
    assert_eq!(parsed.agent.subagents.inflight_penalty, 5);
    assert_eq!(parsed.agent.subagents.recent_selection_penalty, 2);
    assert_eq!(parsed.agent.subagents.recent_failure_penalty, 9);
    assert_eq!(parsed.agent.subagents.queue_wait_ms, 1_000);
    assert_eq!(parsed.agent.subagents.queue_poll_ms, 50);
}

#[test]
async fn coordination_validation_rejects_invalid_limits_and_lead_agent() {
    let mut config = Config::default();
    config.coordination.max_inbox_messages_per_agent = 0;
    let err = config
        .validate()
        .expect_err("expected coordination inbox limit validation failure");
    assert!(err
        .to_string()
        .contains("coordination.max_inbox_messages_per_agent"));

    let mut config = Config::default();
    config.coordination.max_dead_letters = 0;
    let err = config
        .validate()
        .expect_err("expected coordination dead-letter limit validation failure");
    assert!(err.to_string().contains("coordination.max_dead_letters"));

    let mut config = Config::default();
    config.coordination.max_context_entries = 0;
    let err = config
        .validate()
        .expect_err("expected coordination context limit validation failure");
    assert!(err.to_string().contains("coordination.max_context_entries"));

    let mut config = Config::default();
    config.coordination.max_seen_message_ids = 0;
    let err = config
        .validate()
        .expect_err("expected coordination dedupe-window validation failure");
    assert!(err
        .to_string()
        .contains("coordination.max_seen_message_ids"));

    let mut config = Config::default();
    config.coordination.lead_agent = "   ".into();
    let err = config
        .validate()
        .expect_err("expected coordination lead-agent validation failure");
    assert!(err.to_string().contains("coordination.lead_agent"));

    let mut config = Config::default();
    config.agent.teams.max_agents = 0;
    let err = config
        .validate()
        .expect_err("expected team-size validation failure");
    assert!(err.to_string().contains("agent.teams.max_agents"));

    let mut config = Config::default();
    config.agent.subagents.max_concurrent = 0;
    let err = config
        .validate()
        .expect_err("expected subagent concurrency validation failure");
    assert!(err.to_string().contains("agent.subagents.max_concurrent"));

    let mut config = Config::default();
    config.agent.teams.load_window_secs = 0;
    let err = config
        .validate()
        .expect_err("expected team load window validation failure");
    assert!(err.to_string().contains("agent.teams.load_window_secs"));

    let mut config = Config::default();
    config.agent.subagents.load_window_secs = 0;
    let err = config
        .validate()
        .expect_err("expected subagent load window validation failure");
    assert!(err.to_string().contains("agent.subagents.load_window_secs"));

    let mut config = Config::default();
    config.agent.subagents.queue_poll_ms = 0;
    let err = config
        .validate()
        .expect_err("expected subagent queue poll validation failure");
    assert!(err.to_string().contains("agent.subagents.queue_poll_ms"));
}

#[test]
async fn coordination_validation_allows_empty_lead_agent_when_disabled() {
    let mut config = Config::default();
    config.coordination.enabled = false;
    config.coordination.lead_agent = String::new();
    config
        .validate()
        .expect("disabled coordination should allow empty lead agent");
}

#[test]
async fn cost_enforcement_defaults_are_stable() {
    let cost = CostConfig::default();
    assert_eq!(cost.enforcement.mode, CostEnforcementMode::Warn);
    assert_eq!(
        cost.enforcement.route_down_model.as_deref(),
        Some("hint:fast")
    );
    assert_eq!(cost.enforcement.reserve_percent, 10);
}

#[test]
async fn cost_enforcement_config_parses_route_down_mode() {
    let parsed: CostConfig = toml::from_str(
        r#"
enabled = true

[enforcement]
mode = "route_down"
route_down_model = "hint:fast"
reserve_percent = 15
"#,
    )
    .expect("cost enforcement should parse");

    assert!(parsed.enabled);
    assert_eq!(parsed.enforcement.mode, CostEnforcementMode::RouteDown);
    assert_eq!(
        parsed.enforcement.route_down_model.as_deref(),
        Some("hint:fast")
    );
    assert_eq!(parsed.enforcement.reserve_percent, 15);
}

#[test]
async fn validation_rejects_cost_enforcement_reserve_over_100() {
    let mut config = Config::default();
    config.cost.enforcement.reserve_percent = 150;
    let err = config
        .validate()
        .expect_err("expected cost.enforcement.reserve_percent validation failure");
    assert!(err.to_string().contains("cost.enforcement.reserve_percent"));
}

#[test]
async fn validation_rejects_route_down_hint_without_matching_route() {
    let mut config = Config::default();
    config.cost.enforcement.mode = CostEnforcementMode::RouteDown;
    config.cost.enforcement.route_down_model = Some("hint:fast".to_string());
    let err = config
        .validate()
        .expect_err("route_down hint should require a matching model route");
    assert!(err
        .to_string()
        .contains("cost.enforcement.route_down_model uses hint 'fast'"));
}

#[test]
async fn validation_accepts_route_down_hint_with_matching_route() {
    let mut config = Config::default();
    config.cost.enforcement.mode = CostEnforcementMode::RouteDown;
    config.cost.enforcement.route_down_model = Some("hint:fast".to_string());
    config.model_routes = vec![ModelRouteConfig {
        hint: "fast".to_string(),
        provider: "openrouter".to_string(),
        model: "openai/gpt-4.1-mini".to_string(),
        api_key: None,
        max_tokens: None,
        transport: None,
    }];

    config
        .validate()
        .expect("matching route_down hint route should validate");
}
