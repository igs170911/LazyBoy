use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Leased,
    Running,
    WaitingInput,
    WaitingTakeover,
    Completed,
    Failed,
    Cancelled,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Leased => "leased",
            Self::Running => "running",
            Self::WaitingInput => "waiting_input",
            Self::WaitingTakeover => "waiting_takeover",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Queued
                | Self::Leased
                | Self::Running
                | Self::WaitingInput
                | Self::WaitingTakeover
        )
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    pub fn can_transition(self, to: Self) -> bool {
        matches!(
            (self, to),
            (Self::Queued, Self::Leased | Self::Cancelled)
                | (Self::Leased, Self::Running | Self::Queued | Self::Cancelled)
                | (
                    Self::Running,
                    Self::WaitingInput
                        | Self::WaitingTakeover
                        | Self::Completed
                        | Self::Failed
                        | Self::Cancelled
                        | Self::Leased
                )
                | (
                    Self::WaitingInput,
                    Self::Queued | Self::Leased | Self::Cancelled
                )
                | (
                    Self::WaitingTakeover,
                    Self::Queued | Self::Leased | Self::Cancelled
                )
                | (Self::Failed, Self::Queued)
        )
    }
}

pub const ACTIVE_RUN_STATUSES: &[RunStatus] = &[
    RunStatus::Queued,
    RunStatus::Leased,
    RunStatus::Running,
    RunStatus::WaitingInput,
    RunStatus::WaitingTakeover,
];
