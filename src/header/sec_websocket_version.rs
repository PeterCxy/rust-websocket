use std::fmt;
use std::str::{self, FromStr};

use http::header::HeaderValue;

/// Represents a Sec-WebSocket-Version header
#[derive(PartialEq, Clone)]
pub enum WebSocketVersion {
	/// The version of WebSocket defined in RFC6455
	WebSocket13,
	/// An unknown version of WebSocket
	Unknown(String),
}

impl fmt::Debug for WebSocketVersion {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match *self {
			WebSocketVersion::WebSocket13 => write!(f, "13"),
			WebSocketVersion::Unknown(ref value) => write!(f, "{}", value),
		}
	}
}

impl FromStr for WebSocketVersion {
	type Err = ();
	fn from_str(value: &str) -> Result<Self, Self::Err> {
		let value = try!(str::from_utf8(value.as_bytes()).map_err(|_| ())).trim();

		match &value[..] {
			"13" => Ok(WebSocketVersion::WebSocket13),
			_ => Ok(WebSocketVersion::Unknown(value.to_owned())),
		}
	}
}

impl From<WebSocketVersion> for HeaderValue {
	fn from(version: WebSocketVersion) -> HeaderValue {
		match &version {
			&WebSocketVersion::WebSocket13 => HeaderValue::from_str("13").unwrap(),
			&WebSocketVersion::Unknown(ref version) => HeaderValue::from_str(&version.to_owned()).unwrap(),
		}
	}
}

impl fmt::Display for WebSocketVersion {
	fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
		fmt::Debug::fmt(self, fmt)
	}
}

#[cfg(all(feature = "nightly", test))]
mod tests {
	use super::*;
	use hyper::header::Header;
	use test;
	#[test]
	fn test_websocket_version() {
		use header::Headers;

		let version = WebSocketVersion::WebSocket13;
		let mut headers = Headers::new();
		headers.set(version);

		assert_eq!(&headers.to_string()[..], "Sec-WebSocket-Version: 13\r\n");
	}
	#[bench]
	fn bench_header_version_parse(b: &mut test::Bencher) {
		let value = vec![b"13".to_vec()];
		b.iter(|| {
			       let mut version: WebSocketVersion = Header::parse_header(&value[..]).unwrap();
			       test::black_box(&mut version);
			      });
	}
	#[bench]
	fn bench_header_version_format(b: &mut test::Bencher) {
		let value = vec![b"13".to_vec()];
		let val: WebSocketVersion = Header::parse_header(&value[..]).unwrap();
		b.iter(|| {
			       format!("{}", val);
			      });
	}
}
