use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

#[derive(PartialEq, Clone, Debug)]
pub struct Host {
	hostname: Cow<'static, str>,
	port: Option<u16>,
}

impl Host {
	pub fn new<H, P>(hostname: H, port: P) -> Host
	where
		H: Into<Cow<'static, str>>,
		P: Into<Option<u16>>,
	{
		Host {
			hostname: hostname.into(),
			port: port.into(),
		}
	}

	pub fn hostname(&self) -> &str {
		self.hostname.as_ref()
	}

	pub fn port(&self) -> Option<u16> {
		self.port
	}
}

impl fmt::Display for Host {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match self.port {
			None | Some(80) | Some(443) => f.write_str(&self.hostname[..]),
			Some(port) => write!(f, "{}:{}", self.hostname, port),
		}
	}
}

impl FromStr for Host {
	type Err = ();
	fn from_str(s: &str) -> Result<Host, Self::Err> {
		let idx = s.rfind(':');
		let port = idx.and_then(|idx| s[idx + 1..].parse().ok());
		let hostname = match port {
			None => s,
			Some(_) => &s[..idx.unwrap()],
		};

		Ok(Host {
			hostname: hostname.to_owned().into(),
			port: port,
		})
	}
}
