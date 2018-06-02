//! Send HTTP requests and responses asynchronously.
//!
//! This module has both an `HttpClientCodec` for an async HTTP client and an
//! `HttpServerCodec` for an async HTTP server.
use std::borrow::Cow;
use std::io::{self, BufReader, Write};
use std::error::Error;
use std::fmt::{self, Formatter, Display};

use bytes::{BufMut, BytesMut, Bytes};
use http::{self, Method, StatusCode, Uri};
use http::header::{HeaderMap, HeaderName, HeaderValue};
use httparse::{self, Request};
use hyper;
use tokio_io::codec::{Decoder, Encoder};

#[cfg(any(feature = "sync", feature = "async"))]
use http::Version;

pub const MAX_HEADERS: usize = 100;
pub type ParseRespose<T> = hyper::Result<Option<(MessageHead<T>, usize)>>;

/// An incoming request message.
pub type RequestHead = MessageHead<RequestLine>;

#[derive(Debug, Default, PartialEq)]
pub struct RequestLine(pub Method, pub Uri);

/// An incoming response message.
pub type ResponseHead = MessageHead<StatusCode>;

#[derive(Debug)]
pub struct MessageHead<T> {
	pub version: Version,
	pub subject: T,
	pub headers: HeaderMap,
}

#[derive(Clone, Copy)]
pub struct HeaderIndices {
	pub name: (usize, usize),
	pub value: (usize, usize),
}

pub fn record_header_indices(
	bytes: &[u8],
	headers: &[httparse::Header],
	indices: &mut [HeaderIndices],
) {
	let bytes_ptr = bytes.as_ptr() as usize;
	for (header, indices) in headers.iter().zip(indices.iter_mut()) {
		let name_start = header.name.as_ptr() as usize - bytes_ptr;
		let name_end = name_start + header.name.len();
		indices.name = (name_start, name_end);
		let value_start = header.value.as_ptr() as usize - bytes_ptr;
		let value_end = value_start + header.value.len();
		indices.value = (value_start, value_end);
	}
}

pub struct HeadersAsBytesIter<'a> {
	pub headers: ::std::slice::Iter<'a, HeaderIndices>,
	pub slice: Bytes,
}

impl<'a> Iterator for HeadersAsBytesIter<'a> {
	type Item = (HeaderName, HeaderValue);
	fn next(&mut self) -> Option<Self::Item> {
		self.headers.next().map(|header| {
			let name = unsafe {
				let bytes = ::std::slice::from_raw_parts(
					self.slice.as_ref().as_ptr().offset(header.name.0 as isize),
					header.name.1 - header.name.0,
				);
				::std::str::from_utf8_unchecked(bytes)
			};
			let name =
				HeaderName::from_bytes(name.as_bytes()).expect("header name already validated");
			let value = unsafe {
				HeaderValue::from_shared_unchecked(self.slice.slice(header.value.0, header.value.1))
			};
			(name, value)
		})
	}
}

#[derive(Copy, Clone, Debug)]
///A codec to be used with `tokio` codecs that can serialize HTTP requests and
///deserialize HTTP responses. One can use this on it's own without websockets to
///make a very bare async HTTP server.
///
///# Example
///```rust,no_run
///# extern crate tokio_io;
///# extern crate tokio;
///# extern crate websocket;
///# extern crate http;
///# extern crate hyper;
///use websocket::async::HttpClientCodec;
///use websocket::codec::http::MessageHead;
///# use websocket::async::futures::{Future, Sink, Stream};
///# use tokio::net::TcpStream;
///# use tokio_io::AsyncRead;
///# use http::{Method, Uri, Version};
///# use http::header::HeaderMap;
///
///# fn main() {
///let addr = "crouton.net".parse().unwrap();
///
///let f = TcpStream::connect(&addr)
///    .and_then(|s| {
///        Ok(s.framed(HttpClientCodec))
///    })
///    .and_then(|s| {
///        s.send(MessageHead {
///            version: Version::HTTP_11,
///            subject: (Method::GET, "/".parse().unwrap()),
///            headers: HeaderMap::new(),
///        })
///    })
///    .map_err(|e| e.into())
///    .and_then(|s| s.into_future().map_err(|(e, _)| e))
///    .map(|(m, _)| println!("You got a crouton: {:?}", m));
///
///tokio::run(f.map(|_| ()).map_err(|_| ()));
///# }
///```
pub struct HttpClientCodec;

fn split_off_http(src: &mut BytesMut) -> Option<BytesMut> {
	match src.windows(4).position(|i| i == b"\r\n\r\n") {
		Some(p) => Some(src.split_to(p + 4)),
		None => None,
	}
}

fn write_headers(headers: &HeaderMap, dst: &mut BytesMut) {
	for (name, value) in headers {
		dst.extend(name.as_str().as_bytes());
		dst.extend(b": ");
		dst.extend(value.as_bytes());
		dst.extend(b"\r\n");
	}
}

impl Encoder for HttpClientCodec {
	type Item = MessageHead<(Method, Uri)>;
	type Error = io::Error;

	fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {

		// TODO: optomize this!
		let mut request = format!(
			"{} {} {:?}\r\n",
			item.subject.0, item.subject.1, item.version
		).to_owned();

		for (key, value) in item.headers.iter() {
			request += &format!("{}: {}\r\n", key.as_str(), value.to_str().unwrap());
		}

		request += "\r\n";

		let byte_len = request.as_bytes().len();
		if byte_len > dst.remaining_mut() {
			dst.reserve(byte_len);
		}

		dst.writer().write(request.as_bytes()).map(|_| ())

	}
}

impl Decoder for HttpClientCodec {
	type Item = ResponseHead;
	type Error = HttpCodecError;

	fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {

		if buf.len() == 0 {
			return Ok(None);
		}

		let mut headers_indices = [HeaderIndices {
			name: (0, 0),
			value: (0, 0),
		}; MAX_HEADERS];

		let (len, code, reason, version, headers_len) = {
			let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
			//trace!("Response.parse([Header; {}], [u8; {}])", headers.len(), buf.len());
			let mut res = httparse::Response::new(&mut headers);
			let bytes = buf.as_ref();
			match res.parse(bytes).unwrap_or(httparse::Status::Partial) {
				httparse::Status::Complete(len) => {
					//trace!("Response.parse Complete({})", len);
					let code = res.code.unwrap();
					let status = try!(StatusCode::from_u16(code).map_err(
						|_| httparse::Error::Status,
					));
					let reason = match status.canonical_reason() {
						Some(reason) if reason == res.reason.unwrap() => Cow::Borrowed(reason),
						_ => Cow::Owned(res.reason.unwrap().to_owned()),
					};
					let version = if res.version.unwrap() == 1 {
						Version::HTTP_11
					} else {
						Version::HTTP_10
					};
					record_header_indices(bytes, &res.headers, &mut headers_indices);
					let headers_len = res.headers.len();
					(len, code, reason, version, headers_len)
				}
				httparse::Status::Partial => return Ok(None),
			}
		};

		let mut headers = HeaderMap::with_capacity(headers_len);

		let slice = buf.split_to(len).freeze();

		let new_headers = HeadersAsBytesIter {
			headers: headers_indices[..headers_len].iter(),
			slice: slice,
		};
		headers.extend(new_headers);

		Ok(Some(MessageHead {
			version: version,
			subject: StatusCode::from_u16(code).unwrap(),
			headers: headers,
		}))
	}
}

///A codec that can be used with streams implementing `AsyncRead + AsyncWrite`
///that can serialize HTTP responses and deserialize HTTP requests. Using this
///with an async `TcpStream` will give you a very bare async HTTP server.
///
///This crate sends out one HTTP request / response in order to perform the websocket
///handshake then never talks HTTP again. Because of this an async HTTP implementation
///is needed.
///
///# Example
///
///```rust,no_run
///# extern crate tokio;
///# extern crate tokio_io;
///# extern crate websocket;
///# extern crate http;
///# extern crate hyper;
///# use std::io;
///use websocket::async::HttpServerCodec;
///# use websocket::codec::http::MessageHead;
///# use websocket::async::futures::{Future, Sink, Stream};
///# use tokio::net::TcpStream;
///# use tokio_io::AsyncRead;
///# use http::header::HeaderMap;
///# use http::{Method, StatusCode, Uri, Version};
///# fn main() {
///
///let addr = "nothing-to-see-here.com".parse().unwrap();
///
///let f = TcpStream::connect(&addr)
///   .map(|s| s.framed(HttpServerCodec))
///   .map_err(|e| e.into())
///   .and_then(|s| s.into_future().map_err(|(e, _)| e))
///   .and_then(|(m, s)| match m {
///       Some(ref m) if m.subject.0 == Method::GET => Ok(s),
///       _ => panic!(),
///   })
///   .and_then(|stream| {
///       stream
///           .send(MessageHead {
///               version: Version::HTTP_11,
///               subject: StatusCode::NOT_FOUND,
///               headers: HeaderMap::new(),
///           })
///           .map_err(|e| e.into())
///   });
///
///tokio::run(f.map(|_| ()).map_err(|_| ()));
///# }
///```
#[derive(Copy, Clone, Debug)]
pub struct HttpServerCodec;

impl Encoder for HttpServerCodec {
	type Item = ResponseHead;
	type Error = io::Error;

	fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
		dst.extend(
			format!("{:?} {}\r\n", item.version, item.subject).as_bytes(),
		);
		write_headers(&item.headers, dst);
		dst.extend(b"\r\n");
		Ok(())
	}
}

impl Decoder for HttpServerCodec {
	type Item = RequestHead;
	type Error = HttpCodecError;

	fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
		// check if we get a request from hyper
		// TODO: this is ineffecient, but hyper does not give us a better way to parse
		match split_off_http(src) {
			Some(mut buf) => {

				if buf.len() == 0 {
					return Ok(None);
				}

				let mut headers_indices = [HeaderIndices {
					name: (0, 0),
					value: (0, 0),
				}; MAX_HEADERS];

				let (len, method, path, version, headers_len) = {
					let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
					//println!("Request.parse([Header; {}], [u8; {}])", headers.len(), buf.len());
					let mut req = httparse::Request::new(&mut headers);
					match try!(req.parse(&buf)) {
						httparse::Status::Complete(len) => {
							//println!("Request.parse Complete({})", len);
							let method = Method::from_bytes(req.method.unwrap().as_bytes())?;
							let path = req.path.unwrap();
							let bytes_ptr = buf.as_ref().as_ptr() as usize;
							let path_start = path.as_ptr() as usize - bytes_ptr;
							let path_end = path_start + path.len();
							let path = (path_start, path_end);
							let version = if req.version.unwrap() == 1 {
								Version::HTTP_11
							} else {
								Version::HTTP_10
							};

							record_header_indices(buf.as_ref(), &req.headers, &mut headers_indices);
							let headers_len = req.headers.len();
							(len, method, path, version, headers_len)
						}
						httparse::Status::Partial => return Ok(None),
					}
				};

				let mut headers = HeaderMap::with_capacity(headers_len);
				let slice = buf.split_to(len).freeze();
				let path = slice.slice(path.0, path.1);

				// path was found to be utf8 by httparse
				let path = Uri::from_shared(path)?;
				let subject = RequestLine(method, path);

				headers.extend(HeadersAsBytesIter {
					headers: headers_indices[..headers_len].iter(),
					slice: slice,
				});


				Ok(Some(RequestHead {
					version: version,
					subject: subject,
					headers: headers,
				}))

			}
			None => Ok(None),
		}
	}
}

/// Any error that can happen during the writing or parsing of HTTP requests
/// and responses. This consists of HTTP parsing errors (the `Http` variant) and
/// errors that can occur when writing to IO (the `Io` variant).
#[derive(Debug)]
pub enum HttpCodecError {
	/// An invalid `Method`, such as `GE,T`.
	Method,
	/// An invalid `HttpVersion`, such as `HTP/1.1`
	Version,
	/// Uri Errors
	Uri,
	/// An invalid `Header`.
	Header,
	/// A message head is too large to be reasonable.
	TooLarge,
	/// An invalid `Status`, such as `1337 ELITE`.
	Status,
	/// An error that occurs during the writing or reading of HTTP data
	/// from a socket.
	Io(io::Error),
}

impl Display for HttpCodecError {
	fn fmt(&self, fmt: &mut Formatter) -> Result<(), fmt::Error> {
		fmt.write_str(self.description())
	}
}

impl Error for HttpCodecError {
	fn description(&self) -> &str {
		match *self {
			HttpCodecError::Method => "invalid Method specified",
			HttpCodecError::Version => "invalid HTTP version specified",
			HttpCodecError::Uri => "invalid URI",
			HttpCodecError::Header => "invalid Header provided",
			HttpCodecError::TooLarge => "message head is too large",
			HttpCodecError::Status => "invalid Status provided",
			HttpCodecError::Io(ref e) => e.description(),
		}
	}

	fn cause(&self) -> Option<&Error> {
		match *self {
			HttpCodecError::Io(ref error) => Some(error),
			_ => None,
		}
	}
}

impl From<io::Error> for HttpCodecError {
	fn from(err: io::Error) -> HttpCodecError {
		HttpCodecError::Io(err)
	}
}

impl From<httparse::Error> for HttpCodecError {
	fn from(err: httparse::Error) -> HttpCodecError {
		match err {
			httparse::Error::HeaderName |
			httparse::Error::HeaderValue |
			httparse::Error::NewLine |
			httparse::Error::Token => HttpCodecError::Header,
			httparse::Error::Status => HttpCodecError::Status,
			httparse::Error::TooManyHeaders => HttpCodecError::TooLarge,
			httparse::Error::Version => HttpCodecError::Version,
		}
	}
}

impl From<http::method::InvalidMethod> for HttpCodecError {
	fn from(_: http::method::InvalidMethod) -> HttpCodecError {
		HttpCodecError::Method
	}
}

impl From<http::uri::InvalidUriBytes> for HttpCodecError {
	fn from(_: http::uri::InvalidUriBytes) -> HttpCodecError {
		HttpCodecError::Uri
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Cursor;
	use stream::ReadWritePair;
	use futures::{Stream, Sink, Future};
	use tokio;
	use tokio_io::AsyncRead;
	use http::Version;
	use http::header::HeaderMap;

	#[test]
	fn test_client_http_codec() {
		let response = "HTTP/1.1 404 Not Found\r\n\r\npssst extra data here";
		let input = Cursor::new(response.as_bytes());
		let output = Cursor::new(Vec::new());

		let f = ReadWritePair(input, output)
			.framed(HttpClientCodec)
			.send(MessageHead {
				version: Version::HTTP_11,
				subject: (Method::GET, "/".to_string().parse().unwrap()),
				headers: HeaderMap::new(),
			})
			.map_err(|e| e.into())
			.and_then(|s| s.into_future().map_err(|(e, _)| e))
			.and_then(|(m, _)| match m {
				Some(ref m) if m.subject == StatusCode::NOT_FOUND => Ok(()),
				_ => Err(io::Error::new(io::ErrorKind::Other, "test failed").into()),
			});
		tokio::run(f.map_err(|_| ()));
	}

	#[test]
	fn test_server_http_codec() {
		let request = "\
			GET / HTTP/1.0\r\n\
			Host: www.rust-lang.org\r\n\
			\r\n\
			"
		              .as_bytes();
		let input = Cursor::new(request);
		let output = Cursor::new(Vec::new());

		let f = ReadWritePair(input, output)
			.framed(HttpServerCodec)
			.into_future()
			.map_err(|(e, _)| e)
			.and_then(|(m, s)| match m {
				Some(ref m) if m.subject.0 == Method::GET => Ok(s),
				_ => Err(io::Error::new(io::ErrorKind::Other, "test failed").into()),
			})
			.and_then(|s| {
				s.send(MessageHead {
					version: Version::HTTP_11,
					subject: StatusCode::NOT_FOUND,
					headers: HeaderMap::new(),
				})
				 .map_err(|e| e.into())
			});
		tokio::run(f.map(|_| ()).map_err(|_| ()));
	}
}
