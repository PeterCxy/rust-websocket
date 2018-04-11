//! Allows you to take an existing request or stream of data and convert it into a
//! WebSocket client.
use std::io::{self, BufRead};
use std::net::TcpStream;

use client::sync::Client;
use codec::http::{RequestHead, RequestLine};
use server::upgrade::{WsUpgrade, HyperIntoWsError, validate};
use stream::sync::{Stream, AsTcpStream};

use std::io::BufReader;
use http::{self, StatusCode};
use http::header::HeaderMap;
use httparse;

/// This crate uses buffered readers to read in the handshake quickly, in order to
/// interface with other use cases that don't use buffered readers the buffered readers
/// is deconstructed when it is returned to the user and given as the underlying
/// reader and the buffer.
///
/// This struct represents bytes that have already been read in from the stream.
/// A slice of valid data in this buffer can be obtained by: `&buf[pos..cap]`.
#[derive(Debug)]
pub struct Buffer {
	/// the contents of the buffered stream data
	pub buf: Vec<u8>,
	/// the current position of cursor in the buffer
	/// Any data before `pos` has already been read and parsed.
	pub pos: usize,
	/// the last location of valid data
	/// Any data after `cap` is not valid.
	pub cap: usize,
}

/// If you have your requests separate from your stream you can use this struct
/// to upgrade the connection based on the request given
/// (the request should be a handshake).
pub struct RequestStreamPair<S: Stream>(pub S, pub RequestHead);

/// The synchronous specialization of `WsUpgrade`.
/// See the `WsUpgrade` docs for usage and the extra synchronous methods
/// given by this specialization.
pub type Upgrade<S> = WsUpgrade<S, Option<Buffer>>;

/// These methods are the synchronous ways of accepting and rejecting a websocket
/// handshake.
impl<S> WsUpgrade<S, Option<Buffer>>
where
	S: Stream + Send,
{
	/// Accept the handshake request and send a response,
	/// if nothing goes wrong a client will be created.
	pub fn accept(self) -> Result<Client<S>, (S, io::Error)> {
		self.internal_accept(None)
	}

	/// Accept the handshake request and send a response while
	/// adding on a few headers. These headers are added before the required
	/// headers are, so some might be overwritten.
	pub fn accept_with(self, custom_headers: HeaderMap) -> Result<Client<S>, (S, io::Error)> {
		self.internal_accept(Some(custom_headers))
	}

	fn internal_accept(mut self, headers: Option<HeaderMap>) -> Result<Client<S>, (S, io::Error)> {
		let status = self.prepare_headers(headers);

		if let Err(e) = self.send(status) {
			return Err((self.stream, e));
		}

		Ok(Client::unchecked(
			BufReader::new(self.stream),
			self.headers,
			false,
			true,
		))
	}

	/// Reject the client's request to make a websocket connection.
	pub fn reject(self) -> Result<S, (S, io::Error)> {
		self.internal_reject(None)
	}

	/// Reject the client's request to make a websocket connection
	/// and send extra headers.
	pub fn reject_with(self, headers: HeaderMap) -> Result<S, (S, io::Error)> {
		self.internal_reject(Some(headers))
	}

	fn internal_reject(mut self, headers: Option<HeaderMap>) -> Result<S, (S, io::Error)> {
		if let Some(custom) = headers {
			self.headers.extend(custom.into_iter());
		}
		match self.send(StatusCode::BAD_REQUEST) {
			Ok(()) => Ok(self.stream),
			Err(e) => Err((self.stream, e)),
		}
	}
}

impl<S, B> WsUpgrade<S, B>
where
	S: Stream + AsTcpStream + Send,
	B: Send,
{
	/// Get a handle to the underlying TCP stream, useful to be able to set
	/// TCP options, etc.
	pub fn tcp_stream(&self) -> &TcpStream {
		self.stream.as_tcp()
	}
}

/// Trait to take a stream or similar and attempt to recover the start of a
/// websocket handshake from it.
/// Should be used when a stream might contain a request for a websocket session.
///
/// If an upgrade request can be parsed, one can accept or deny the handshake with
/// the `WsUpgrade` struct.
/// Otherwise the original stream is returned along with an error.
///
/// Note: the stream is owned because the websocket client expects to own its stream.
///
/// This is already implemented for all Streams, which means all types with Read + Write.
///
/// # Example
///
/// ```rust,no_run
/// use std::net::TcpListener;
/// use std::net::TcpStream;
/// use websocket::sync::server::upgrade::IntoWs;
/// use websocket::sync::Client;
///
/// let listener = TcpListener::bind("127.0.0.1:80").unwrap();
///
/// for stream in listener.incoming().filter_map(Result::ok) {
///     let mut client: Client<TcpStream> = match stream.into_ws() {
/// 		    Ok(upgrade) => {
///             match upgrade.accept() {
///                 Ok(client) => client,
///                 Err(_) => panic!(),
///             }
///         },
/// 		    Err(_) => panic!(),
///     };
/// }
/// ```
pub trait IntoWs {
	/// The type of stream this upgrade process is working with (TcpStream, etc.)
	type Stream: Stream + Send;
	/// An error value in case the stream is not asking for a websocket connection
	/// or something went wrong. It is common to also include the stream here.
	type Error;
	/// Attempt to parse the start of a Websocket handshake, later with the  returned
	/// `WsUpgrade` struct, call `accept` to start a websocket client, and `reject` to
	/// send a handshake rejection response.
	fn into_ws(self) -> Result<Upgrade<Self::Stream>, Self::Error>;
}

impl<S> IntoWs for S
where
	S: Stream + Send,
{
	type Stream = S;
	type Error = (S, Option<RequestHead>, Option<Buffer>, HyperIntoWsError);

	fn into_ws(self) -> Result<Upgrade<Self::Stream>, Self::Error> {

		let mut buf = String::new();
		let mut reader = BufReader::new(self);
		reader.read_line(&mut buf);

		// cleanup
		let buf2 = buf.clone();

		let mut parse = httparse::Request::new(&mut []);

		let stream = reader.into_inner();

		match parse.parse(buf2.as_bytes()) {
			Ok(o) => {
				match o {
					httparse::Status::Complete(_) => {}
					_ => return Err((stream, None, None, ::httparse::Error::HeaderValue.into())),
				}
			}
			Err(e) => return Err((stream, None, None, e.into())),
		}

		let cap = buf.len();
		let pos = buf.len();
		let buffer = Some(Buffer {
			buf: buf.into_bytes(),
			cap: cap,
			pos: pos,
		});

		let request = RequestHead {
			version: match parse.version {
				Some(0) => http::Version::HTTP_10,
				Some(1) => http::Version::HTTP_11,
				Some(_) | None => {
					return Err((stream, None, buffer, httparse::Error::Version.into()))
				}
			},
			subject: RequestLine(
				match parse.method.unwrap().parse() {
					Ok(method) => method,
					Err(e) => {
						return Err((stream, None, buffer, httparse::Error::HeaderValue.into()))
					}
				},
				match parse.path.unwrap().parse() {
					Ok(path) => path,
					Err(e) => {
						return Err((stream, None, buffer, httparse::Error::HeaderValue.into()))
					}
				},
			),
			headers: HeaderMap::new(),
		};

		match validate(&request.subject.0, &request.version, &request.headers) {
			Ok(_) => {
				Ok(WsUpgrade {
					headers: HeaderMap::new(),
					stream: stream,
					request: request,
					buffer: buffer,
				})
			}
			Err(e) => Err((stream, Some(request), buffer, e)),
		}
	}
}

impl<S> IntoWs for RequestStreamPair<S>
where
	S: Stream + Send,
{
	type Stream = S;
	type Error = (S, RequestHead, HyperIntoWsError);

	fn into_ws(self) -> Result<Upgrade<Self::Stream>, Self::Error> {
		match validate(&self.1.subject.0, &self.1.version, &self.1.headers) {
			Ok(_) => {
				Ok(WsUpgrade {
					headers: HeaderMap::new(),
					stream: self.0,
					request: self.1,
					buffer: None,
				})
			}
			Err(e) => Err((self.0, self.1, e)),
		}
	}
}

/// Upgrade a hyper connection to a websocket one.
///
/// A hyper request is implicitly defined as a stream from other `impl`s of Stream.
/// Until trait impl specialization comes along, we use this struct to differentiate
/// a hyper request (which already has parsed headers) from a normal stream.
///
/// Using this method, one can start a hyper server and check if each request
/// is a websocket upgrade request, if so you can use websockets and hyper on the
/// same port!
////
//// ```rust,no_run
//// # extern crate hyper;
//// # extern crate websocket;
//// # fn main() {
//// use hyper::server::{Server, Request, Response};
//// use websocket::Message;
//// use websocket::sync::server::upgrade::IntoWs;
//// use websocket::sync::server::upgrade::HyperRequest;
////
//// Server::http("0.0.0.0:80").unwrap().handle(move |req: Request, res: Response| {
////     match HyperRequest(req).into_ws() {
////         Ok(upgrade) => {
////             // `accept` sends a successful handshake, no need to worry about res
////             let mut client = match upgrade.accept() {
////                 Ok(c) => c,
////                 Err(_) => panic!(),
////             };
////
////             client.send_message(&Message::text("its free real estate"));
////         },
////
////         Err((request, err)) => {
////             // continue using the request as normal, "echo uri"
////             res.send(b"Try connecting over ws instead.").unwrap();
////         },
////     };
//// })
//// .unwrap();
//// # }
//// ```
pub struct HyperRequest(); //pub ::hyper::server::Request);

/*impl IntoWs for HyperRequest {
	type Stream = &'static mut Body;
	type Error = (::hyper::server::Request, HyperIntoWsError);

	fn into_ws(self) -> Result<Upgrade<Self::Stream>, Self::Error> {
		if let Err(e) = validate(&self.0.method(), &self.0.version(), &self.0.headers()) {
			return Err((self.0, e));
		}

		let (method, uri, version, headers, body) =
			self.0.deconstruct();

		Ok(Upgrade {
			headers: Headers::new(),
			stream: body,
			buffer: None,
			request: ::codec::http::MessageHead {
				version: version,
				headers: headers,
				subject: (method, uri),
			},
		})
	}
}*/
