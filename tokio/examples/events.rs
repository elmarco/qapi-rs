extern crate tokio_qapi;
extern crate tokio_uds;
extern crate tokio;
extern crate futures;
extern crate env_logger;

#[cfg(feature = "qmp")]
mod main {
    use std::env::args;
    use tokio_uds::UnixStream;
    use tokio::prelude::*;
    use tokio::run;
    use tokio_qapi;

    pub fn main() {
        ::env_logger::init();

        let socket_addr = args().nth(1).expect("argument: QMP socket path");

        let stream = UnixStream::connect(socket_addr).expect("failed to connect to socket");
        let (stream, events) = tokio_qapi::event_stream(stream);
        run(tokio_qapi::qmp_handshake(stream)
            .and_then(|(caps, _stream)| {
                println!("{:#?}", caps);
                events.for_each(|e| Ok(println!("Got event {:#?}", e)))
            }).map_err(|e| panic!("Failed with {:?}", e))
        );
    }
}

#[cfg(not(feature = "qmp"))]
mod main {
    pub fn main() { panic!("requires feature qmp") }
}

fn main() { main::main() }
