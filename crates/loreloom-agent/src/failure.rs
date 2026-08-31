use std::{fmt, time::Duration};

use armillae_llm::{BridgeError, ErrorMetadata};
use loreloom_core::FailureId;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

const MAX_DIAGNOSTIC_LABEL_BYTES: usize = 256;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DiagnosticLabel(String);

impl DiagnosticLabel {
    #[must_use]
    pub fn from_untrusted(value: &str) -> Option<Self> {
        let allowed = |byte: u8| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/' | b'@')
        };
        (!value.is_empty()
            && value.len() <= MAX_DIAGNOSTIC_LABEL_BYTES
            && value.bytes().all(allowed))
        .then(|| Self(value.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for DiagnosticLabel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for DiagnosticLabel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for DiagnosticLabel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for DiagnosticLabel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_untrusted(&value).ok_or_else(|| de::Error::custom("invalid diagnostic label"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelInvocationKind {
    ProviderSetup,
    Narrator,
    Npc,
    NpcGeneration,
}

impl ModelInvocationKind {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ProviderSetup => "provider_setup",
            Self::Narrator => "narrator",
            Self::Npc => "npc",
            Self::NpcGeneration => "npc_generation",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFailureStage {
    Configuration,
    RequestEncoding,
    Projection,
    Invocation,
}

impl ModelFailureStage {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::RequestEncoding => "request_encoding",
            Self::Projection => "projection",
            Self::Invocation => "invocation",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFailureCategory {
    RequestEncoding,
    InvalidConfiguration,
    UnsupportedCapability,
    InvalidRequest,
    ProjectionIncompatible,
    Authentication,
    PermissionDenied,
    RateLimited,
    Timeout,
    Cancelled,
    Transport,
    ProviderRejected,
    InvalidProviderResponse,
    StreamInterrupted,
    Unknown,
}

impl ModelFailureCategory {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::RequestEncoding => "request_encoding",
            Self::InvalidConfiguration => "invalid_configuration",
            Self::UnsupportedCapability => "unsupported_capability",
            Self::InvalidRequest => "invalid_request",
            Self::ProjectionIncompatible => "projection_incompatible",
            Self::Authentication => "authentication",
            Self::PermissionDenied => "permission_denied",
            Self::RateLimited => "rate_limited",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::Transport => "transport",
            Self::ProviderRejected => "provider_rejected",
            Self::InvalidProviderResponse => "invalid_provider_response",
            Self::StreamInterrupted => "stream_interrupted",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelFailureDiagnostic {
    pub correlation_id: FailureId,
    pub invocation: ModelInvocationKind,
    pub stage: ModelFailureStage,
    pub category: ModelFailureCategory,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<DiagnosticLabel>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_http_status"
    )]
    pub http_status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<DiagnosticLabel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
}

impl ModelFailureDiagnostic {
    #[must_use]
    pub fn request_encoding(invocation: ModelInvocationKind) -> Self {
        Self::new(
            invocation,
            ModelFailureStage::RequestEncoding,
            ModelFailureCategory::RequestEncoding,
        )
    }

    #[must_use]
    pub fn from_bridge_error(
        invocation: ModelInvocationKind,
        stage: ModelFailureStage,
        error: &BridgeError,
    ) -> Self {
        let category = match error {
            BridgeError::InvalidConfiguration { .. } => ModelFailureCategory::InvalidConfiguration,
            BridgeError::UnsupportedCapability { .. } => {
                ModelFailureCategory::UnsupportedCapability
            }
            BridgeError::InvalidRequest { .. } => ModelFailureCategory::InvalidRequest,
            BridgeError::ProjectionIncompatible { .. } => {
                ModelFailureCategory::ProjectionIncompatible
            }
            BridgeError::Authentication { .. } => ModelFailureCategory::Authentication,
            BridgeError::PermissionDenied { .. } => ModelFailureCategory::PermissionDenied,
            BridgeError::RateLimited { .. } => ModelFailureCategory::RateLimited,
            BridgeError::Timeout { .. } => ModelFailureCategory::Timeout,
            BridgeError::Cancelled => ModelFailureCategory::Cancelled,
            BridgeError::Transport { .. } => ModelFailureCategory::Transport,
            BridgeError::ProviderRejected { .. } => ModelFailureCategory::ProviderRejected,
            BridgeError::InvalidProviderResponse { .. } => {
                ModelFailureCategory::InvalidProviderResponse
            }
            BridgeError::StreamInterrupted { .. } => ModelFailureCategory::StreamInterrupted,
            _ => ModelFailureCategory::Unknown,
        };
        let mut diagnostic = Self::new(invocation, stage, category);
        match error {
            BridgeError::ProjectionIncompatible {
                target_provider, ..
            } => {
                diagnostic.provider = DiagnosticLabel::from_untrusted(target_provider);
            }
            BridgeError::Authentication { metadata }
            | BridgeError::PermissionDenied { metadata }
            | BridgeError::Timeout { metadata }
            | BridgeError::ProviderRejected { metadata, .. }
            | BridgeError::InvalidProviderResponse { metadata, .. }
            | BridgeError::StreamInterrupted { metadata } => {
                diagnostic.apply_metadata(metadata);
            }
            BridgeError::RateLimited {
                retry_after,
                metadata,
            } => {
                diagnostic.apply_metadata(metadata);
                diagnostic.retryable = Some(true);
                diagnostic.retry_after_ms = retry_after.map(duration_millis);
            }
            BridgeError::Transport {
                retryable,
                metadata,
            } => {
                diagnostic.apply_metadata(metadata);
                diagnostic.retryable = Some(*retryable);
            }
            _ => {}
        }
        diagnostic
    }

    #[must_use]
    pub fn user_summary(&self) -> String {
        let mut summary = format!(
            "{}/{} · {}",
            self.invocation.code(),
            self.stage.code(),
            self.category.code()
        );
        if let Some(provider) = &self.provider {
            summary.push_str(" · provider ");
            summary.push_str(provider.as_str());
        }
        if let Some(status) = self.http_status {
            summary.push_str(&format!(" · HTTP {status}"));
        }
        if let Some(request_id) = &self.request_id {
            summary.push_str(" · request ");
            summary.push_str(request_id.as_str());
        }
        if let Some(retryable) = self.retryable {
            summary.push_str(if retryable {
                " · retryable"
            } else {
                " · not retryable"
            });
        }
        if let Some(retry_after_ms) = self.retry_after_ms {
            summary.push_str(&format!(" · retry after {retry_after_ms} ms"));
        }
        summary.push_str(" · ref ");
        summary.push_str(&self.correlation_id.to_string());
        summary
    }

    fn new(
        invocation: ModelInvocationKind,
        stage: ModelFailureStage,
        category: ModelFailureCategory,
    ) -> Self {
        Self {
            correlation_id: FailureId::new(),
            invocation,
            stage,
            category,
            provider: None,
            http_status: None,
            request_id: None,
            retryable: None,
            retry_after_ms: None,
        }
    }

    fn apply_metadata(&mut self, metadata: &ErrorMetadata) {
        self.provider = DiagnosticLabel::from_untrusted(&metadata.provider);
        self.http_status = metadata
            .http_status
            .filter(|status| (100..=599).contains(status));
        self.request_id = metadata
            .request_id
            .as_deref()
            .and_then(DiagnosticLabel::from_untrusted);
    }
}

impl fmt::Display for ModelFailureDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.user_summary())
    }
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn deserialize_http_status<'de, D>(deserializer: D) -> Result<Option<u16>, D::Error>
where
    D: Deserializer<'de>,
{
    let status = Option::<u16>::deserialize(deserializer)?;
    if status.is_none_or(|status| (100..=599).contains(&status)) {
        Ok(status)
    } else {
        Err(de::Error::custom("invalid diagnostic HTTP status"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_projection_keeps_safe_facts_and_drops_raw_text() {
        let error = BridgeError::ProviderRejected {
            code: Some("must-not-escape".to_owned()),
            message: "secret provider response must-not-escape".to_owned(),
            metadata: ErrorMetadata::new("openai-compatible")
                .with_http_status(400)
                .with_request_id("req_safe-123"),
        };
        let diagnostic = ModelFailureDiagnostic::from_bridge_error(
            ModelInvocationKind::Narrator,
            ModelFailureStage::Invocation,
            &error,
        );
        let rendered = format!("{diagnostic:?} {diagnostic}");

        assert_eq!(diagnostic.category, ModelFailureCategory::ProviderRejected);
        assert_eq!(diagnostic.http_status, Some(400));
        assert!(diagnostic.correlation_id.to_string().starts_with("err_"));
        assert!(!rendered.contains("must-not-escape"));
    }

    #[test]
    fn untrusted_metadata_cannot_inject_control_sequences() {
        let error = BridgeError::Transport {
            retryable: true,
            metadata: ErrorMetadata::new("openai\u{1b}[31m")
                .with_http_status(999)
                .with_request_id("request with spaces"),
        };
        let diagnostic = ModelFailureDiagnostic::from_bridge_error(
            ModelInvocationKind::Npc,
            ModelFailureStage::Invocation,
            &error,
        );

        assert!(diagnostic.provider.is_none());
        assert!(diagnostic.http_status.is_none());
        assert!(diagnostic.request_id.is_none());
        assert_eq!(diagnostic.retryable, Some(true));
    }

    #[test]
    fn diagnostic_wire_rejects_invalid_http_status_and_unknown_fields() {
        let diagnostic = ModelFailureDiagnostic::request_encoding(ModelInvocationKind::Narrator);
        let mut value = serde_json::to_value(diagnostic).expect("diagnostic wire");
        value["http_status"] = serde_json::json!(999);
        assert!(serde_json::from_value::<ModelFailureDiagnostic>(value.clone()).is_err());

        value["http_status"] = serde_json::json!(400);
        value["raw_provider_body"] = serde_json::json!("must-not-enter-the-wire");
        assert!(serde_json::from_value::<ModelFailureDiagnostic>(value).is_err());
    }
}
