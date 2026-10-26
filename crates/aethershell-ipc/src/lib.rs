//! Session-bound Local IPC protocol, framing, server, and client for AetherShell.

pub mod auth;
pub mod client;
pub mod framing;
pub mod protocol;
pub mod server;

pub use auth::{SessionToken, SessionTokenGuard};
pub use client::IpcClient;
pub use framing::{decode_request, decode_response, encode_request, encode_response, FramingError};
pub use protocol::{IpcRequest, IpcResponse, IPC_PROTOCOL_VERSION, MAX_IPC_MESSAGE_SIZE};
pub use server::{CurrentUserSecurityAttributes, IpcClientIdentity, IpcServer, RequestHandler};
