use serde::{Deserialize, Serialize};

pub const IPC_PROTOCOL_VERSION: u32 = 1;
pub const MAX_IPC_MESSAGE_SIZE: usize = 1024 * 1024; // 1 MiB hard ceiling (PRD Section 3.2)

/// Versioned request envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IpcRequest {
    pub protocol_version: u32,
    pub message_id: String,
    pub command: String,
    pub payload: serde_json::Value,
    pub deadline_ms: u64,
}

impl IpcRequest {
    pub fn new(command: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            protocol_version: IPC_PROTOCOL_VERSION,
            message_id: uuid::Uuid::new_v4().to_string(),
            command: command.into(),
            payload,
            deadline_ms: 5000,
        }
    }
}

/// Versioned response envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IpcResponse {
    pub protocol_version: u32,
    pub correlation_id: String,
    pub success: bool,
    pub payload: serde_json::Value,
    pub error_message: Option<String>,
}

impl IpcResponse {
    pub fn ok(correlation_id: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            protocol_version: IPC_PROTOCOL_VERSION,
            correlation_id: correlation_id.into(),
            success: true,
            payload,
            error_message: None,
        }
    }

    pub fn error(correlation_id: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            protocol_version: IPC_PROTOCOL_VERSION,
            correlation_id: correlation_id.into(),
            success: false,
            payload: serde_json::Value::Null,
            error_message: Some(error.into()),
        }
    }
}
