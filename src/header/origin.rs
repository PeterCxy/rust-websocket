use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use header::Host;

/// Represents an Origin header
#[derive(PartialEq, Clone, Debug)]
pub struct Origin(OriginOrNull);

#[derive(PartialEq, Clone, Debug)]
enum OriginOrNull {
	Origin {
		scheme: Cow<'static, str>,
		host: Host,
	},
	Null,
}

impl Origin {
	pub fn new<S: Into<Cow<'static, str>>, H: Into<Cow<'static, str>>>(
		scheme: S,
		hostname: H,
		port: Option<u16>,
	) -> Origin {
		Origin(OriginOrNull::Origin {
			scheme: scheme.into(),
			host: Host::new(hostname, port),
		})
	}

	pub fn null() -> Origin {
		Origin(OriginOrNull::Null)
	}

	pub fn is_null(&self) -> bool {
		match self {
			&Origin(OriginOrNull::Null) => true,
			_ => false,
		}
	}

	pub fn scheme(&self) -> Option<&str> {
		match self {
			&Origin(OriginOrNull::Origin { ref scheme, .. }) => Some(&scheme),
			_ => None,
		}
	}

	pub fn host(&self) -> Option<&Host> {
		match self {
			&Origin(OriginOrNull::Origin { ref host, .. }) => Some(&host),
			_ => None,
		}
	}
}

static HTTP: &'static str = "http";
static HTTPS: &'static str = "https";

impl FromStr for Origin {
	type Err = ();

	fn from_str(s: &str) -> Result<Origin, Self::Err> {
		let idx = match s.find("://") {
			Some(idx) => idx,
			None => return Err(()),
		};

		let (scheme, etc) = (&s[..idx], &s[idx + 3..]);
		let host = try!(Host::from_str(etc));
		let scheme = match scheme {
			"http" => Cow::Borrowed(HTTP),
			"https" => Cow::Borrowed(HTTPS),
			s => Cow::Owned(s.to_owned()),
		};

		Ok(Origin(OriginOrNull::Origin {
			scheme: scheme,
			host: host,
		}))
	}
}

impl fmt::Display for Origin {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match self.0 {
			OriginOrNull::Origin {
				ref scheme,
				ref host,
			} => write!(f, "{}://{}", scheme, host),
			OriginOrNull::Null => f.write_str("null"),
		}
	}
}

#[cfg(all(feature = "nightly", test))]
mod tests {
	use super::*;
	use hyper::header::Header;
	use test;
	#[test]
	fn test_header_origin() {
		use header::Headers;

		let origin = Origin("foo bar".to_string());
		let mut headers = Headers::new();
		headers.set(origin);

		assert_eq!(&headers.to_string()[..], "Origin: foo bar\r\n");
	}
	#[bench]
	fn bench_header_origin_parse(b: &mut test::Bencher) {
		let value = vec![b"foobar".to_vec()];
		b.iter(|| {
			let mut origin: Origin = Header::parse_header(&value[..]).unwrap();
			test::black_box(&mut origin);
		});
	}
	#[bench]
	fn bench_header_origin_format(b: &mut test::Bencher) {
		let value = vec![b"foobar".to_vec()];
		let val: Origin = Header::parse_header(&value[..]).unwrap();
		b.iter(|| {
			format!("{}", val);
		});
	}
}
