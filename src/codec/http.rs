//! Send HTTP requests and responses asynchronously.
//!
//! This module has both an `HttpClientCodec` for an async HTTP client and an
//! `HttpServerCodec` for an async HTTP server.
use std::borrow::Cow;
use std::io::{self, BufReader, Write};
use std::error::Error;
use std::fmt::{self, Formatter, Display};

use bytes::{BufMut, BytesMut, Bytes};
use http::{Method, StatusCode, Uri};
use http::header::{HeaderMap, HeaderName, HeaderValue};
use httparse::{self, Request};
use hyper;
use tokio_io::codec::{Decoder, Encoder};

#[cfg(any(feature="sync", feature="async"))]
use http::Version;

const MAX_HEADERS: usize = 100;
pub type ParseRespose<T> = hyper::Result<Option<(MessageHead<T>, usize)>>;
pub struct MessageHead<T> {
	pub version: Version,
	pub subject: T,
	pub headers: HeaderMap,
}

#[derive(Clone, Copy)]
struct HeaderIndices {
    name: (usize, usize),
    value: (usize, usize),
}

pub struct RawStatus(pub u16, pub Cow<'static, str>);

fn record_header_indices(bytes: &[u8], headers: &[httparse::Header], indices: &mut [HeaderIndices]) {
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

struct HeadersAsBytesIter<'a> {
    headers: ::std::slice::Iter<'a, HeaderIndices>,
    slice: Bytes,
}

impl<'a> Iterator for HeadersAsBytesIter<'a> {
    type Item = (&'a str, Bytes);
    fn next(&mut self) -> Option<Self::Item> {
        self.headers.next().map(|header| {
            let name = unsafe {
                let bytes = ::std::slice::from_raw_parts(
                    self.slice.as_ref().as_ptr().offset(header.name.0 as isize),
                    header.name.1 - header.name.0
                );
                ::std::str::from_utf8_unchecked(bytes)
            };
            (name, self.slice.slice(header.value.0, header.value.1))
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
///# extern crate tokio_core;
///# extern crate tokio_io;
///# extern crate websocket;
///# extern crate hyper;
///use websocket::async::HttpClientCodec;
///# use websocket::async::futures::{Future, Sink, Stream};
///# use tokio_core::net::TcpStream;
///# use tokio_core::reactor::Core;
///# use tokio_io::AsyncRead;
///# use hyper::http::h1::Incoming;
///# use hyper::version::HttpVersion;
///# use hyper::header::Headers;
///# use hyper::method::Method;
///# use hyper::uri::Uri;
///
///# fn main() {
///let mut core = Core::new().unwrap();
///let addr = "crouton.net".parse().unwrap();
///
///let f = TcpStream::connect(&addr, &core.handle())
///    .and_then(|s| {
///        Ok(s.framed(HttpClientCodec))
///    })
///    .and_then(|s| {
///        s.send(Incoming {
///            version: HttpVersion::Http11,
///            subject: (Method::Get, Uri::AbsolutePath("/".to_string())),
///            headers: Headers::new(),
///        })
///    })
///    .map_err(|e| e.into())
///    .and_then(|s| s.into_future().map_err(|(e, _)| e))
///    .map(|(m, _)| println!("You got a crouton: {:?}", m));
///
///core.run(f).unwrap();
///# }
///```
pub struct HttpClientCodec;

fn split_off_http(src: &mut BytesMut) -> Option<BytesMut> {
	match src.windows(4).position(|i| i == b"\r\n\r\n") {
		Some(p) => Some(src.split_to(p + 4)),
		None => None,
	}
}

impl Encoder for HttpClientCodec {
	type Item = MessageHead<(Method, Uri)>;
	type Error = io::Error;

	fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
		// TODO: optomize this!
		let request = format!("{} {} {:?}\r\n{:?}\r\n",
		                      item.subject.0,
		                      item.subject.1,
		                      item.version,
		                      item.headers);
		let byte_len = request.as_bytes().len();
		if byte_len > dst.remaining_mut() {
			dst.reserve(byte_len);
		}
		dst.writer().write(request.as_bytes()).map(|_| ())
	}
}

impl Decoder for HttpClientCodec {
	type Item = MessageHead<RawStatus>;
	type Error = HttpCodecError;

	fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {

		if buf.len() == 0 {
			return Ok(None);
		}

		let mut headers_indices = [HeaderIndices {
			name: (0, 0),
			value: (0, 0)
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
					let status = try!(StatusCode::from_u16(code).map_err(|_| httparse::Error::Status));
					let reason = match status.canonical_reason() {
						Some(reason) if reason == res.reason.unwrap() => Cow::Borrowed(reason),
						_ => Cow::Owned(res.reason.unwrap().to_owned())
					};
					let version = if res.version.unwrap() == 1 {
						Version::HTTP_11
					} else {
						Version::HTTP_10
					};
					record_header_indices(bytes, &res.headers, &mut headers_indices);
					let headers_len = res.headers.len();
					(len, code, reason, version, headers_len)
				},
				httparse::Status::Partial => return Ok(None),
			}
		};

		let mut headers = HeaderMap::with_capacity(headers_len);

		let slice = buf.split_to(len).freeze();

		let new_headers = HeadersAsBytesIter {
			headers: headers_indices[..headers_len].iter(),
			slice: slice,
		};
		let new_headers = new_headers.map(|h| {
		    (
			h.0.parse::<HeaderName>().unwrap(),
			HeaderValue::from_bytes(h.1.as_ref()).unwrap()
		    )
		});
		headers.extend(new_headers);

		Ok(Some(MessageHead {
			version: version,
			subject: RawStatus(code, reason),
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
///# extern crate tokio_core;
///# extern crate tokio_io;
///# extern crate websocket;
///# extern crate hyper;
///# use std::io;
///use websocket::async::HttpServerCodec;
///# use websocket::async::futures::{Future, Sink, Stream};
///# use tokio_core::net::TcpStream;
///# use tokio_core::reactor::Core;
///# use tokio_io::AsyncRead;
///# use hyper::http::h1::Incoming;
///# use hyper::version::HttpVersion;
///# use hyper::header::Headers;
///# use hyper::method::Method;
///# use hyper::uri::Uri;
///# use hyper::status::StatusCode;
///# fn main() {
///
///let mut core = Core::new().unwrap();
///let addr = "nothing-to-see-here.com".parse().unwrap();
///
///let f = TcpStream::connect(&addr, &core.handle())
///   .map(|s| s.framed(HttpServerCodec))
///   .map_err(|e| e.into())
///   .and_then(|s| s.into_future().map_err(|(e, _)| e))
///   .and_then(|(m, s)| match m {
///       Some(ref m) if m.subject.0 == Method::Get => Ok(s),
///       _ => panic!(),
///   })
///   .and_then(|stream| {
///       stream
///          .send(Incoming {
///               version: HttpVersion::Http11,
///               subject: StatusCode::NotFound,
///               headers: Headers::new(),
///           })
///           .map_err(|e| e.into())
///   });
///
///core.run(f).unwrap();
///# }
///```
#[derive(Copy, Clone, Debug)]
pub struct HttpServerCodec;

impl Encoder for HttpServerCodec {
	type Item = MessageHead<StatusCode>;
	type Error = io::Error;

	fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
		// TODO: optomize this!
		let response = format!("{:?} {}\r\n{:?}\r\n", item.version, item.subject, item.headers);
		let byte_len = response.as_bytes().len();
		if byte_len > dst.remaining_mut() {
			dst.reserve(byte_len);
		}
		dst.writer().write(response.as_bytes()).map(|_| ())
	}
}

impl Decoder for HttpServerCodec {
	type Item = MessageHead<(Method, Uri)>;
	type Error = HttpCodecError;

	fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
		// check if we get a request from hyper
		// TODO: this is ineffecient, but hyper does not give us a better way to parse
		match split_off_http(src) {
			Some(buf) => {
				let mut reader = BufReader::with_capacity(buf.len(), &*buf as &[u8]);

				let mut req = Request::new(&mut []);
				req.parse(&buf);

				Ok(Some(MessageHead {
					version: match req.version {
						Some(0) => Version::HTTP_10,
						Some(1) => Version::HTTP_11,
						None | Some(_) => return Ok(None),
					},
					subject: (
						match req.method.unwrap().parse() {
							Ok(method) => method,
							Err(_) => return Ok(None),
						},
						match req.path.unwrap().parse() {
							Ok(path) => path,
							Err(_) => return Ok(None),
						},
					),
					headers: HeaderMap::new(),
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
	/// An error that occurs during the writing or reading of HTTP data
	/// from a socket.
	Io(io::Error),
	/// An error that occurs during the parsing of an HTTP request or response.
	Http(httparse::Error),
}

impl Display for HttpCodecError {
	fn fmt(&self, fmt: &mut Formatter) -> Result<(), fmt::Error> {
		fmt.write_str(self.description())
	}
}

impl Error for HttpCodecError {
	fn description(&self) -> &str {
		match *self {
			HttpCodecError::Io(ref e) => e.description(),
			HttpCodecError::Http(ref e) => e.description(),
		}
	}

	fn cause(&self) -> Option<&Error> {
		match *self {
			HttpCodecError::Io(ref error) => Some(error),
			HttpCodecError::Http(ref error) => Some(error),
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
		HttpCodecError::Http(err)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Cursor;
	use stream::ReadWritePair;
	use tokio_core::reactor::Core;
	use futures::{Stream, Sink, Future};
	use tokio_io::AsyncRead;
	use http::Version;
	use http::header::HeaderMap;

	#[test]
	fn test_client_http_codec() {
		let mut core = Core::new().unwrap();
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
			              Some(ref m) if StatusCode::from_u16(m.subject.0).unwrap() ==
			                             StatusCode::NOT_FOUND => Ok(()),
			              _ => Err(io::Error::new(io::ErrorKind::Other, "test failed").into()),
			          });
		core.run(f).unwrap();
	}

	#[test]
	fn test_server_http_codec() {
		let mut core = Core::new().unwrap();
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
		core.run(f).unwrap();
	}
}
