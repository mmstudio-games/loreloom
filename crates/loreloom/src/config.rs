use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
    path::Path,
    sync::Arc,
    time::Duration,
};

use armillae_llm::{
    BridgeConfig, BridgeFactory, BridgeResolveContext, CredentialRef, EndpointPolicy, LlmBridge,
};
use armillae_llm_rig::RigBridgeFactory;
use loreloom_agent::ResourceBudget;
use loreloom_content::GenerationPolicy;
use loreloom_runtime::{NpcResourcePolicy, OrchestrationBudget, RuntimeConfig};
use loreloom_tui::TuiConfig;
use loreloom_world::RuleLimits;
use serde::Deserialize;
use url::Url;

use crate::error::AppError;

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
    generation_policies: Vec<GenerationPolicy>,
    #[serde(default)]
    rule_limits: RuleLimitConfig,
    #[serde(default)]
    tui: TuiProductConfig,
}

#[derive(Debug, Clone, Copy, Deserialize)]
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

#[derive(Debug, Clone, Copy, Deserialize)]
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

#[derive(Debug, Clone, Copy, Deserialize)]
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

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct TuiProductConfig {
    state_width_percent: u16,
    event_poll_ms: u64,
}

impl Default for TuiProductConfig {
    fn default() -> Self {
        let value = TuiConfig::default();
        let event_poll_ms = u64::try_from(value.event_poll_interval.as_millis())
            .expect("the built-in TUI interval fits u64");
        Self {
            state_width_percent: value.state_width_percent,
            event_poll_ms,
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
    pub fn load(path: &Path) -> Result<Self, AppError> {
        let source = std::fs::read_to_string(path)?;
        let value: Self = toml::from_str(&source).map_err(|_| AppError::ConfigCodec)?;
        value.validate()?;
        Ok(value)
    }

    pub async fn resolve(self) -> Result<ResolvedProductConfig, AppError> {
        let endpoint_policy = AllowedEndpointPolicy {
            hosts: self.allowed_endpoint_hosts,
        };
        let context = BridgeResolveContext::new().endpoint_policy(&endpoint_policy);
        let narrator = self.narrator.resolve_with(context).await?;
        let npc = self.npc.resolve_with(context).await?;
        let factory = RigBridgeFactory;
        let narrator = factory.create(narrator).await?;
        let npc = factory.create(npc).await?;
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
                    generation_policies: self
                        .generation_policies
                        .into_iter()
                        .map(|policy| (policy.id.clone(), policy))
                        .collect::<BTreeMap<_, _>>(),
                },
                rules: self.rule_limits.into(),
            },
            tui: TuiConfig {
                state_width_percent: self.tui.state_width_percent,
                event_poll_interval: Duration::from_millis(self.tui.event_poll_ms),
            },
        })
    }

    fn validate(&self) -> Result<(), AppError> {
        if self.schema_version != CONFIG_SCHEMA_V1 {
            return Err(AppError::ConfigPolicy("unsupported config schema"));
        }
        validate_bridge_credential(&self.narrator)?;
        validate_bridge_credential(&self.npc)?;
        validate_hosts(&self.allowed_endpoint_hosts)?;
        let policy = AllowedEndpointPolicy {
            hosts: self.allowed_endpoint_hosts.clone(),
        };
        self.narrator.validate(Some(&policy))?;
        self.npc.validate(Some(&policy))?;
        let mut generation_policy_ids = BTreeSet::new();
        if self
            .generation_policies
            .iter()
            .any(|policy| !generation_policy_ids.insert(policy.id.clone()))
        {
            return Err(AppError::ConfigPolicy("duplicate generation policy"));
        }
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

fn validate_bridge_credential(config: &BridgeConfig) -> Result<(), AppError> {
    if matches!(
        config.credential.as_ref(),
        Some(CredentialRef::Resolver { .. })
    ) {
        return Err(AppError::ConfigPolicy(
            "resolver credentials are not installed",
        ));
    }
    Ok(())
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
    fn checked_in_example_is_a_valid_non_secret_configuration() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../loreloom.example.toml");
        ProductConfig::load(&path).expect("example config");
    }
}
