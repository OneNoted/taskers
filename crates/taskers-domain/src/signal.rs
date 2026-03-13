use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
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
    pub timestamp: OffsetDateTime,
}

impl SignalEvent {
    pub fn new(source: impl Into<String>, kind: SignalKind, message: Option<String>) -> Self {
        Self {
            source: source.into(),
            kind,
            message,
            timestamp: OffsetDateTime::now_utc(),
        }
    }
}
