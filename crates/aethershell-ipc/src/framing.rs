use crate::protocol::{IpcRequest, IpcResponse, IPC_PROTOCOL_VERSION, MAX_IPC_MESSAGE_SIZE};
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum FramingError {
    #[error("Message exceeds maximum allowed size of {0} bytes: got {1}")]
    MessageTooLarge(usize, usize),
    #[error("Incomplete frame: expected {0} bytes, got {1}")]
    IncompleteFrame(usize, usize),
    #[error("JSON decode error: {0}")]
    JsonDecode(String),
    #[error("Unsupported protocol version: got {0}, expected {1}")]
    UnsupportedProtocolVersion(u32, u32),
    #[error("IO error: {0}")]
    Io(String),
}

/// Encodes a request with a 4-byte little-endian length prefix.
pub fn encode_request(req: &IpcRequest) -> Result<Vec<u8>, FramingError> {
    let json_bytes =
        serde_json::to_vec(req).map_err(|e| FramingError::JsonDecode(e.to_string()))?;
    if json_bytes.len() > MAX_IPC_MESSAGE_SIZE {
        return Err(FramingError::MessageTooLarge(
            MAX_IPC_MESSAGE_SIZE,
            json_bytes.len(),
        ));
    }

    let mut out = Vec::with_capacity(4 + json_bytes.len());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&json_bytes);
    Ok(out)
}

/// Encodes a response with a 4-byte little-endian length prefix.
pub fn encode_response(res: &IpcResponse) -> Result<Vec<u8>, FramingError> {
    let json_bytes =
        serde_json::to_vec(res).map_err(|e| FramingError::JsonDecode(e.to_string()))?;
    if json_bytes.len() > MAX_IPC_MESSAGE_SIZE {
        return Err(FramingError::MessageTooLarge(
            MAX_IPC_MESSAGE_SIZE,
            json_bytes.len(),
        ));
    }

    let mut out = Vec::with_capacity(4 + json_bytes.len());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&json_bytes);
    Ok(out)
}

/// Decodes an IPC request from a length-prefixed slice and validates protocol version.
pub fn decode_request(buf: &[u8]) -> Result<(IpcRequest, usize), FramingError> {
    if buf.len() < 4 {
        return Err(FramingError::IncompleteFrame(4, buf.len()));
    }

    let length = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if length > MAX_IPC_MESSAGE_SIZE {
        return Err(FramingError::MessageTooLarge(MAX_IPC_MESSAGE_SIZE, length));
    }

    if buf.len() < 4 + length {
        return Err(FramingError::IncompleteFrame(4 + length, buf.len()));
    }

    let payload = &buf[4..4 + length];
    let req: IpcRequest =
        serde_json::from_slice(payload).map_err(|e| FramingError::JsonDecode(e.to_string()))?;

    // Validate protocol version (Finding IVA-005)
    if req.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(FramingError::UnsupportedProtocolVersion(
            req.protocol_version,
            IPC_PROTOCOL_VERSION,
        ));
    }

    Ok((req, 4 + length))
}

/// Decodes an IPC response from a length-prefixed slice and validates protocol version.
pub fn decode_response(buf: &[u8]) -> Result<(IpcResponse, usize), FramingError> {
    if buf.len() < 4 {
        return Err(FramingError::IncompleteFrame(4, buf.len()));
    }

    let length = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if length > MAX_IPC_MESSAGE_SIZE {
        return Err(FramingError::MessageTooLarge(MAX_IPC_MESSAGE_SIZE, length));
    }

    if buf.len() < 4 + length {
        return Err(FramingError::IncompleteFrame(4 + length, buf.len()));
    }

    let payload = &buf[4..4 + length];
    let res: IpcResponse =
        serde_json::from_slice(payload).map_err(|e| FramingError::JsonDecode(e.to_string()))?;

    // Validate protocol version (Finding IVA-005)
    if res.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(FramingError::UnsupportedProtocolVersion(
            res.protocol_version,
            IPC_PROTOCOL_VERSION,
        ));
    }

    Ok((res, 4 + length))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_request() {
        let req = IpcRequest::new("get_status", serde_json::json!({ "detailed": true }));
        let encoded = encode_request(&req).expect("encode");
        let (decoded, consumed) = decode_request(&encoded).expect("decode");

        assert_eq!(consumed, encoded.len());
        assert_eq!(decoded.command, "get_status");
        assert_eq!(decoded.protocol_version, 1);
        assert_eq!(decoded.payload["detailed"], true);
    }

    #[test]
    fn test_roundtrip_response() {
        let res = IpcResponse::ok("test-id", serde_json::json!({ "running": true }));
        let encoded = encode_response(&res).expect("encode");
        let (decoded, consumed) = decode_response(&encoded).expect("decode");

        assert_eq!(consumed, encoded.len());
        assert_eq!(decoded.correlation_id, "test-id");
        assert!(decoded.success);
        assert_eq!(decoded.payload["running"], true);
    }

    #[test]
    fn test_reject_oversized_payload() {
        let mut buf = vec![0u8; 10];
        let fake_len = (2 * 1024 * 1024u32).to_le_bytes();
        buf[0..4].copy_from_slice(&fake_len);

        assert_eq!(
            decode_request(&buf),
            Err(FramingError::MessageTooLarge(
                MAX_IPC_MESSAGE_SIZE,
                2 * 1024 * 1024
            ))
        );
    }

    #[test]
    fn test_reject_unsupported_protocol_version_iva_005() {
        let mut req = IpcRequest::new("test", serde_json::json!({}));
        req.protocol_version = 99; // Unsupported
        let encoded = encode_request(&req).expect("encode");

        assert_eq!(
            decode_request(&encoded),
            Err(FramingError::UnsupportedProtocolVersion(
                99,
                IPC_PROTOCOL_VERSION
            ))
        );
    }
}
