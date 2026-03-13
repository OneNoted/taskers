use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum AttentionState {
    #[default]
    Normal,
    Busy,
    Completed,
    WaitingInput,
    Error,
}

impl AttentionState {
    pub fn rank(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::Busy => 1,
            Self::Completed => 2,
            Self::WaitingInput => 3,
            Self::Error => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Busy => "Busy",
            Self::Completed => "Completed",
            Self::WaitingInput => "Waiting",
            Self::Error => "Error",
        }
    }
}
