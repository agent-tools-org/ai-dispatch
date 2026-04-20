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

impl MessageDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::In => "in",
            Self::Out => "out",
        }
    }

    fn parse_str(value: &str) -> Option<Self> {
        match value {
            "in" => Some(Self::In),
            "out" => Some(Self::Out),
            _ => None,
        }
    }
}

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

impl MessageSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reply => "reply",
            Self::Steer => "steer",
            Self::UnstickAuto => "unstick-auto",
            Self::AgentAck => "agent-ack",
        }
    }

    fn parse_str(value: &str) -> Option<Self> {
        match value {
            "reply" => Some(Self::Reply),
            "steer" => Some(Self::Steer),
            "unstick-auto" => Some(Self::UnstickAuto),
            "agent-ack" => Some(Self::AgentAck),
            _ => None,
        }
    }
}

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
    pub acked_at: Option<DateTime<Local>>,
}
