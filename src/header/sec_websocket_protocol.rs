//use hyper::header::parsing::{from_comma_delimited, fmt_comma_delimited};
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

use http::header::HeaderValue;

// TODO: only allow valid protocol names to be added

/// Represents a Sec-WebSocket-Protocol header
#[derive(PartialEq, Clone, Debug)]
pub struct WebSocketProtocol(pub Vec<String>);

impl Deref for WebSocketProtocol {
	type Target = Vec<String>;
	fn deref(&self) -> &Vec<String> {
		&self.0
	}
}

impl FromStr for WebSocketProtocol {
	type Err = ();
	fn from_str(s: &str) -> Result<Self, Self::Err> {
		Ok(WebSocketProtocol(s.split(',')
			.map(|s| s.trim().to_owned())
			.collect()))
	}
}

impl From<WebSocketProtocol> for HeaderValue {
	fn from(protocol: WebSocketProtocol) -> Self {
		HeaderValue::from_str(&protocol.0.join(", ")).unwrap()
	}
}

impl fmt::Display for WebSocketProtocol {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		Ok(())
	}
}

#[cfg(all(feature = "nightly", test))]
mod tests {
	use super::*;
	use hyper::header::Header;
	use test;
	#[test]
	fn test_header_protocol() {
		use header::Headers;

		let protocol = WebSocketProtocol(vec!["foo".to_string(), "bar".to_string()]);
		let mut headers = Headers::new();
		headers.set(protocol);

		assert_eq!(&headers.to_string()[..], "Sec-WebSocket-Protocol: foo, bar\r\n");
	}
	#[bench]
	fn bench_header_protocol_parse(b: &mut test::Bencher) {
		let value = vec![b"foo, bar".to_vec()];
		b.iter(|| {
			let mut protocol: WebSocketProtocol = Header::parse_header(&value[..]).unwrap();
			test::black_box(&mut protocol);
		});
	}
	#[bench]
	fn bench_header_protocol_format(b: &mut test::Bencher) {
		let value = vec![b"foo, bar".to_vec()];
		let val: WebSocketProtocol = Header::parse_header(&value[..]).unwrap();
		b.iter(|| {
			format!("{}", val);
		});
	}
}
