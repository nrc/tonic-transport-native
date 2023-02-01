//! Client implementation and builder.

mod endpoint;

pub use self::endpoint::Endpoint;

use crate::service::{Connection, DynamicServiceStream};
use crate::{BoxBody, BoxError, Error, Result};
use bytes::Bytes;
use http::{uri::Uri, Request, Response};
use hyper::client::connect::Connection as HyperConnection;
use std::{
    fmt,
    future::Future,
    hash::Hash,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc::{channel, Sender},
};
use tokio_native_tls::TlsConnector;

use tower::balance::p2c::Balance;
use tower::{
    buffer::{self, Buffer},
    discover::{Change, Discover},
    util::{BoxService, Either},
    Service,
};

type Svc = Either<Connection, BoxService<Request<BoxBody>, Response<hyper::Body>, BoxError>>;

const DEFAULT_BUFFER_SIZE: usize = 1024;

/// A default batteries included `transport` channel.
///
/// This provides a fully featured http2 gRPC client based on [`hyper::Client`]
/// and `tower` services.
///
/// # Multiplexing requests
///
/// Sending a request on a channel requires a `&mut self` and thus can only send
/// one request in flight. This is intentional and is required to follow the `Service`
/// contract from the `tower` library which this channel implementation is built on
/// top of.
///
/// `tower` itself has a concept of `poll_ready` which is the main mechanism to apply
/// back pressure. `poll_ready` takes a `&mut self` and when it returns `Poll::Ready`
/// we know the `Service` is able to accept only one request before we must `poll_ready`
/// again. Due to this fact any `async fn` that wants to poll for readiness and submit
/// the request must have a `&mut self` reference.
///
/// To work around this and to ease the use of the channel, `Channel` provides a
/// `Clone` implementation that is _cheap_. This is because at the very top level
/// the channel is backed by a `tower_buffer::Buffer` which runs the connection
/// in a background task and provides a `mpsc` channel interface. Due to this
/// cloning the `Channel` type is cheap and encouraged.
#[derive(Clone)]
pub struct Channel {
    svc: Buffer<Svc, Request<BoxBody>>,
}

/// A future that resolves to an HTTP response.
///
/// This is returned by the `Service::call` on [`Channel`].
pub struct ResponseFuture {
    inner: buffer::future::ResponseFuture<<Svc as Service<Request<BoxBody>>>::Future>,
}

pub trait IntoUri {
    fn into_uri(self) -> Result<Uri>;
}

impl IntoUri for Uri {
    fn into_uri(self) -> Result<Uri> {
        Ok(self)
    }
}

impl IntoUri for &'static str {
    fn into_uri(self) -> Result<Uri> {
        Ok(Uri::from_static(self))
    }
}

impl IntoUri for String {
    fn into_uri(self) -> Result<Uri> {
        let bytes: Bytes = self.into_bytes().into();
        bytes.into_uri()
    }
}

impl IntoUri for Bytes {
    fn into_uri(self) -> Result<Uri> {
        Uri::from_maybe_shared(self).map_err(|e| Error::new_invalid_uri(e.to_string()))
    }
}

impl Channel {
    /// Create an [`Endpoint`] builder that can create [`Channel`]s.
    pub fn builder(uri: impl IntoUri, tls: TlsConnector) -> Result<Endpoint> {
        Endpoint::new(uri, tls)
    }

    /// Balance a list of [`Endpoint`]'s.
    ///
    /// This creates a [`Channel`] that will load balance across all the
    /// provided endpoints.
    pub fn balance_list(list: impl Iterator<Item = Endpoint>) -> Self {
        let (channel, tx) = Self::balance_channel(DEFAULT_BUFFER_SIZE);
        list.for_each(|endpoint| {
            tx.try_send(Change::Insert(endpoint.uri.clone(), endpoint))
                .unwrap();
        });

        channel
    }

    /// Balance a list of [`Endpoint`]'s.
    ///
    /// This creates a [`Channel`] that will listen to a stream of change events and will add or remove provided endpoints.
    pub fn balance_channel<K>(capacity: usize) -> (Self, Sender<Change<K, Endpoint>>)
    where
        K: Hash + Eq + Send + Clone + 'static,
    {
        let (tx, rx) = channel(capacity);
        let list = DynamicServiceStream::new(rx);
        (Self::balance(list, DEFAULT_BUFFER_SIZE), tx)
    }

    pub(crate) fn new<C>(connector: C, endpoint: Endpoint) -> Self
    where
        C: Service<Uri> + Send + 'static,
        C::Error: Into<BoxError> + Send,
        C::Future: Unpin + Send,
        C::Response: AsyncRead + AsyncWrite + HyperConnection + Unpin + Send + 'static,
    {
        let buffer_size = endpoint.buffer_size.unwrap_or(DEFAULT_BUFFER_SIZE);

        let svc = Connection::lazy(connector, endpoint);
        let (svc, worker) = Buffer::pair(Either::A(svc), buffer_size);
        tokio::spawn(Box::pin(worker));

        Channel { svc }
    }

    pub(crate) async fn connect<C>(connector: C, endpoint: Endpoint) -> Result<Self>
    where
        C: Service<Uri> + Send + 'static,
        C::Error: Into<BoxError> + Send,
        C::Future: Unpin + Send,
        C::Response: AsyncRead + AsyncWrite + HyperConnection + Unpin + Send + 'static,
    {
        let buffer_size = endpoint.buffer_size.unwrap_or(DEFAULT_BUFFER_SIZE);

        let svc = Connection::connect(connector, endpoint)
            .await
            .map_err(super::Error::from_source)?;
        let (svc, worker) = Buffer::pair(Either::A(svc), buffer_size);
        tokio::spawn(Box::pin(worker));

        Ok(Channel { svc })
    }

    pub(crate) fn balance<D>(discover: D, buffer_size: usize) -> Self
    where
        D: Discover<Service = Connection> + Unpin + Send + 'static,
        D::Error: Into<BoxError>,
        D::Key: Hash + Send + Clone,
    {
        let svc = Balance::new(discover);

        let svc = BoxService::new(svc);
        let (svc, worker) = Buffer::pair(Either::B(svc), buffer_size);
        tokio::spawn(Box::pin(worker));

        Channel { svc }
    }
}

impl Service<http::Request<BoxBody>> for Channel {
    type Response = http::Response<crate::Body>;
    type Error = Error;
    type Future = ResponseFuture;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<()>> {
        Service::poll_ready(&mut self.svc, cx).map_err(Error::from_source)
    }

    fn call(&mut self, request: http::Request<BoxBody>) -> Self::Future {
        let inner = Service::call(&mut self.svc, request);

        ResponseFuture { inner }
    }
}

impl Future for ResponseFuture {
    type Output = Result<Response<hyper::Body>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let val =
            futures_util::ready!(Pin::new(&mut self.inner).poll(cx)).map_err(Error::from_source)?;
        Ok(val).into()
    }
}

impl fmt::Debug for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Channel").finish()
    }
}

impl fmt::Debug for ResponseFuture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResponseFuture").finish()
    }
}
