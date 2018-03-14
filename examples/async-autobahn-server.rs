extern crate websocket;
extern crate futures;
extern crate tokio;

use websocket::message::OwnedMessage;
use websocket::server::InvalidConnection;
use websocket::async::Server;

use tokio::prelude::*;
use tokio::executor::current_thread;
use tokio::reactor::Handle;
use futures::{Future, Sink, Stream};
use futures::future::{loop_fn, Loop};

fn main() {
	// bind to the server
	let server = Server::bind("127.0.0.1:9002", &Handle::current()).unwrap();

	// time to build the server's future
	// this will be a struct containing everything the server is going to do

	// a stream of incoming connections
	let f = server.incoming()
        // we don't wanna save the stream if it drops
        .map_err(|InvalidConnection { error, .. }| error)
        .for_each(|(upgrade, addr)| {
            // accept the request to be a ws connection
            println!("Got a connection from: {}", addr);
            let f = upgrade
                .accept()
                .and_then(|(s, _)| {
                    // simple echo server impl
                    let (sink, stream) = s.split();
                    stream
                    .take_while(|m| Ok(!m.is_close()))
                    .filter_map(|m| {
                        match m {
                            OwnedMessage::Ping(p) => Some(OwnedMessage::Pong(p)),
                            OwnedMessage::Pong(_) => None,
                            _ => Some(m),
                        }
                    })
                    .forward(sink)
                    .and_then(|(_, sink)| {
                        sink.send(OwnedMessage::Close(None))
                    })
                });

	          current_thread::spawn(f.map_err(move |e| println!("{}: '{:?}'", addr, e))
	                       .map(move |_| println!("{} closed.", addr)));
            Ok(())
        });

    current_thread::spawn(f.map_err(|_| ()));

    tokio::run(loop_fn((), |acc| {
        Ok(Loop::Continue(acc))
    }));
}
