type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[cfg(feature = "qmp")]
mod main {
    use crate::Result;
    use async_std::{os::unix::net::UnixStream, task};
    use std::env;
    use std::path::Path;

    async fn try_main<P: AsRef<Path>>(path: P) -> Result<()> {
        use qapi_async::{qmp, Qmp};

        let stream = UnixStream::connect(path).await?;

        let mut qmp = Qmp::from_stream(&stream);
        let info = qmp.handshake().await.expect("handshake failed");
        println!("QMP info: {:#?}", info);

        let status = qmp.execute(&qmp::query_status {}).await??;
        println!("VCPU status: {:#?}", status);

        loop {
            for event in qmp.events().await? {
                println!("Got event: {:#?}", event);
            }
        }
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
