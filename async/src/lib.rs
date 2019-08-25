#[cfg(feature = "qapi-qmp")]
pub use qapi_qmp as qmp;

#[cfg(feature = "qapi-qga")]
pub use qapi_qga as qga;

#[cfg(feature = "qmp")]
pub use self::qmp_impl::*;

#[cfg(feature = "qga")]
pub use self::qga_impl::*;

pub use self::stream::Stream;

mod stream {
    use core::pin::Pin;
    use futures::io::{AsyncBufRead, AsyncRead, AsyncWrite, Result};
    use futures::task::{Context, Poll};

    pub struct Stream<R, W> {
        r: R,
        w: W,
    }

    impl<R, W> Stream<R, W> {
        pub fn new(r: R, w: W) -> Self {
            Stream { r, w }
        }

        pub fn into_inner(self) -> (R, W) {
            (self.r, self.w)
        }

        pub fn get_ref_read(&self) -> &R {
            &self.r
        }
        pub fn get_mut_read(&mut self) -> &mut R {
            &mut self.r
        }
        pub fn get_ref_write(&self) -> &W {
            &self.w
        }
        pub fn get_mut_write(&mut self) -> &mut W {
            &mut self.w
        }
    }

    impl<R: AsyncRead + Unpin, W: Unpin> AsyncRead for Stream<R, W> {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<Result<usize>> {
            Pin::new(&mut self.get_mut().r).poll_read(cx, buf)
        }
    }

    impl<R: AsyncBufRead + Unpin, W: Unpin> AsyncBufRead for Stream<R, W> {
        fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<&[u8]>> {
            Pin::new(&mut self.get_mut().r).poll_fill_buf(cx)
        }

        fn consume(self: Pin<&mut Self>, amt: usize) {
            Pin::new(&mut self.get_mut().r).consume(amt)
        }
    }

    impl<R: Unpin, W: AsyncWrite + Unpin> AsyncWrite for Stream<R, W> {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<Result<usize>> {
            Pin::new(&mut self.get_mut().w).poll_write(cx, buf)
        }
        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
            Pin::new(&mut self.get_mut().w).poll_flush(cx)
        }
        fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
            Pin::new(&mut self.get_mut().w).poll_close(cx)
        }
    }
}

#[cfg(any(feature = "qmp", feature = "qga"))]
mod qapi {
    use futures::io::AsyncBufReadExt;
    use futures::prelude::*;
    use log::trace;
    use qapi_spec::{self, Command};
    use serde::Deserialize;
    use serde_json;
    use std::io;

    pub struct Qapi<S> {
        pub stream: S,
        pub buffer: String,
    }

    impl<S> Qapi<S> {
        pub fn new(s: S) -> Self {
            Qapi {
                stream: s,
                buffer: String::new(),
            }
        }
    }

    impl<S: AsyncBufRead + Unpin> Qapi<S> {
        pub async fn decode_line<'de, D: Deserialize<'de>>(&'de mut self) -> io::Result<Option<D>> {
            self.buffer.clear();
            let line = self.stream.read_line(&mut self.buffer).await?;
            trace!("<- {} {}", line, self.buffer);

            if line == 0 {
                Ok(None)
            } else {
                serde_json::from_str(&self.buffer)
                    .map(Some)
                    .map_err(From::from)
            }
        }
    }

    impl<S: AsyncWrite + Unpin> Qapi<S> {
        pub async fn write_command<C: Command>(&mut self, command: &C) -> io::Result<()> {
            {
                // TODO: add async serializer?
                let ser = serde_json::to_vec(&qapi_spec::CommandSerializerRef(command)).unwrap();
                self.stream.write(&ser).await?;

                trace!(
                    "-> execute {}: {}",
                    C::NAME,
                    serde_json::to_string_pretty(command).unwrap()
                );
            }

            self.stream.write(&[b'\n']).await?;

            self.stream.flush().await
        }
    }
}

#[cfg(feature = "qmp")]
mod qmp_impl {
    use crate::{qapi::Qapi, Stream};
    use futures::io::{AsyncBufRead, AsyncRead, AsyncWrite, BufReader};
    use qapi_qmp::{qmp_capabilities, Event, QapiCapabilities, QmpMessage, QMP};
    use qapi_spec::{Command, Error};
    use std::io;
    use std::vec::Drain;

    pub struct Qmp<S> {
        inner: Qapi<S>,
        event_queue: Vec<Event>,
    }

    impl<S: AsyncRead + AsyncWrite + Clone> Qmp<Stream<BufReader<S>, S>> {
        pub fn from_stream(s: S) -> Self {
            Self::new(Stream::new(BufReader::new(s.clone()), s))
        }
    }

    impl<S> Qmp<S> {
        pub fn new(stream: S) -> Self {
            Qmp {
                inner: Qapi::new(stream),
                event_queue: Default::default(),
            }
        }
    }

    impl<S: AsyncBufRead + Unpin> Qmp<S> {
        pub async fn read_capabilities(&mut self) -> io::Result<QMP> {
            self.inner
                .decode_line()
                .await
                .map(|v: Option<QapiCapabilities>| v.expect("unexpected eof").QMP)
        }

        pub async fn read_response<C: Command>(&mut self) -> io::Result<Result<C::Ok, Error>> {
            loop {
                match self.inner.decode_line().await? {
                    None => {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "expected command response",
                        ))
                    }
                    Some(QmpMessage::Greeting(..)) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "unexpected greeting",
                        ))
                    }
                    Some(QmpMessage::Response(res)) => return Ok(res.result()),
                    Some(QmpMessage::Event(e)) => self.event_queue.push(e),
                }
            }
        }
    }

    impl<S: AsyncBufRead + AsyncWrite + Unpin> Qmp<S> {
        pub async fn write_command<C: Command>(&mut self, command: &C) -> io::Result<()> {
            self.inner.write_command(command).await
        }

        pub async fn execute<C: Command>(
            &mut self,
            command: &C,
        ) -> io::Result<Result<C::Ok, Error>> {
            self.write_command(command).await?;
            self.read_response::<C>().await
        }

        pub async fn handshake(&mut self) -> io::Result<QMP> {
            let caps = self.read_capabilities().await?;
            self.execute(&qmp_capabilities { enable: None })
                .await
                .and_then(|v| v.map_err(From::from))
                .map(|_| caps)
        }

        pub async fn wait_event(&mut self) -> io::Result<()> {
            match self.inner.decode_line().await? {
                Some(e) => self.event_queue.push(e),
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "unexpected error while waiting for event",
                    ))
                }
            }

            Ok(())
        }

        pub async fn events(&mut self) -> io::Result<Drain<'_, Event>> {
            if self.event_queue.is_empty() {
                self.wait_event().await?;
            }
            Ok(self.event_queue.drain(..))
        }
    }
}
