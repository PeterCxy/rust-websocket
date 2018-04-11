use base64;
//use hyper::header::parsing::from_one_raw_str;
use http::header::HeaderValue;
use std::fmt::{self, Debug};
use std::str::FromStr;
use header::WebSocketKey;
use result::{WebSocketResult, WebSocketError};
use sha1::Sha1;

static MAGIC_GUID: &'static str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Represents a Sec-WebSocket-Accept header
#[derive(PartialEq, Clone, Copy)]
pub struct WebSocketAccept([u8; 20]);

impl Debug for WebSocketAccept {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "WebSocketAccept({})", self)
	}
}

impl FromStr for WebSocketAccept {
	type Err = WebSocketError;

	fn from_str(accept: &str) -> WebSocketResult<WebSocketAccept> {
		match base64::decode(accept) {
			Ok(vec) => {
				if vec.len() != 20 {
					return Err(WebSocketError::ProtocolError(
						"Sec-WebSocket-Accept must be 20 bytes",
					));
				}
				let mut array = [0u8; 20];
				let mut iter = vec.into_iter();
				for i in &mut array {
					*i = iter.next().unwrap();
				}
				Ok(WebSocketAccept(array))
			}
			Err(_) => Err(WebSocketError::ProtocolError(
				"Invalid Sec-WebSocket-Accept",
			)),
		}
	}
}

impl WebSocketAccept {
	/// Create a new WebSocketAccept from the given WebSocketKey
	pub fn new<K: Into<WebSocketKey>>(key: K) -> WebSocketAccept {
		let key: WebSocketKey = key.into();
		let serialized = format!("{}", key);
		let mut concat_key = String::with_capacity(serialized.len() + 36);
		concat_key.push_str(&serialized[..]);
		concat_key.push_str(MAGIC_GUID);
		let mut sha1 = Sha1::new();
		sha1.update(concat_key.as_bytes());
		let bytes = sha1.digest().bytes();
		WebSocketAccept(bytes)
	}
}

impl From<WebSocketAccept> for HeaderValue {
	fn from(accept: WebSocketAccept) -> HeaderValue {
		HeaderValue::from_str(&format!("{}", accept)).unwrap()
	}
}

// impl Header for WebSocketAccept {
// 	fn header_name() -> &'static str {
// 		"Sec-WebSocket-Accept"
// 	}

// 	fn parse_header(raw: &hyper::header::Raw) -> hyper::Result<WebSocketAccept> {
// 		from_one_raw_str(raw)
// 	}

// 	fn fmt_header(&self, fmt: &mut hyper::header::Formatter) -> fmt::Result {
// 		fmt.fmt_line(self)
// 	}
// }

impl fmt::Display for WebSocketAccept {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let WebSocketAccept(accept) = *self;
		write!(f, "{}", base64::encode(&accept))
	}
}

#[cfg(all(feature = "nightly", test))]
mod tests {
	use super::*;
	use test;
	use std::str::FromStr;
	use header::{Headers, WebSocketKey};
	use hyper::header::Header;

	#[test]
	fn test_header_accept() {
		let key = FromStr::from_str("dGhlIHNhbXBsZSBub25jZQ==").unwrap();
		let accept = WebSocketAccept::new(&key);
		let mut headers = Headers::new();
		headers.set(accept);

		assert_eq!(
			&headers.to_string()[..],
			"Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"
		);
	}
	#[bench]
	fn bench_header_accept_new(b: &mut test::Bencher) {
		let key = WebSocketKey::new();
		b.iter(|| {
			let mut accept = WebSocketAccept::new(&key);
			test::black_box(&mut accept);
		});
	}
	#[bench]
	fn bench_header_accept_parse(b: &mut test::Bencher) {
		let value = vec![b"s3pPLMBiTxaQ9kYGzzhZRbK+xOo=".to_vec()];
		b.iter(|| {
			let mut accept: WebSocketAccept = Header::parse_header(&value[..]).unwrap();
			test::black_box(&mut accept);
		});
	}
	#[bench]
	fn bench_header_accept_format(b: &mut test::Bencher) {
		let value = vec![b"s3pPLMBiTxaQ9kYGzzhZRbK+xOo=".to_vec()];
		let val: WebSocketAccept = Header::parse_header(&value[..]).unwrap();
		b.iter(|| {
			format!("{}", val.serialize());
		});
	}
}
