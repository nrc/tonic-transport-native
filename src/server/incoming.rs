use crate::server::{Connected, Server};
use crate::BoxError;

use futures_core::Stream;
use futures_util::stream::TryStreamExt;
use hyper::server::{
    accept::Accept,
    conn::{AddrIncoming, AddrStream},
};
use std::{
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
};
use tokio_native_tls::TlsStream;

pub(crate) fn tcp_incoming<IO, IE, L>(
    incoming: impl Stream<Item = Result<IO, IE>>,
    server: Server<L>,
) -> impl Stream<Item = Result<TlsStream<IO>, BoxError>>
where
    IO: AsyncRead + AsyncWrite + Connected + Unpin + Send + 'static,
    IE: Into<BoxError>,
{
    async_stream::try_stream! {
        futures_util::pin_mut!(incoming);

        let mut tasks = futures_util::stream::futures_unordered::FuturesUnordered::new();

        loop {
            match select(&mut incoming, &mut tasks).await {
                SelectOutput::Incoming(stream) => {
                    let tls = server.tls.clone();

                    let accept = tokio::spawn(async move {
                        Ok(tls.accept(stream).await?)
                    });

                    tasks.push(accept);
                }

                SelectOutput::Io(io) => {
                    yield io;
                }

                SelectOutput::Err(e) => {
                    tracing::debug!(message = "Accept loop error.", error = %e);
                }

                SelectOutput::Done => {
                    break;
                }
            }
        }
    }
}

async fn select<IO, IE>(
    incoming: &mut (impl Stream<Item = Result<IO, IE>> + Unpin),
    tasks: &mut futures_util::stream::futures_unordered::FuturesUnordered<
        tokio::task::JoinHandle<Result<TlsStream<IO>, BoxError>>,
    >,
) -> SelectOutput<IO>
where
    IE: Into<BoxError>,
{
    use futures_util::StreamExt;

    if tasks.is_empty() {
        return match incoming.try_next().await {
            Ok(Some(stream)) => SelectOutput::Incoming(stream),
            Ok(None) => SelectOutput::Done,
            Err(e) => SelectOutput::Err(e.into()),
        };
    }

    tokio::select! {
        stream = incoming.try_next() => {
            match stream {
                Ok(Some(stream)) => SelectOutput::Incoming(stream),
                Ok(None) => SelectOutput::Done,
                Err(e) => SelectOutput::Err(e.into()),
            }
        }

        accept = tasks.next() => {
            match accept.expect("FuturesUnordered stream should never end") {
                Ok(Ok(io)) => SelectOutput::Io(io),
                Ok(Err(e)) => SelectOutput::Err(e),
                Err(e) => SelectOutput::Err(e.into()),
            }
        }
    }
}

enum SelectOutput<A> {
    Incoming(A),
    Io(TlsStream<A>),
    Err(BoxError),
    Done,
}

/// Binds a socket address for a [Router](super::Router)
///
/// An incoming stream, usable with [Router::serve_with_incoming](super::Router::serve_with_incoming),
/// of `AsyncRead + AsyncWrite` that communicate with clients that connect to a socket address.
#[derive(Debug)]
pub struct TcpIncoming {
    inner: AddrIncoming,
}

impl TcpIncoming {
    /// Creates an instance by binding (opening) the specified socket address
    /// to which the specified TCP 'nodelay' and 'keepalive' parameters are applied.
    /// Returns a TcpIncoming if the socket address was successfully bound.
    ///
    /// # Examples
    /// ```no_run
    /// # use tower_service::Service;
    /// # use http::{request::Request, response::Response};
    /// # use tonic::{body::BoxBody, transport::{Body, NamedService, Server, server::TcpIncoming}};
    /// # use core::convert::Infallible;
    /// # use std::error::Error;
    /// # fn main() { }  // Cannot have type parameters, hence instead define:
    /// # fn run<S>(some_service: S) -> Result<(), Box<dyn Error + Send + Sync>>
    /// # where
    /// #   S: Service<Request<Body>, Response = Response<BoxBody>, Error = Infallible> + NamedService + Clone + Send + 'static,
    /// #   S::Future: Send + 'static,
    /// # {
    /// // Find a free port
    /// let mut port = 1322;
    /// let tinc = loop {
    ///    let addr = format!("127.0.0.1:{}", port).parse().unwrap();
    ///    match TcpIncoming::new(addr, true, None) {
    ///       Ok(t) => break t,
    ///       Err(_) => port += 1
    ///    }
    /// };
    /// Server::builder()
    ///    .add_service(some_service)
    ///    .serve_with_incoming(tinc);
    /// # Ok(())
    /// # }
    pub fn new(
        addr: SocketAddr,
        nodelay: bool,
        keepalive: Option<Duration>,
    ) -> Result<Self, BoxError> {
        let mut inner = AddrIncoming::bind(&addr)?;
        inner.set_nodelay(nodelay);
        inner.set_keepalive(keepalive);
        Ok(TcpIncoming { inner })
    }

    /// Creates a new `TcpIncoming` from an existing `tokio::net::TcpListener`.
    pub fn from_listener(
        listener: TcpListener,
        nodelay: bool,
        keepalive: Option<Duration>,
    ) -> Result<Self, BoxError> {
        let mut inner = AddrIncoming::from_listener(listener)?;
        inner.set_nodelay(nodelay);
        inner.set_keepalive(keepalive);
        Ok(TcpIncoming { inner })
    }
}

impl Stream for TcpIncoming {
    type Item = Result<AddrStream, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner).poll_accept(cx)
    }
}

#[cfg(test)]
mod tests {
    use crate::transport::server::TcpIncoming;
    #[tokio::test]
    async fn one_tcpincoming_at_a_time() {
        let addr = "127.0.0.1:1322".parse().unwrap();
        {
            let _t1 = TcpIncoming::new(addr, true, None).unwrap();
            let _t2 = TcpIncoming::new(addr, true, None).unwrap_err();
        }
        let _t3 = TcpIncoming::new(addr, true, None).unwrap();
    }
}
