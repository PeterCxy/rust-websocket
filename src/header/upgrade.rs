use std::fmt::{self, Display};
use std::str::FromStr;

use http::header::HeaderValue;
use unicase;

pub struct Upgrade(pub Vec<Protocol>);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolName {
	HTTP,
	TLS,
	WebSocket,
	H2C,
	Unregistered(String),
}

impl FromStr for ProtocolName {
	type Err = ();
	fn from_str(s: &str) -> Result<ProtocolName, ()> {
		Ok(match s {
			"HTTP" => ProtocolName::HTTP,
			"TLS" => ProtocolName::TLS,
			"h2c" => ProtocolName::H2C,
			_ => {
				if unicase::eq_ascii(s, "websocket") {
					ProtocolName::WebSocket
				} else {
					ProtocolName::Unregistered(s.to_owned())
				}
			}
		})
	}
}

impl Display for ProtocolName {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.write_str(match *self {
			ProtocolName::HTTP => "HTTP",
			ProtocolName::TLS => "TLS",
			ProtocolName::WebSocket => "websocket",
			ProtocolName::H2C => "h2c",
			ProtocolName::Unregistered(ref s) => s,
		})
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Protocol {
	pub name: ProtocolName,
	pub version: Option<String>,
}

impl Protocol {
	pub fn new(name: ProtocolName, version: Option<String>) -> Protocol {
		Protocol { name: name, version: version }
	}
}

impl FromStr for Protocol {
	type Err = ();
	fn from_str(s: &str) -> Result<Protocol, ()> {
		let mut parts = s.splitn(2, '/');
		Ok(Protocol::new(try!(parts.next().unwrap().parse()), parts.next().map(|x| x.to_owned())))
	}
}

impl Display for Protocol {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		try!(fmt::Display::fmt(&self.name, f));
		if let Some(ref version) = self.version {
			try!(write!(f, "/{}", version));
		}
		Ok(())
	}
}

impl FromStr for Upgrade {
	type Err = ();
	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let protocols = s.split(',')
			.filter_map(|x| match x.trim() {
				"" => None,
				y => Some(y),
			})
			.filter_map(|x| x.trim().parse().ok())
			.collect();
		Ok(Upgrade(protocols))
	}
}

impl From<Upgrade> for HeaderValue {
	fn from(upgrade: Upgrade) -> Self {
		HeaderValue::from_str(
			&upgrade.0.iter()
				.map(|p| format!("{}", p))
				.collect::<Vec<String>>()
				.join(", ")
		).unwrap()
	}
}