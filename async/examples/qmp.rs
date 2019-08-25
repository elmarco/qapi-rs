type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[cfg(feature = "qmp")]
mod main {
    use crate::Result;
    use async_std::{
        prelude::*,
        io::{BufReader, stdin},
        os::unix::net::UnixStream,
        task,
    };
    use std::env;
    use std::path::Path;
    use futures::{select, FutureExt};
    use qapi_async::{qmp, Qmp};

    async fn try_main<P: AsRef<Path>>(path: P) -> Result<()> {
        let stream = UnixStream::connect(path).await?;

        let mut qmp = Qmp::from_stream(&stream);
        let info = qmp.handshake().await.expect("handshake failed");
        println!("QMP info: {:#?}", info);

        let status = qmp.execute(&qmp::query_status {}).await??;
        println!("VCPU status: {:#?}", status);

        let stdin = BufReader::new(stdin());
        let mut lines_from_stdin = futures::StreamExt::fuse(stdin.lines());
        let mut events = futures::StreamExt::fuse(qmp.events());

        loop {

            select!{
                line = lines_from_stdin.next().fuse() => match line {
                    Some(line) => {
                        let line = line?;
                        dbg!(line);
                    }
                    None => break,
                },
               event = events.next().fuse() => match event {
                   Some(event) => {
                       let event = event?;
                       dbg!(event);
                   }
                   None => {dbg!("None");},
               }
            }
//
//            drop(events);
//            let status = qmp.execute(&qmp::query_status {}).await??;
//            println!("VCPU status: {:#?}", status);
//
        }

        dbg!("end");
        Ok(())
    }

    pub fn main() -> Result<()> {
        env_logger::init();
        let args: Vec<String> = env::args().collect();
        task::block_on(try_main(&args[1]))
    }
}

#[cfg(not(feature = "qmp"))]
mod main {
    use crate::Result;

    pub fn main() -> Result<()> {
        panic!("requires feature qmp")
    }
}

fn main() -> Result<()> {
    main::main()
}
