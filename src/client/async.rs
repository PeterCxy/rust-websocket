//! Contains the asynchronous websocket client.
//!
//! The async client is simply a `Stream + Sink` of `OwnedMessage` structs.
//! This definition means that you don't have to learn any new APIs other than
//! futures-rs.
//! The client simply wraps around an `AsyncRead + AsyncWrite` stream and uses
//! a `MessageCodec` to chop up the bytes into websocket messages.
//! See the `codec` module for all the cool codecs this crate has.
//!
//! Since this is all asynchronous, you will not create a client from `ClientBuilder`
//! but instead you will create a `ClientNew` struct, which is a Future that
//! will eventually evaluate to a `Client`.
//!
//! # Example with Type Annotations
//!
//! ```rust,no_run
//! # extern crate futures;
//! # extern crate tokio;
//! # extern crate websocket;
//! use tokio::reactor::Handle;
//! use websocket::ClientBuilder;
//! use websocket::async::client::{Client, ClientNew};
//! use websocket::async::TcpStream;
//! use websocket::futures::{Future, Stream, Sink};
//! use websocket::Message;
//! # fn main() {
//!
//! // create a Future of a client
//! let client_future: ClientNew<TcpStream> =
//!     ClientBuilder::new("ws://echo.websocket.org").unwrap()
//!         .async_connect_insecure(&Handle::default());
//!
//! // send a message
//! let send_future = client_future
//!     .and_then(|(client, headers)| {
//!         // just to make it clear what type this is
//!         let client: Client<TcpStream> = client;
//!         client.send(Message::text("hallo").into())
//!     });
//!
//! tokio::run(send_future.map(|_| ()).map_err(|_| ()));
//! # }
//! ```

pub use tokio::reactor::Handle;
pub use tokio_io::codec::Framed;
pub use tokio::net::TcpStream;
pub use futures::Future;
use http::header::HeaderMap;

use result::WebSocketError;
use codec::ws::MessageCodec;
use message::OwnedMessage;

#[cfg(feature = "async-ssl")]
pub use tokio_tls::TlsStream;

/// An asynchronous websocket client.
///
/// This is simply a `Stream` and `Sink` of `OwnedMessage`s.
/// See the docs for `Stream` and `Sink` to learn more about how to use
/// these futures.
pub type Client<S: Send> = Framed<S, MessageCodec<OwnedMessage>>;

/// A future which will evaluate to a `Client` and a set of hyper `Headers`.
///
/// The `Client` can send and receive websocket messages, and the Headers are
/// the headers that came back from the server handshake.
/// If the user used a protocol or attached some other headers check these response
/// headers to see if the server accepted the protocol or other custom header.
/// This crate will not automatically close the connection if the server refused
/// to use the user protocols given to it, you must check that the server accepted.
pub type ClientNew<S: Send> = Box<
	Future<Item = (Client<S>, HeaderMap), Error = WebSocketError>
		+ Send,
>;
