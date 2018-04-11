extern crate websocket;
extern crate futures;
extern crate tokio;

use websocket::{ClientBuilder, OwnedMessage};
use websocket::result::WebSocketError;
use futures::sink::Sink;
use futures::stream::Stream;
use futures::Future;
use futures::future::{self, Loop};
use tokio::reactor::Handle;

type BoxFuture<I, E> = Box<Future<Item = I, Error = E> + Send>;

fn main() {
	let addr = "ws://127.0.0.1:9001".to_string();
	let agent = "rust-websocket";

	println!("Using fuzzingserver {}", addr);
	println!("Using agent {}", agent);

	let case_count = get_case_count(addr.clone());
	println!("We will be running {} test cases!", case_count);

	println!("Running test suite...");
	for case_id in 1..(case_count + 1) {
		let url = addr.clone() + "/runCase?case=" + &case_id.to_string()[..] + "&agent=" + agent;

		let test_case = ClientBuilder::new(&url)
			.unwrap()
			.async_connect_insecure(&Handle::default())
			.and_then(move |(duplex, _)| {
				println!("Executing test case: {}/{}", case_id, case_count);
				future::loop_fn(duplex, |stream| {
					stream.into_future()
					      .or_else(|(err, stream)| {
						println!("Could not receive message: {:?}", err);
						stream.send(OwnedMessage::Close(None)).map(|s| (None, s))
					})
					      .and_then(|(msg, stream)| -> BoxFuture<_, _> {
						match msg {
							Some(OwnedMessage::Text(txt)) => {
								Box::new(stream.send(OwnedMessage::Text(txt)).map(
									|s| Loop::Continue(s),
								))
							}
							Some(OwnedMessage::Binary(bin)) => {
								Box::new(stream.send(OwnedMessage::Binary(bin)).map(
									|s| Loop::Continue(s),
								))
							}
							Some(OwnedMessage::Ping(data)) => {
								Box::new(stream.send(OwnedMessage::Pong(data)).map(
									|s| Loop::Continue(s),
								))
							}
							Some(OwnedMessage::Pong(_)) => {
								Box::new(future::ok(Loop::Continue(stream)))
							}
							Some(OwnedMessage::Close(_)) => {
								Box::new(stream.send(OwnedMessage::Close(None)).map(
									|_| Loop::Break(()),
								))
							}
							None => Box::new(future::ok(Loop::Break(()))),
						}
					})
				})
			})
			.map(move |_| {
				println!("Test case {} is finished!", case_id);
			})
			.or_else(move |err| {
				println!("Test case {} ended with an error: {:?}", case_id, err);
				Ok(()) as Result<(), ()>
			});

		tokio::run(test_case.map(|_| ()).map_err(|_| ()));
	}

	update_reports(addr.clone(), agent);
	println!("Test suite finished!");
}

fn get_case_count(addr: String) -> usize {
	let url = addr + "/getCaseCount";
	let err = "Unsupported message in /getCaseCount";

	let counter = ClientBuilder::new(&url)
		.unwrap()
		.async_connect_insecure(&Handle::default())
		.and_then(|(s, _)| s.into_future().map_err(|e| e.0))
		.and_then(move |(msg, _)| match msg {
			Some(OwnedMessage::Text(txt)) => Ok(txt.parse().unwrap()),
			_ => Err(WebSocketError::ProtocolError(err)),
		});
	tokio::run(counter.map(|_: String| ()).map_err(|_| ()));
	0
}

fn update_reports(addr: String, agent: &str) {
	println!("Updating reports...");
	let url = addr + "/updateReports?agent=" + agent;

	let updater = ClientBuilder::new(&url)
		.unwrap()
		.async_connect_insecure(&Handle::default())
		.and_then(|(sink, _)| sink.send(OwnedMessage::Close(None)));
	tokio::run(updater.map(|_| ()).map_err(|_| ()));

	println!("Reports updated.");
}
