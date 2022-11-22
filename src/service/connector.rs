use crate::service::io::BoxedIo;
use crate::tls::TlsConnector;
use crate::{BoxError, BoxFuture};

use http::Uri;
use std::task::{Context, Poll};
use tower::make::MakeConnection;
use tower_service::Service;

pub(crate) fn connector<C>(inner: C, tls: TlsConnector) -> Connector<C> {
    Connector::new(inner, tls)
}

pub(crate) struct Connector<C> {
    inner: C,
    tls: TlsConnector,
}

impl<C> Connector<C> {
    fn new(inner: C, tls: TlsConnector) -> Self {
        Self { inner, tls }
    }
}

impl<C> Service<Uri> for Connector<C>
where
    C: MakeConnection<Uri>,
    C::Connection: Unpin + Send + 'static,
    C::Future: Send + 'static,
    BoxError: From<C::Error> + Send + 'static,
{
    type Response = BoxedIo;
    type Error = BoxError;
    type Future = BoxFuture<Self::Response, Self::Error>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        MakeConnection::poll_ready(&mut self.inner, cx).map_err(Into::into)
    }

    fn call(&mut self, uri: Uri) -> Self::Future {
        let tls = self.tls.clone();

        let connect = self.inner.make_connection(uri);

        Box::pin(async move {
            let io = connect.await?;

            let conn = tls.connect(io).await?;
            Ok(BoxedIo::new(conn))
        })
    }
}
