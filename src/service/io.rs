use crate::server::Connected;
use crate::Result;

use hyper::client::connect::{Connected as HyperConnected, Connection};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub(crate) trait Io: AsyncRead + AsyncWrite + Send + 'static {}

impl<T> Io for T where T: AsyncRead + AsyncWrite + Send + 'static {}

pub(crate) struct BoxedIo(Pin<Box<dyn Io>>);

impl BoxedIo {
    pub(crate) fn new<I: Io>(io: I) -> Self {
        BoxedIo(Box::pin(io))
    }
}

impl Connection for BoxedIo {
    fn connected(&self) -> HyperConnected {
        HyperConnected::new()
    }
}

impl Connected for BoxedIo {
    type ConnectInfo = NoneConnectInfo;

    fn connect_info(&self) -> Result<Self::ConnectInfo> {
        Ok(NoneConnectInfo)
    }
}

#[derive(Copy, Clone)]
pub(crate) struct NoneConnectInfo;

impl AsyncRead for BoxedIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for BoxedIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}
