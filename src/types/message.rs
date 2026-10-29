// Task message structs and enums for reply/steer delivery tracking.
// Exports: MessageDirection, MessageSource, TaskMessage.
// Deps: chrono and parent crate::types::TaskId.

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use super::TaskId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageDirection {
    In,
    Out,
}

super::string_values::string_values!(
    MessageDirection, as_str(&self), parse_str {
        In => "in",
        Out => "out",
    }
);

impl TryFrom<&str> for MessageDirection {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse_str(value).ok_or_else(|| format!("unknown message direction: {value}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MessageSource {
    Reply,
    Steer,
    UnstickAuto,
    AgentAck,
}

super::string_values::string_values!(
    MessageSource, as_str(&self), parse_str {
        Reply => "reply",
        Steer => "steer",
        UnstickAuto => "unstick-auto",
        AgentAck => "agent-ack",
    }
);

impl TryFrom<&str> for MessageSource {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse_str(value).ok_or_else(|| format!("unknown message source: {value}"))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskMessage {
    pub id: i64,
    pub task_id: TaskId,
    pub direction: MessageDirection,
    pub content: String,
    pub source: MessageSource,
    pub created_at: DateTime<Local>,
    pub delivered_at: Option<DateTime<Local>>,
}
