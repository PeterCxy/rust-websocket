//! Allows you to take an existing request or stream of data and convert it into a
//! WebSocket client.
use std::error::Error;
use std::io;
use std::iter::Iterator;
use std::fmt::{self, Formatter, Display};
use std::str::{self, FromStr};
use stream::Stream;

use unicase::Ascii;
use http::header::{HeaderMap, HeaderName, HeaderValue};
use http::header::{
	CONNECTION,
	ORIGIN,
	SEC_WEBSOCKET_ACCEPT,
	SEC_WEBSOCKET_EXTENSIONS,
	SEC_WEBSOCKET_KEY,
	SEC_WEBSOCKET_PROTOCOL,
	SEC_WEBSOCKET_VERSION,
	UPGRADE,
};
use http::{Method, StatusCode, Uri};

#[cfg(any(feature="sync", feature="async"))]
use http::{self, Version};
use httparse;

use codec;
use codec::http::RequestHead;
use header::{WebSocketAccept, WebSocketKey, WebSocketVersion};
use header::connection::{Connection, ConnectionOption};
use header::upgrade::{Protocol, ProtocolName, Upgrade};
use header::sec_websocket_extensions::Extension;

#[cfg(feature="async")]
pub mod async;

#[cfg(feature="sync")]
pub mod sync;

/// Intermediate representation of a half created websocket session.
/// Should be used to examine the client's handshake
/// accept the protocols requested, route the path, etc.
///
/// Users should then call `accept` or `reject` to complete the handshake
/// and start a session.
/// Note: if the stream in use is `AsyncRead + AsyncWrite`, then asynchronous
/// functions will be available when completing the handshake.
/// Otherwise if the stream is simply `Read + Write` blocking functions will be
/// available to complete the handshake.
pub struct WsUpgrade<S, B>
	where S: Stream
{
	/// The headers that will be used in the handshake response.
	pub headers: HeaderMap,
	/// The stream that will be used to read from / write to.
	pub stream: S,
	/// The handshake request, filled with useful metadata.
	pub request: RequestHead,
	/// Some buffered data from the stream, if it exists.
	pub buffer: B,
}

impl<S, B> WsUpgrade<S, B>
    where S: Stream
{
	/// Select a protocol to use in the handshake response.
	pub fn use_protocols(mut self, protocols: Vec<&str>) -> Self
	{
		self.headers.insert("Sec-WebSocket-Protocol", HeaderValue::from_str(&protocols.join(", ")).unwrap());
		self
	}

	/// Select multiple extensions to use in the connection
	pub fn use_extensions<I>(mut self, extensions: I) -> Self
		where I: IntoIterator<Item = Extension>
	{
		//let mut extensions: Vec<Extension> = extensions.into_iter().collect().join(", ");
		self.headers.insert("Sec-WebSocket-Extensions", HeaderValue::from_static(""));
		/*upsert_header!(self.headers; WebSocketExtensions; {
			Some(protos) => protos.0.append(&mut extensions),
			None => WebSocketExtensions(extensions)
		});*/
		self
	}

	/// Drop the connection without saying anything.
	pub fn drop(self) {
		::std::mem::drop(self);
	}

	/// A list of protocols requested from the client.
	pub fn protocols(&self) -> Vec<&str> {
		self.request
			.headers
			.get(SEC_WEBSOCKET_PROTOCOL)
			.map(|e| {
				str::from_utf8(e.as_bytes()).unwrap()
					.split(',')
					.filter_map(|x| match x.trim() {
						"" => None,
						y => Some(y),
					})
					.collect::<Vec<&str>>()
			})
			.unwrap_or(vec![])
	}

	/// A list of extensions requested from the client.
	pub fn extensions(&self) -> Vec<Extension> {
		self.request
			.headers
			.get(SEC_WEBSOCKET_EXTENSIONS)
			.map(|e| {
				str::from_utf8(e.as_bytes()).unwrap()
					.split(',')
					.filter_map(|x| match x.trim() {
						"" => None,
						y => Some(y),
					})
					.filter_map(|x| match Extension::from_str(x) {
						Ok(ext) => Some(ext),
						_ => None,
					})
					.collect::<Vec<Extension>>()
			}).unwrap_or(vec![])
	}

	/// The client's websocket accept key.
	pub fn key(&self) -> Option<[u8; 16]> {
		self.request
			.headers
			.get(SEC_WEBSOCKET_KEY)
			.map(|k| {
				let k = k.as_bytes();
				let mut key = [0u8; 16];
				for (&x, p) in k.iter().zip(key.iter_mut()) {
					*p = x;
				}
				key
			})
	}

	/// The client's websocket version.
	pub fn version(&self) -> Option<WebSocketVersion> {
		match self.request.headers.get("Sec-WebSocket-Version") {
			Some(value) => Some(WebSocketVersion::from_str(value.to_str().unwrap()).unwrap()),
			_ => None,
		}
	}

	/// Origin of the client
	pub fn origin(&self) -> Option<&str> {
		self.request.headers.get("Origin")
			.map(|o| str::from_utf8(o.as_ref()).unwrap())
	}

	#[cfg(feature="sync")]
	fn send(&mut self, status: StatusCode) -> io::Result<()> {
		write!(&mut self.stream, "{:?} {}\r\n", self.request.version, status)?;
		write!(&mut self.stream, "{:?}\r\n", self.headers)?;
		Ok(())
	}

	#[doc(hidden)]
	pub fn prepare_headers(&mut self, custom: Option<HeaderMap>) -> StatusCode {
		if let Some(headers) = custom {
			self.headers.extend(headers.into_iter());
		}
		// NOTE: we know there is a key because this is a valid request
		// i.e. to construct this you must go through the validate function
		let key = self.request.headers.get(SEC_WEBSOCKET_KEY).unwrap();
		let key = WebSocketKey::from_str(key.to_str().unwrap()).unwrap();
		self.headers.append(HeaderName::from_bytes("Sec-WebSocket-Accept".as_bytes()).unwrap(), WebSocketAccept::new(key).into());
		self.headers.append(
			HeaderName::from_bytes("Connection".as_bytes()).unwrap(),
			Connection(vec![
				ConnectionOption::ConnectionHeader(Ascii::new("Upgrade".to_string()))
			]).into()
		);
		self.headers.append(
			HeaderName::from_bytes("Upgrade".as_bytes()).unwrap(),
			Upgrade(vec![Protocol::new(ProtocolName::WebSocket, None)]).into()
		);

		StatusCode::SWITCHING_PROTOCOLS
	}
}

/// Errors that can occur when one tries to upgrade a connection to a
/// websocket connection.
#[derive(Debug)]
pub enum HyperIntoWsError {
	/// The HTTP method in a valid websocket upgrade request must be GET
	MethodNotGet,
	/// Currently HTTP 2 is not supported
	UnsupportedHttpVersion,
	/// Currently only WebSocket13 is supported (RFC6455)
	UnsupportedWebsocketVersion,
	/// A websocket upgrade request must contain a key
	NoSecWsKeyHeader,
	/// A websocket upgrade request must ask to upgrade to a `websocket`
	NoWsUpgradeHeader,
	/// A websocket upgrade request must contain an `Upgrade` header
	NoUpgradeHeader,
	/// A websocket upgrade request's `Connection` header must be `Upgrade`
	NoWsConnectionHeader,
	/// A websocket upgrade request must contain a `Connection` header
	NoConnectionHeader,
	/// IO error from reading the underlying socket
	Io(io::Error),
	/// 
	Http(codec::http::HttpCodecError),
}

impl Display for HyperIntoWsError {
	fn fmt(&self, fmt: &mut Formatter) -> Result<(), fmt::Error> {
		fmt.write_str(self.description())
	}
}

impl Error for HyperIntoWsError {
	fn description(&self) -> &str {
		use self::HyperIntoWsError::*;
		match *self {
			MethodNotGet => "Request method must be GET",
			UnsupportedHttpVersion => "Unsupported request HTTP version",
			UnsupportedWebsocketVersion => "Unsupported WebSocket version",
			NoSecWsKeyHeader => "Missing Sec-WebSocket-Key header",
			NoWsUpgradeHeader => "Invalid Upgrade WebSocket header",
			NoUpgradeHeader => "Missing Upgrade WebSocket header",
			NoWsConnectionHeader => "Invalid Connection WebSocket header",
			NoConnectionHeader => "Missing Connection WebSocket header",
			Io(ref e) => e.description(),
			Http(ref e) => e.description(),
		}
	}

	fn cause(&self) -> Option<&Error> {
		match *self {
			HyperIntoWsError::Io(ref e) => Some(e),
			HyperIntoWsError::Http(ref e) => Some(e),
			_ => None,
		}
	}
}

impl From<io::Error> for HyperIntoWsError {
	fn from(err: io::Error) -> Self {
		HyperIntoWsError::Io(err)
	}
}

impl From<httparse::Error> for HyperIntoWsError {
	fn from(err: httparse::Error) -> Self {
		HyperIntoWsError::Http(err.into())
	}
}

#[cfg(feature="async")]
impl From<::codec::http::HttpCodecError> for HyperIntoWsError {
	fn from(src: ::codec::http::HttpCodecError) -> Self {
		match src {
			::codec::http::HttpCodecError::Io(e) => HyperIntoWsError::Io(e),
			_ => HyperIntoWsError::Http(src.into()),
		}
	}
}

#[cfg(any(feature="sync", feature="async"))]
/// Check whether an incoming request is a valid WebSocket upgrade attempt.
pub fn validate(
	method: &Method,
	version: &Version,
	headers: &HeaderMap,
) -> Result<(), HyperIntoWsError> {

	if *method != Method::GET {
		return Err(HyperIntoWsError::MethodNotGet);
	}

	if *version == Version::HTTP_09 || *version == Version::HTTP_09 {
		return Err(HyperIntoWsError::UnsupportedHttpVersion);
	}

	if let Some(version) = headers.get(SEC_WEBSOCKET_VERSION)
		.map(|v| v.to_str().unwrap().parse::<WebSocketVersion>().unwrap()
	) {
		if version != WebSocketVersion::WebSocket13 {
			return Err(HyperIntoWsError::UnsupportedWebsocketVersion);
		}
	}

	if headers.get(SEC_WEBSOCKET_KEY).is_none() {
		return Err(HyperIntoWsError::NoSecWsKeyHeader);
	}

	match headers.get(UPGRADE)
		.map(|v| v.to_str().unwrap().parse().unwrap())
	{
		Some(Upgrade(ref upgrade)) => {
			if upgrade.iter().all(|u| u.name != ProtocolName::WebSocket) {
				return Err(HyperIntoWsError::NoWsUpgradeHeader);
			}
		}
		None => return Err(HyperIntoWsError::NoUpgradeHeader),
	};

	fn check_connection_header(headers: &[ConnectionOption]) -> bool {
		for header in headers {
			if let ConnectionOption::ConnectionHeader(ref h) = *header {
				if Ascii::new(h as &str) == Ascii::new("upgrade") {
					return true;
				}
			}
		}
		false
	}

	match headers.get(CONNECTION).map(|v| v.to_str().unwrap().parse().unwrap()) {
		Some(Connection(ref connection)) => {
			if !check_connection_header(connection) {
				return Err(HyperIntoWsError::NoWsConnectionHeader);
			}
		}
		None => return Err(HyperIntoWsError::NoConnectionHeader),
	};

	Ok(())
}
