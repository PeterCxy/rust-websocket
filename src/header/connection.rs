use std::fmt::{self, Display};
use std::str::FromStr;

use http::header::HeaderValue;
use unicase::Ascii;

pub use self::ConnectionOption::{KeepAlive, Close, ConnectionHeader};

static KEEP_ALIVE: &'static str = "keep-alive";
static CLOSE: &'static str = "close";

pub enum ConnectionOption {
	KeepAlive,
	Close,
	ConnectionHeader(Ascii<String>),
}

impl FromStr for ConnectionOption {
	type Err = ();
	fn from_str(s: &str) -> Result<ConnectionOption, ()> {
		let s = Ascii::new(s.to_owned());
		if s == KEEP_ALIVE {
			Ok(KeepAlive)
		} else if s == CLOSE {
			Ok(Close)
		} else {
			Ok(ConnectionHeader(s))
		}
	}
}

impl Display for ConnectionOption {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.write_str(match *self {
			KeepAlive => "keep-alive",
			Close => "close",
			ConnectionHeader(ref s) => s.as_ref(),
		})
	}
}

pub struct Connection(pub Vec<ConnectionOption>);

impl FromStr for Connection {
	type Err = ();
	fn from_str(s: &str) -> Result<Connection, ()> {
		let options = s.split(',')
		               .filter_map(|x| match x.trim() {
			"" => None,
			y => Some(y),
		})
		               .filter_map(|x| x.trim().parse().ok())
		               .collect();
		Ok(Connection(options))
	}
}

impl Connection {
	#[inline]
	pub fn close() -> Connection {
		Connection(vec![ConnectionOption::Close])
	}
	#[inline]
	pub fn keep_alive() -> Connection {
		Connection(vec![ConnectionOption::KeepAlive])
	}
}

impl From<Connection> for HeaderValue {
	fn from(connection: Connection) -> Self {
		HeaderValue::from_str(&connection.0
		           .iter()
		           .map(|o| format!("{}", o))
		           .collect::<Vec<String>>()
		           .join(", "))
		.unwrap()
	}
}
