use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalPaneMetadata {
    pub title: Option<String>,
    #[serde(default)]
    pub agent_title: Option<String>,
    pub cwd: Option<String>,
    pub repo_name: Option<String>,
    pub git_branch: Option<String>,
    pub ports: Vec<u16>,
    pub agent_kind: Option<String>,
    #[serde(default)]
    pub agent_active: Option<bool>,
    #[serde(default)]
    pub agent_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    Metadata,
    Started,
    Progress,
    Completed,
    WaitingInput,
    Error,
    Notification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalEvent {
    pub source: String,
    pub kind: SignalKind,
    pub message: Option<String>,
    pub metadata: Option<SignalPaneMetadata>,
    pub timestamp: OffsetDateTime,
}

impl SignalEvent {
    pub fn new(source: impl Into<String>, kind: SignalKind, message: Option<String>) -> Self {
        Self::with_metadata(source, kind, message, None)
    }

    pub fn with_metadata(
        source: impl Into<String>,
        kind: SignalKind,
        message: Option<String>,
        metadata: Option<SignalPaneMetadata>,
    ) -> Self {
        Self {
            source: source.into(),
            kind,
            message,
            metadata,
            timestamp: OffsetDateTime::now_utc(),
        }
    }
}
