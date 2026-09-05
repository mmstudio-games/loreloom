use std::{collections::BTreeSet, net::IpAddr, path::Path, sync::Arc, time::Duration};

use armillae_llm::{
    BridgeConfig, BridgeFactory, BridgeResolveContext, CredentialRef, EndpointPolicy, LlmBridge,
};
use armillae_llm_rig::RigBridgeFactory;
use loreloom_agent::ResourceBudget;
use loreloom_runtime::{
    ContextProjectionPolicy, NpcResourcePolicy, OrchestrationBudget, RuntimeConfig,
};
use loreloom_tui::{ImageProtocolPreference, TuiConfig};
use loreloom_world::RuleLimits;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{AppError, ProviderSetupDiagnostic, ProviderSetupIssue, ProviderSlot};

const CONFIG_SCHEMA_V1: u32 = 1;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductConfig {
    schema_version: u32,
    narrator: BridgeConfig,
    npc: BridgeConfig,
    #[serde(default)]
    allowed_endpoint_hosts: BTreeSet<String>,
    #[serde(default)]
    narrator_capabilities: BTreeSet<String>,
    #[serde(default)]
    turn_budget: ResourceBudget,
    #[serde(default)]
    orchestration_budget: OrchestrationConfig,
    #[serde(default)]
    npc_resources: NpcResourceConfig,
    #[serde(default)]
    context_projection: ContextProjectionPolicy,
    #[serde(default)]
    rule_limits: RuleLimitConfig,
    #[serde(default)]
    tui: TuiProductConfig,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct OrchestrationConfig {
    resources: ResourceBudget,
    max_started_agent_turns: u32,
    max_orchestration_rounds: u32,
}

impl Default for OrchestrationConfig {
    fn default() -> Self {
        let value = OrchestrationBudget::default();
        Self {
            resources: value.resources,
            max_started_agent_turns: value.max_started_agent_turns,
            max_orchestration_rounds: value.max_orchestration_rounds,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct NpcResourceConfig {
    max_generated_per_orchestration: u32,
    max_materialized_per_scene: u32,
    max_persistent_generated: u32,
}

impl Default for NpcResourceConfig {
    fn default() -> Self {
        let value = NpcResourcePolicy::default();
        Self {
            max_generated_per_orchestration: value.max_generated_per_orchestration,
            max_materialized_per_scene: value.max_materialized_per_scene,
            max_persistent_generated: value.max_persistent_generated,
        }
    }
}

impl From<NpcResourceConfig> for NpcResourcePolicy {
    fn from(value: NpcResourceConfig) -> Self {
        Self {
            max_generated_per_orchestration: value.max_generated_per_orchestration,
            max_materialized_per_scene: value.max_materialized_per_scene,
            max_persistent_generated: value.max_persistent_generated,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RuleLimitConfig {
    max_triggered_rules: u32,
    max_evaluated_predicates: u32,
    max_applied_effects: u32,
    max_cascade_depth: u32,
}

impl Default for RuleLimitConfig {
    fn default() -> Self {
        let value = RuleLimits::default();
        Self {
            max_triggered_rules: value.max_triggered_rules,
            max_evaluated_predicates: value.max_evaluated_predicates,
            max_applied_effects: value.max_applied_effects,
            max_cascade_depth: value.max_cascade_depth,
        }
    }
}

impl From<RuleLimitConfig> for RuleLimits {
    fn from(value: RuleLimitConfig) -> Self {
        Self {
            max_triggered_rules: value.max_triggered_rules,
            max_evaluated_predicates: value.max_evaluated_predicates,
            max_applied_effects: value.max_applied_effects,
            max_cascade_depth: value.max_cascade_depth,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct TuiProductConfig {
    state_width_percent: u16,
    event_poll_ms: u64,
    image_protocol: ImageProtocolConfig,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ImageProtocolConfig {
    #[default]
    Auto,
    Kitty,
    Iterm2,
    Sixel,
    Halfblocks,
    Disabled,
}

impl From<ImageProtocolConfig> for ImageProtocolPreference {
    fn from(value: ImageProtocolConfig) -> Self {
        match value {
            ImageProtocolConfig::Auto => Self::Auto,
            ImageProtocolConfig::Kitty => Self::Kitty,
            ImageProtocolConfig::Iterm2 => Self::Iterm2,
            ImageProtocolConfig::Sixel => Self::Sixel,
            ImageProtocolConfig::Halfblocks => Self::Halfblocks,
            ImageProtocolConfig::Disabled => Self::Disabled,
        }
    }
}

impl Default for TuiProductConfig {
    fn default() -> Self {
        let value = TuiConfig::default();
        let event_poll_ms = u64::try_from(value.event_poll_interval.as_millis())
            .expect("the built-in TUI interval fits u64");
        Self {
            state_width_percent: value.state_width_percent,
            event_poll_ms,
            image_protocol: ImageProtocolConfig::Auto,
        }
    }
}

pub struct ConfiguredProviders {
    pub narrator: Arc<dyn LlmBridge>,
    pub npc: Arc<dyn LlmBridge>,
    pub runtime: RuntimeConfig,
    pub rules: RuleLimits,
}

pub struct ResolvedProductConfig {
    pub providers: ConfiguredProviders,
    pub tui: TuiConfig,
}

impl ProductConfig {
    #[cfg(test)]
    pub fn load(path: &Path) -> Result<Self, AppError> {
        let source = std::fs::read_to_string(path)?;
        let value: Self = toml::from_str(&source).map_err(|_| AppError::ConfigCodec)?;
        value.validate()?;
        Ok(value)
    }

    pub fn settings(&self) -> Result<Vec<loreloom_tui::StartupSettingView>, AppError> {
        use loreloom_tui::StartupSettingView;
        let mut fields = Vec::new();
        for (slot, bridge) in [("narrator", &self.narrator), ("npc", &self.npc)] {
            for (name, value, help) in [
                (
                    "provider",
                    bridge.provider.clone(),
                    "Provider: anthropic, deepseek, minimax, moonshot, ollama, openai, openai-compatible",
                ),
                ("model", bridge.model.clone(), "Model name (plain text)"),
                (
                    "endpoint",
                    bridge
                        .endpoint
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                    "Custom URL; blank uses Provider default. Add its host to allowed_endpoint_hosts.",
                ),
                (
                    "credential",
                    match &bridge.credential {
                        Some(CredentialRef::Environment { name }) => format!("env:{name}"),
                        Some(CredentialRef::File { path }) => format!("file:{}", path.display()),
                        _ => String::new(),
                    },
                    "Credential reference: env:VARIABLE or file:/path; blank for no credential. Never enter an API key.",
                ),
            ] {
                fields.push(StartupSettingView {
                    key: format!("{slot}.{name}"),
                    value,
                    help: help.to_owned(),
                });
            }
        }
        macro_rules! project {
            ($($name:ident),* $(,)?) => { $(
                flatten_settings(stringify!($name), &toml::Value::try_from(&self.$name).map_err(|_| AppError::ConfigCodec)?, &mut fields);
            )* };
        }
        project!(
            allowed_endpoint_hosts,
            narrator_capabilities,
            turn_budget,
            orchestration_budget,
            npc_resources,
            context_projection,
            rule_limits,
            tui
        );
        Ok(fields)
    }

    pub async fn resolve(self) -> Result<ResolvedProductConfig, AppError> {
        let endpoint_policy = AllowedEndpointPolicy {
            hosts: self.allowed_endpoint_hosts,
        };
        let context = BridgeResolveContext::new().endpoint_policy(&endpoint_policy);
        let factory = RigBridgeFactory;
        let narrator =
            resolve_provider(self.narrator, ProviderSlot::Narrator, context, &factory).await?;
        let npc = resolve_provider(self.npc, ProviderSlot::Npc, context, &factory).await?;
        Ok(ResolvedProductConfig {
            providers: ConfiguredProviders {
                narrator,
                npc,
                runtime: RuntimeConfig {
                    turn_budget: self.turn_budget,
                    orchestration_budget: OrchestrationBudget {
                        resources: self.orchestration_budget.resources,
                        max_started_agent_turns: self.orchestration_budget.max_started_agent_turns,
                        max_orchestration_rounds: self
                            .orchestration_budget
                            .max_orchestration_rounds,
                    },
                    narrator_capabilities: self.narrator_capabilities,
                    npc_resources: self.npc_resources.into(),
                    generation_policy: None,
                    context_projection: self.context_projection,
                },
                rules: self.rule_limits.into(),
            },
            tui: TuiConfig {
                state_width_percent: self.tui.state_width_percent,
                event_poll_interval: Duration::from_millis(self.tui.event_poll_ms),
                image_protocol: self.tui.image_protocol.into(),
            },
        })
    }

    #[must_use]
    pub fn tui_config(&self) -> TuiConfig {
        TuiConfig {
            state_width_percent: self.tui.state_width_percent,
            event_poll_interval: Duration::from_millis(self.tui.event_poll_ms),
            image_protocol: self.tui.image_protocol.into(),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), AppError> {
        if self.schema_version != CONFIG_SCHEMA_V1 {
            return Err(AppError::ConfigPolicy("unsupported config schema"));
        }
        validate_hosts(&self.allowed_endpoint_hosts)?;
        let policy = AllowedEndpointPolicy {
            hosts: self.allowed_endpoint_hosts.clone(),
        };
        validate_bridge_setup(&self.narrator, ProviderSlot::Narrator, &policy)?;
        validate_bridge_setup(&self.npc, ProviderSlot::Npc, &policy)?;
        self.context_projection
            .validate()
            .map_err(AppError::ConfigPolicy)?;
        let configured_limits: RuleLimits = self.rule_limits.into();
        let maximum = RuleLimits::default();
        if configured_limits.max_triggered_rules > maximum.max_triggered_rules
            || configured_limits.max_evaluated_predicates > maximum.max_evaluated_predicates
            || configured_limits.max_applied_effects > maximum.max_applied_effects
            || configured_limits.max_cascade_depth > maximum.max_cascade_depth
        {
            return Err(AppError::ConfigPolicy("rule limits exceed engine maxima"));
        }
        if !(25..=35).contains(&self.tui.state_width_percent) || self.tui.event_poll_ms == 0 {
            return Err(AppError::ConfigPolicy("TUI configuration is out of range"));
        }
        Ok(())
    }
}

fn flatten_settings(
    key: &str,
    value: &toml::Value,
    fields: &mut Vec<loreloom_tui::StartupSettingView>,
) {
    if let toml::Value::Table(table) = value {
        for (name, value) in table {
            flatten_settings(&format!("{key}.{name}"), value, fields);
        }
    } else {
        let (value, help) = match value {
            toml::Value::String(value) => (
                value.clone(),
                "Plain text; image protocol: auto, kitty, iterm2, sixel, halfblocks, disabled",
            ),
            toml::Value::Array(_) => (
                value.to_string(),
                "TOML list of quoted names, e.g. [\"localhost\"]",
            ),
            toml::Value::Boolean(_) => (value.to_string(), "Boolean: true or false"),
            _ => (
                value.to_string(),
                "Non-negative integer; state_width_percent: 25–35; event_poll_ms: greater than 0",
            ),
        };
        fields.push(loreloom_tui::StartupSettingView {
            key: key.to_owned(),
            value,
            help: help.to_owned(),
        });
    }
}

/// Retains the original source for conflict detection; never contains resolved credentials.
pub struct SettingsDocument {
    source: String,
}

impl SettingsDocument {
    pub fn load(path: &Path) -> Result<(Self, ProductConfig), AppError> {
        let source = std::fs::read_to_string(path)?;
        let config: ProductConfig = toml::from_str(&source).map_err(|_| AppError::ConfigCodec)?;
        config.validate()?;
        Ok((Self { source }, config))
    }

    pub fn save(
        &mut self,
        path: &Path,
        fields: &[loreloom_tui::StartupSettingView],
    ) -> Result<ProductConfig, AppError> {
        let original: ProductConfig =
            toml::from_str(&self.source).map_err(|_| AppError::ConfigCodec)?;
        let expected = original.settings()?;
        if fields.len() != expected.len()
            || fields.iter().zip(&expected).any(|(a, b)| a.key != b.key)
        {
            return Err(AppError::ConfigPolicy(
                "settings fields do not match the configuration",
            ));
        }
        let mut document: toml::Value =
            toml::from_str(&self.source).map_err(|_| AppError::ConfigCodec)?;
        for field in fields {
            let mut table = document.as_table_mut().ok_or(AppError::ConfigCodec)?;
            let parts = field.key.split('.').collect::<Vec<_>>();
            let (name, parents) = parts.split_last().ok_or(AppError::ConfigCodec)?;
            for parent in parents {
                table = table
                    .entry((*parent).to_owned())
                    .or_insert_with(|| toml::Value::Table(Default::default()))
                    .as_table_mut()
                    .ok_or(AppError::ConfigCodec)?;
            }
            let value = if *name == "credential" {
                if field.value.is_empty() {
                    table.remove(*name);
                    continue;
                }
                let reference = if let Some(name) = field.value.strip_prefix("env:") {
                    CredentialRef::Environment {
                        name: name.to_owned(),
                    }
                } else if let Some(path) = field.value.strip_prefix("file:") {
                    CredentialRef::File { path: path.into() }
                } else {
                    return Err(AppError::ConfigPolicy(
                        "credential must be an env: or file: reference",
                    ));
                };
                toml::Value::try_from(reference).map_err(|_| AppError::ConfigCodec)?
            } else if matches!(*name, "provider" | "model" | "endpoint" | "image_protocol") {
                if *name == "endpoint" && field.value.is_empty() {
                    table.remove(*name);
                    continue;
                }
                toml::Value::String(field.value.clone())
            } else {
                let parsed: toml::Table = toml::from_str(&format!("value = {}", field.value))
                    .map_err(|_| {
                        AppError::ConfigPolicy(
                            "setting requires a valid number, boolean or TOML list",
                        )
                    })?;
                if parsed.len() != 1 {
                    return Err(AppError::ConfigCodec);
                }
                parsed.get("value").cloned().ok_or(AppError::ConfigCodec)?
            };
            table.insert((*name).to_owned(), value);
        }
        let encoded = toml::to_string_pretty(&document).map_err(|_| AppError::ConfigCodec)?;
        let candidate: ProductConfig =
            toml::from_str(&encoded).map_err(|_| AppError::ConfigCodec)?;
        candidate.validate()?;
        if std::fs::read_to_string(path)? != self.source {
            return Err(AppError::ConfigPolicy(
                "configuration changed on disk; reopen the launcher before saving",
            ));
        }
        atomic_save_config(path, &encoded)?;
        self.source = encoded;
        Ok(candidate)
    }
}

fn atomic_save_config(path: &Path, encoded: &str) -> Result<(), AppError> {
    use std::{
        fs::{self, OpenOptions},
        io::Write,
    };
    // Resolve symlinks so saving through a config link updates its target.
    let path = fs::canonicalize(path)?;
    let parent = path.parent().ok_or(AppError::ConfigPolicy(
        "configuration has no parent directory",
    ))?;
    let permissions = fs::metadata(&path)?.permissions();
    for nonce in 0_u8..16 {
        let temporary = parent.join(format!(
            ".loreloom-config-{}-{nonce}.tmp",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = match options.open(&temporary) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        };
        let result = file
            .write_all(encoded.as_bytes())
            .and_then(|()| file.set_permissions(permissions.clone()))
            .and_then(|()| file.sync_all());
        drop(file);
        let result = result.and_then(|()| fs::rename(&temporary, &path));
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(error.into());
        }
        return Ok(());
    }
    Err(AppError::ConfigPolicy(
        "could not reserve a configuration temporary file",
    ))
}

const SUPPORTED_PROVIDERS: [&str; 7] = [
    "anthropic",
    "deepseek",
    "minimax",
    "moonshot",
    "ollama",
    "openai",
    "openai-compatible",
];

async fn resolve_provider(
    config: BridgeConfig,
    slot: ProviderSlot,
    context: BridgeResolveContext<'_>,
    factory: &RigBridgeFactory,
) -> Result<Arc<dyn LlmBridge>, AppError> {
    validate_credential_availability(&config, slot)?;
    let resolved = config.resolve_with(context).await.map_err(|error| {
        let issue = match &error {
            armillae_llm::BridgeError::InvalidConfiguration { .. } => {
                ProviderSetupIssue::CredentialResolutionFailed
            }
            _ => ProviderSetupIssue::BridgeCreationFailed,
        };
        setup_error(&config, slot, issue)
    })?;
    factory.create(resolved).await.map_err(|error| {
        let issue = match &error {
            armillae_llm::BridgeError::InvalidConfiguration { .. } => {
                ProviderSetupIssue::ProviderConfigurationRejected
            }
            _ => ProviderSetupIssue::BridgeCreationFailed,
        };
        setup_error(&config, slot, issue)
    })
}

fn validate_bridge_setup(
    config: &BridgeConfig,
    slot: ProviderSlot,
    endpoint_policy: &AllowedEndpointPolicy,
) -> Result<(), AppError> {
    config
        .validate(None)
        .map_err(|_| setup_error(config, slot, ProviderSetupIssue::InvalidBridgeConfiguration))?;
    if !SUPPORTED_PROVIDERS.contains(&config.provider.as_str()) {
        return Err(setup_error(
            config,
            slot,
            ProviderSetupIssue::UnsupportedProvider,
        ));
    }
    if config.provider != "ollama" && config.credential.is_none() {
        return Err(setup_error(
            config,
            slot,
            ProviderSetupIssue::CredentialReferenceMissing,
        ));
    }
    if matches!(config.credential, Some(CredentialRef::Resolver { .. })) {
        return Err(setup_error(
            config,
            slot,
            ProviderSetupIssue::CredentialResolverUnsupported,
        ));
    }
    if let Some(endpoint) = &config.endpoint {
        endpoint_policy
            .validate(endpoint)
            .map_err(|_| setup_error(config, slot, ProviderSetupIssue::EndpointNotAllowed))?;
    }
    Ok(())
}

fn validate_credential_availability(
    config: &BridgeConfig,
    slot: ProviderSlot,
) -> Result<(), AppError> {
    validate_credential_availability_with(
        config,
        slot,
        |name: &str| std::env::var(name),
        |path| std::fs::read_to_string(path),
    )
}

fn validate_credential_availability_with<E, F>(
    config: &BridgeConfig,
    slot: ProviderSlot,
    read_environment: E,
    read_file: F,
) -> Result<(), AppError>
where
    E: FnOnce(&str) -> Result<String, std::env::VarError>,
    F: FnOnce(&Path) -> Result<String, std::io::Error>,
{
    match config.credential.as_ref() {
        None if config.provider == "ollama" => Ok(()),
        None => Err(setup_error(
            config,
            slot,
            ProviderSetupIssue::CredentialReferenceMissing,
        )),
        Some(CredentialRef::Environment { name }) => match read_environment(name) {
            Ok(value) if value.is_empty() => Err(setup_environment_error(
                config,
                slot,
                ProviderSetupIssue::CredentialEnvironmentEmpty,
                name,
            )),
            Ok(_) => Ok(()),
            Err(std::env::VarError::NotPresent) => Err(setup_environment_error(
                config,
                slot,
                ProviderSetupIssue::CredentialEnvironmentMissing,
                name,
            )),
            Err(std::env::VarError::NotUnicode(_)) => Err(setup_environment_error(
                config,
                slot,
                ProviderSetupIssue::CredentialEnvironmentInvalid,
                name,
            )),
        },
        Some(CredentialRef::File { path }) => match read_file(path) {
            Ok(value) if credential_file_value_is_empty(&value) => Err(setup_error(
                config,
                slot,
                ProviderSetupIssue::CredentialFileEmpty,
            )),
            Ok(_) => Ok(()),
            Err(_) => Err(setup_error(
                config,
                slot,
                ProviderSetupIssue::CredentialFileUnreadable,
            )),
        },
        Some(CredentialRef::Resolver { .. }) => Err(setup_error(
            config,
            slot,
            ProviderSetupIssue::CredentialResolverUnsupported,
        )),
    }
}

fn credential_file_value_is_empty(value: &str) -> bool {
    matches!(value, "" | "\n" | "\r\n")
}

fn setup_error(config: &BridgeConfig, slot: ProviderSlot, issue: ProviderSetupIssue) -> AppError {
    AppError::ProviderSetup(ProviderSetupDiagnostic::new(slot, &config.provider, issue))
}

fn setup_environment_error(
    config: &BridgeConfig,
    slot: ProviderSlot,
    issue: ProviderSetupIssue,
    name: &str,
) -> AppError {
    AppError::ProviderSetup(
        ProviderSetupDiagnostic::new(slot, &config.provider, issue).environment(name),
    )
}

fn validate_hosts(hosts: &BTreeSet<String>) -> Result<(), AppError> {
    if hosts.iter().any(|host| {
        host.is_empty()
            || !host.is_ascii()
            || host != &host.to_ascii_lowercase()
            || (!valid_dns_name(host) && parse_ip(host).is_none())
    }) {
        return Err(AppError::ConfigPolicy("endpoint allowlist host is invalid"));
    }
    Ok(())
}

fn valid_dns_name(host: &str) -> bool {
    host.split('.').all(|label| {
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

fn parse_ip(host: &str) -> Option<IpAddr> {
    host.trim_matches(['[', ']']).parse().ok()
}

struct AllowedEndpointPolicy {
    hosts: BTreeSet<String>,
}

impl EndpointPolicy for AllowedEndpointPolicy {
    fn validate(&self, endpoint: &Url) -> Result<(), armillae_llm::BridgeError> {
        let host = endpoint
            .host_str()
            .ok_or_else(|| invalid_endpoint("endpoint host is missing"))?;
        if !self.hosts.contains(host) {
            return Err(invalid_endpoint("endpoint host is not allowed"));
        }
        let loopback = host == "localhost" || parse_ip(host).is_some_and(|ip| ip.is_loopback());
        if endpoint.scheme() != "https" && !loopback {
            return Err(invalid_endpoint(
                "non-loopback custom endpoint requires HTTPS",
            ));
        }
        Ok(())
    }
}

fn invalid_endpoint(message: &'static str) -> armillae_llm::BridgeError {
    armillae_llm::BridgeError::InvalidConfiguration {
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change_setting(fields: &mut [loreloom_tui::StartupSettingView], key: &str, value: &str) {
        fields
            .iter_mut()
            .find(|field| field.key == key)
            .expect("setting")
            .value = value.to_owned();
    }

    #[test]
    fn settings_save_round_trips_defaults_provider_references_and_runtime_values() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("config.toml");
        let source = config("http://127.0.0.1:11434", "\"127.0.0.1\"");
        std::fs::write(
            &path,
            format!("{source}\n[narrator.transport]\nrequest_timeout_ms = 12345\n"),
        )
        .expect("write");
        let (mut document, config) = SettingsDocument::load(&path).expect("load");
        let mut fields = config
            .settings()
            .expect("fields including omitted defaults");
        change_setting(&mut fields, "narrator.model", "新模型");
        change_setting(
            &mut fields,
            "narrator.credential",
            "env:LORELOOM_SETTINGS_TEST_KEY",
        );
        change_setting(
            &mut fields,
            "npc.credential",
            "file:/private/credential-reference",
        );
        change_setting(&mut fields, "tui.state_width_percent", "35");
        change_setting(&mut fields, "tui.image_protocol", "disabled");
        change_setting(&mut fields, "turn_budget.max_model_calls", "7");
        let saved = document.save(&path, &fields).expect("save");
        assert_eq!(saved.tui_config().state_width_percent, 35);
        assert_eq!(saved.turn_budget.max_model_calls, 7);
        assert_eq!(saved.narrator.model, "新模型");
        let reloaded = ProductConfig::load(&path).expect("reload");
        assert_eq!(reloaded.settings().expect("fields"), fields);
        let stored: toml::Value =
            toml::from_str(&std::fs::read_to_string(&path).expect("read")).expect("TOML");
        assert_eq!(
            stored["narrator"]["transport"]["request_timeout_ms"].as_integer(),
            Some(12345)
        );
    }

    #[test]
    fn settings_reject_invalid_edits_without_overwriting_and_allow_retry() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("config.toml");
        let source = config("http://127.0.0.1:11434", "\"127.0.0.1\"");
        std::fs::write(&path, &source).expect("write");
        let (mut document, config) = SettingsDocument::load(&path).expect("load");
        for (key, value) in [
            ("tui.state_width_percent", "99"),
            ("tui.event_poll_ms", "0"),
            ("tui.image_protocol", "invalid"),
            ("narrator.provider", "unsupported"),
            ("narrator.endpoint", "https://untrusted.example"),
            ("turn_budget.max_model_calls", "-1"),
            ("context_projection.max_context_tokens", "999999999"),
            ("narrator.credential", "raw-secret-must-not-escape"),
            ("allowed_endpoint_hosts", "["),
        ] {
            let mut fields = config.settings().expect("fields");
            change_setting(&mut fields, key, value);
            let error = document.save(&path, &fields).err().expect("reject");
            assert!(!format!("{error:?} {error}").contains("raw-secret-must-not-escape"));
            assert_eq!(std::fs::read_to_string(&path).expect("read"), source);
        }
        document
            .save(&path, &config.settings().expect("fields"))
            .expect("retry");
    }

    #[test]
    fn settings_preserve_external_changes_and_previous_file_on_write_failure() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("config.toml");
        let source = config("http://127.0.0.1:11434", "\"127.0.0.1\"");
        std::fs::write(&path, &source).expect("write");
        let (mut document, config) = SettingsDocument::load(&path).expect("load");
        let fields = config.settings().expect("fields");
        let external = format!("{source}\n# externally edited\n");
        std::fs::write(&path, &external).expect("external edit");
        assert!(document.save(&path, &fields).is_err());
        assert_eq!(std::fs::read_to_string(&path).expect("read"), external);
        std::fs::write(&path, &source).expect("restore");
        for nonce in 0_u8..16 {
            std::fs::write(
                directory.path().join(format!(
                    ".loreloom-config-{}-{nonce}.tmp",
                    std::process::id()
                )),
                "reserved",
            )
            .expect("reserve");
        }
        assert!(document.save(&path, &fields).is_err());
        assert_eq!(std::fs::read_to_string(&path).expect("read"), source);
    }

    fn credential_config(credential: CredentialRef) -> BridgeConfig {
        BridgeConfig::builder("deepseek", "test")
            .credential(credential)
            .build()
            .expect("credential fixture")
    }

    fn config(endpoint: &str, hosts: &str) -> String {
        format!(
            r#"
schema_version = 1
allowed_endpoint_hosts = [{hosts}]
narrator_capabilities = ["gameplay.weather"]

[narrator]
api_version = "armillae.llm/v1alpha1"
provider = "ollama"
model = "narrator"
endpoint = "{endpoint}"

[npc]
api_version = "armillae.llm/v1alpha1"
provider = "ollama"
model = "npc"
endpoint = "{endpoint}"

[rule_limits]
max_triggered_rules = 64

[tui]
state_width_percent = 32
event_poll_ms = 25
"#
        )
    }

    #[tokio::test]
    async fn strict_config_resolves_bridges_without_exposing_a_secret() {
        let directory = tempfile::tempdir().expect("config directory");
        let path = directory.path().join("loreloom.toml");
        std::fs::write(&path, config("http://127.0.0.1:11434", "\"127.0.0.1\""))
            .expect("write config");
        let resolved = ProductConfig::load(&path)
            .expect("load config")
            .resolve()
            .await
            .expect("resolve bridges");
        assert_eq!(resolved.providers.rules.max_triggered_rules, 64);
        assert!(
            resolved
                .providers
                .runtime
                .narrator_capabilities
                .contains("gameplay.weather")
        );
        assert_eq!(resolved.tui.state_width_percent, 32);
    }

    #[test]
    fn config_rejects_unknown_fields_raw_secrets_and_unsafe_endpoints() {
        let directory = tempfile::tempdir().expect("config directory");
        for (name, source) in [
            (
                "unknown",
                format!(
                    "{}\nunknown = true\n",
                    config("https://gateway.example.com", "\"gateway.example.com\"")
                ),
            ),
            (
                "secret",
                config("https://gateway.example.com", "\"gateway.example.com\"").replace(
                    "model = \"narrator\"",
                    "model = \"narrator\"\napi_key = \"secret\"",
                ),
            ),
            (
                "host",
                config("https://gateway.example.com", "\"other.example.com\""),
            ),
            (
                "http",
                config("http://gateway.example.com", "\"gateway.example.com\""),
            ),
            (
                "context",
                format!(
                    "{}\n[context_projection]\nmax_context_tokens = 131073\n",
                    config("https://gateway.example.com", "\"gateway.example.com\"")
                ),
            ),
        ] {
            let path = directory.path().join(format!("{name}.toml"));
            std::fs::write(&path, source).expect("write invalid config");
            assert!(ProductConfig::load(&path).is_err(), "{name} must fail");
        }
        let secret_path = directory.path().join("secret.toml");
        let marker = "must-not-appear-in-errors";
        std::fs::write(
            &secret_path,
            config("https://gateway.example.com", "\"gateway.example.com\"").replace(
                "model = \"narrator\"",
                &format!("model = \"narrator\"\napi_key = \"{marker}\""),
            ),
        )
        .expect("write secret-shaped config");
        let error = match ProductConfig::load(&secret_path) {
            Ok(_) => panic!("raw secret field must fail"),
            Err(error) => error,
        };
        assert!(!error.to_string().contains(marker));
        assert!(!format!("{error:?}").contains(marker));
    }

    #[test]
    fn credential_preflight_distinguishes_missing_and_empty_environment_values() {
        let config = credential_config(CredentialRef::Environment {
            name: "DEEPSEEK_API_KEY".to_owned(),
        });
        let missing = validate_credential_availability_with(
            &config,
            ProviderSlot::Npc,
            |_| Err(std::env::VarError::NotPresent),
            |_| unreachable!("environment credential must not read a file"),
        )
        .expect_err("missing environment must fail");
        let empty = validate_credential_availability_with(
            &config,
            ProviderSlot::Narrator,
            |_| Ok(String::new()),
            |_| unreachable!("environment credential must not read a file"),
        )
        .expect_err("empty environment must fail");

        let AppError::ProviderSetup(missing) = missing else {
            panic!("missing environment must use setup diagnostics");
        };
        let AppError::ProviderSetup(empty) = empty else {
            panic!("empty environment must use setup diagnostics");
        };
        assert_eq!(missing.slot(), ProviderSlot::Npc);
        assert_eq!(
            missing.issue(),
            ProviderSetupIssue::CredentialEnvironmentMissing
        );
        assert_eq!(empty.slot(), ProviderSlot::Narrator);
        assert_eq!(
            empty.issue(),
            ProviderSetupIssue::CredentialEnvironmentEmpty
        );
        assert!(missing.to_string().contains("environment DEEPSEEK_API_KEY"));
    }

    #[test]
    fn credential_file_failure_does_not_retain_path_content_or_io_error() {
        let config = credential_config(CredentialRef::File {
            path: "/private/credential/must-not-escape".into(),
        });
        let error = validate_credential_availability_with(
            &config,
            ProviderSlot::Narrator,
            |_| unreachable!("file credential must not read the environment"),
            |_| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "secret-io-detail-must-not-escape",
                ))
            },
        )
        .expect_err("unreadable credential file must fail");
        let rendered = format!("{error:?} {error}");

        assert!(rendered.contains("credential_file_unreadable"));
        assert!(!rendered.contains("/private/credential"));
        assert!(!rendered.contains("secret-io-detail"));
    }

    #[test]
    fn endpoint_policy_failure_has_a_stable_setup_code_and_slot() {
        let directory = tempfile::tempdir().expect("config directory");
        let path = directory.path().join("endpoint.toml");
        std::fs::write(
            &path,
            config("https://gateway.example.com", "\"other.example.com\""),
        )
        .expect("write config");
        let error = match ProductConfig::load(&path) {
            Ok(_) => panic!("disallowed endpoint must fail"),
            Err(error) => error,
        };
        let AppError::ProviderSetup(diagnostic) = error else {
            panic!("endpoint failure must use setup diagnostics");
        };

        assert_eq!(diagnostic.slot(), ProviderSlot::Narrator);
        assert_eq!(diagnostic.issue(), ProviderSetupIssue::EndpointNotAllowed);
    }

    #[test]
    fn checked_in_example_is_a_valid_non_secret_configuration() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../loreloom.example.toml");
        let config = ProductConfig::load(&path).expect("example config");
        let turn = ResourceBudget::default();
        let orchestration = OrchestrationBudget::default();

        assert_eq!(config.turn_budget, turn);
        assert_eq!(
            config.orchestration_budget.max_started_agent_turns,
            orchestration.max_started_agent_turns
        );
        assert_eq!(
            config.orchestration_budget.max_orchestration_rounds,
            orchestration.max_orchestration_rounds
        );
        assert_eq!(
            config.orchestration_budget.resources,
            orchestration.resources
        );
    }
}
