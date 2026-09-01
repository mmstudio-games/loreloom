use std::fmt;

use loreloom_agent::DiagnosticLabel;
use loreloom_core::FailureId;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderSlot {
    Narrator,
    Npc,
}

impl ProviderSlot {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Narrator => "narrator",
            Self::Npc => "npc",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderSetupIssue {
    CredentialReferenceMissing,
    CredentialEnvironmentMissing,
    CredentialEnvironmentInvalid,
    CredentialEnvironmentEmpty,
    CredentialFileUnreadable,
    CredentialFileEmpty,
    CredentialResolverUnsupported,
    InvalidBridgeConfiguration,
    EndpointNotAllowed,
    UnsupportedProvider,
    CredentialResolutionFailed,
    ProviderConfigurationRejected,
    BridgeCreationFailed,
}

impl ProviderSetupIssue {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::CredentialReferenceMissing => "credential_reference_missing",
            Self::CredentialEnvironmentMissing => "credential_environment_missing",
            Self::CredentialEnvironmentInvalid => "credential_environment_invalid",
            Self::CredentialEnvironmentEmpty => "credential_environment_empty",
            Self::CredentialFileUnreadable => "credential_file_unreadable",
            Self::CredentialFileEmpty => "credential_file_empty",
            Self::CredentialResolverUnsupported => "credential_resolver_unsupported",
            Self::InvalidBridgeConfiguration => "invalid_bridge_configuration",
            Self::EndpointNotAllowed => "endpoint_not_allowed",
            Self::UnsupportedProvider => "unsupported_provider",
            Self::CredentialResolutionFailed => "credential_resolution_failed",
            Self::ProviderConfigurationRejected => "provider_configuration_rejected",
            Self::BridgeCreationFailed => "bridge_creation_failed",
        }
    }

    const fn hint(self) -> &'static str {
        match self {
            Self::CredentialReferenceMissing => {
                "configure an environment or file credential for this Provider"
            }
            Self::CredentialEnvironmentMissing => {
                "export this variable in the environment that starts Loreloom"
            }
            Self::CredentialEnvironmentInvalid => {
                "set this variable to a valid UTF-8 credential value"
            }
            Self::CredentialEnvironmentEmpty => "set this variable to a non-empty credential value",
            Self::CredentialFileUnreadable => {
                "check that the configured credential file exists and is readable UTF-8"
            }
            Self::CredentialFileEmpty => {
                "write a non-empty credential value to the configured credential file"
            }
            Self::CredentialResolverUnsupported => {
                "use an environment or file credential in Loreloom configuration"
            }
            Self::InvalidBridgeConfiguration => {
                "check the Provider, model, transport, defaults, and provider options"
            }
            Self::EndpointNotAllowed => {
                "allow the endpoint host explicitly and use HTTPS unless it is loopback"
            }
            Self::UnsupportedProvider => "choose a Provider supported by this Loreloom build",
            Self::CredentialResolutionFailed => {
                "recheck the credential source and retry the Loreloom process"
            }
            Self::ProviderConfigurationRejected => {
                "check endpoint and Provider-specific configuration options"
            }
            Self::BridgeCreationFailed => "check the selected Provider configuration and retry",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ProviderSetupDiagnostic {
    correlation_id: FailureId,
    slot: ProviderSlot,
    issue: ProviderSetupIssue,
    provider: Option<DiagnosticLabel>,
    environment: Option<DiagnosticLabel>,
}

impl ProviderSetupDiagnostic {
    pub(crate) fn new(slot: ProviderSlot, provider: &str, issue: ProviderSetupIssue) -> Self {
        Self {
            correlation_id: FailureId::new(),
            slot,
            issue,
            provider: DiagnosticLabel::from_untrusted(provider),
            environment: None,
        }
    }

    pub(crate) fn environment(mut self, name: &str) -> Self {
        self.environment = DiagnosticLabel::from_untrusted(name);
        self
    }

    #[cfg(test)]
    pub(crate) const fn slot(&self) -> ProviderSlot {
        self.slot
    }

    #[cfg(test)]
    pub(crate) const fn issue(&self) -> ProviderSetupIssue {
        self.issue
    }
}

impl fmt::Debug for ProviderSetupDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for ProviderSetupDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} · {}", self.slot.code(), self.issue.code())?;
        if let Some(provider) = &self.provider {
            write!(formatter, " · provider {provider}")?;
        }
        if let Some(environment) = &self.environment {
            write!(formatter, " · environment {environment}")?;
        }
        write!(
            formatter,
            " · hint {} · ref {}",
            self.issue.hint(),
            self.correlation_id
        )
    }
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("invalid command line: {0}")]
    Arguments(&'static str),
    #[error("application runtime could not be created")]
    Tokio(#[source] std::io::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Content(#[from] loreloom_content::ContentError),
    #[error(transparent)]
    Package(#[from] loreloom_content::PackageError),
    #[error(transparent)]
    WorldProject(#[from] loreloom_content::WorldProjectError),
    #[error("engine content could not be encoded")]
    ContentCodec(#[source] serde_json::Error),
    #[error("application configuration could not be parsed")]
    ConfigCodec,
    #[error("application configuration is invalid: {0}")]
    ConfigPolicy(&'static str),
    #[error("world configuration is invalid: {0}")]
    WorldPolicy(&'static str),
    #[error("provider setup failed: {0}")]
    ProviderSetup(ProviderSetupDiagnostic),
    #[error(transparent)]
    Store(#[from] loreloom_store::StoreError),
    #[error(transparent)]
    World(#[from] loreloom_world::WorldError),
    #[error(transparent)]
    Runtime(#[from] loreloom_runtime::RuntimeError),
    #[error(transparent)]
    Tui(#[from] loreloom_tui::TuiError),
    #[error(transparent)]
    Identity(#[from] loreloom_core::IdentityError),
    #[error(transparent)]
    Text(#[from] loreloom_core::TextError),
    #[error(transparent)]
    Fixed(#[from] loreloom_core::FixedError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_diagnostic_only_renders_safe_configuration_facts() {
        let diagnostic = ProviderSetupDiagnostic::new(
            ProviderSlot::Narrator,
            "deepseek",
            ProviderSetupIssue::CredentialEnvironmentMissing,
        )
        .environment("DEEPSEEK_API_KEY");
        let rendered = format!("{diagnostic:?} {diagnostic}");

        assert!(rendered.contains("narrator · credential_environment_missing"));
        assert!(rendered.contains("provider deepseek"));
        assert!(rendered.contains("environment DEEPSEEK_API_KEY"));
        assert!(rendered.contains("export this variable"));
        assert!(rendered.contains("ref err_"));
    }

    #[test]
    fn setup_diagnostic_omits_unsafe_labels() {
        let diagnostic = ProviderSetupDiagnostic::new(
            ProviderSlot::Npc,
            "deepseek\u{1b}[31m",
            ProviderSetupIssue::CredentialEnvironmentMissing,
        )
        .environment("SECRET WITH SPACES");
        let rendered = format!("{diagnostic:?} {diagnostic}");

        assert!(!rendered.contains("\u{1b}"));
        assert!(!rendered.contains("SECRET WITH SPACES"));
    }
}
