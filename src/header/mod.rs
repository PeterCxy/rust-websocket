//! Structs representing headers relevant in a WebSocket context.
//!
//! These headers are commonly used in WebSocket requests and responses.
//! The `Header` trait from the `hyper` crate is used.

pub use self::host::Host;
pub use self::origin::Origin;
pub use self::sec_websocket_key::WebSocketKey;
pub use self::sec_websocket_accept::WebSocketAccept;
pub use self::sec_websocket_protocol::WebSocketProtocol;
pub use self::sec_websocket_version::WebSocketVersion;
pub use self::sec_websocket_extensions::WebSocketExtensions;
pub use self::upgrade::Upgrade;

pub mod connection;
mod host;
mod origin;
mod sec_websocket_accept;
mod sec_websocket_key;
mod sec_websocket_protocol;
mod sec_websocket_version;
pub mod sec_websocket_extensions;
pub mod upgrade;