use crate::{BoxError, OptionPin, OptionPinProj};

use futures_util::ready;
use http::Response;
use pin_project::pin_project;
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tonic::{Code, Status};
use tower::Service;

/// Middleware that attempts to recover from service errors by turning them into a response built
/// from the `Status`.
#[derive(Debug, Clone)]
pub(crate) struct RecoverError<S> {
    inner: S,
}

impl<S> RecoverError<S> {
    pub(crate) fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S, R, ResBody> Service<R> for RecoverError<S>
where
    S: Service<R, Response = Response<ResBody>>,
    S::Error: Into<BoxError>,
{
    type Response = Response<MaybeEmptyBody<ResBody>>;
    type Error = BoxError;
    type Future = ResponseFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, req: R) -> Self::Future {
        ResponseFuture {
            inner: self.inner.call(req),
        }
    }
}

#[pin_project]
pub(crate) struct ResponseFuture<F> {
    #[pin]
    inner: F,
}

impl<F, E, ResBody> Future for ResponseFuture<F>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    E: Into<BoxError>,
{
    type Output = Result<Response<MaybeEmptyBody<ResBody>>, BoxError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let result: Result<Response<_>, BoxError> =
            ready!(self.project().inner.poll(cx)).map_err(Into::into);

        match result {
            Ok(response) => {
                let response = response.map(MaybeEmptyBody::full);
                Poll::Ready(Ok(response))
            }
            Err(err) => match try_status_from_error(err) {
                Ok(status) => {
                    let mut res = Response::new(MaybeEmptyBody::empty());
                    status.add_header(res.headers_mut()).unwrap();
                    Poll::Ready(Ok(res))
                }
                Err(err) => Poll::Ready(Err(err)),
            },
        }
    }
}

fn try_status_from_error(err: BoxError) -> Result<Status, BoxError> {
    // Adapted from Status::try_from_error.

    let err = match err.downcast::<h2::Error>() {
        Ok(h2) => {
            // See https://github.com/grpc/grpc/blob/3977c30/doc/PROTOCOL-HTTP2.md#errors
            let code = match h2.reason() {
                Some(h2::Reason::NO_ERROR)
                | Some(h2::Reason::PROTOCOL_ERROR)
                | Some(h2::Reason::INTERNAL_ERROR)
                | Some(h2::Reason::FLOW_CONTROL_ERROR)
                | Some(h2::Reason::SETTINGS_TIMEOUT)
                | Some(h2::Reason::COMPRESSION_ERROR)
                | Some(h2::Reason::CONNECT_ERROR) => Code::Internal,
                Some(h2::Reason::REFUSED_STREAM) => Code::Unavailable,
                Some(h2::Reason::CANCEL) => Code::Cancelled,
                Some(h2::Reason::ENHANCE_YOUR_CALM) => Code::ResourceExhausted,
                Some(h2::Reason::INADEQUATE_SECURITY) => Code::PermissionDenied,

                _ => Code::Unknown,
            };

            let mut status = Status::new(code, format!("h2 protocol error: {}", h2));
            status.set_source(Arc::new(*h2));

            return Ok(status);
        }
        Err(err) => err,
    };

    Status::try_from_error(err)
}

#[pin_project]
pub(crate) struct MaybeEmptyBody<B> {
    #[pin]
    inner: OptionPin<B>,
}

impl<B> MaybeEmptyBody<B> {
    fn full(inner: B) -> Self {
        Self {
            inner: OptionPin::Some(inner),
        }
    }

    fn empty() -> Self {
        Self {
            inner: OptionPin::None,
        }
    }
}

impl<B> http_body::Body for MaybeEmptyBody<B>
where
    B: http_body::Body + Send,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        match self.project().inner.project() {
            OptionPinProj::Some(b) => b.poll_data(cx),
            OptionPinProj::None => Poll::Ready(None),
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        match self.project().inner.project() {
            OptionPinProj::Some(b) => b.poll_trailers(cx),
            OptionPinProj::None => Poll::Ready(Ok(None)),
        }
    }

    fn is_end_stream(&self) -> bool {
        match &self.inner {
            OptionPin::Some(b) => b.is_end_stream(),
            OptionPin::None => true,
        }
    }
}
