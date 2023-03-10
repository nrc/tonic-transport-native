use crate::{BoxError, Error, Result};

use axum::handler::Handler;
use http::{Request, Response};
use hyper::Body;
use pin_project::pin_project;
use std::{
    convert::Infallible,
    fmt,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tonic::{body::BoxBody, transport::NamedService};
use tower::ServiceExt;
use tower_service::Service;

/// A [`Service`] router.
#[derive(Debug, Default, Clone)]
pub struct Routes {
    router: axum::Router,
}

impl Routes {
    pub(crate) fn new<S>(svc: S) -> Self
    where
        S: Service<Request<Body>, Response = Response<BoxBody>, Error = Infallible>
            + NamedService
            + Clone
            + Send
            + 'static,
        S::Future: Send + 'static,
        S::Error: Into<BoxError> + Send,
    {
        let router = axum::Router::new().fallback(unimplemented.into_service());
        Self { router }.add_service(svc)
    }

    pub(crate) fn add_service<S>(mut self, svc: S) -> Self
    where
        S: Service<Request<Body>, Response = Response<BoxBody>, Error = Infallible>
            + NamedService
            + Clone
            + Send
            + 'static,
        S::Future: Send + 'static,
        S::Error: Into<BoxError> + Send,
    {
        let svc = svc.map_response(|res| res.map(axum::body::boxed));
        self.router = self.router.route(&format!("/{}/*rest", S::NAME), svc);
        self
    }
}

async fn unimplemented() -> impl axum::response::IntoResponse {
    let status = http::StatusCode::OK;
    let headers = [("grpc-status", "12"), ("content-type", "application/grpc")];
    (status, headers)
}

impl Service<Request<Body>> for Routes {
    type Response = Response<BoxBody>;
    type Error = Error;
    type Future = RoutesFuture;

    #[inline]
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        RoutesFuture(self.router.call(req))
    }
}

#[pin_project]
pub struct RoutesFuture(#[pin] axum::routing::future::RouteFuture<Body, Infallible>);

impl fmt::Debug for RoutesFuture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RoutesFuture").finish()
    }
}

impl Future for RoutesFuture {
    type Output = Result<Response<BoxBody>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match futures_util::ready!(self.project().0.poll(cx)) {
            Ok(res) => Ok(res.map(boxed_body)).into(),
            Err(err) => match err {},
        }
    }
}

/// Convert a [`http_body::Body`] into a [`BoxBody`].
fn boxed_body<B>(body: B) -> BoxBody
where
    B: http_body::Body<Data = bytes::Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    use http_body::Body as _;

    body.map_err(|e| {
        let err: BoxError = e.into();
        tonic::Status::from_error(err)
    })
    .boxed_unsync()
}
