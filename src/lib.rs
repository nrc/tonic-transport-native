#[doc(inline)]
pub use crate::channel::{Channel, Endpoint};
#[doc(inline)]
pub use crate::server::{NamedService, Router, Server};
#[doc(inline)]
pub use crate::service::grpc_timeout::TimeoutExpired;
pub use crate::tls::TlsAcceptor;
pub use hyper::{Body, Uri};

use pin_project::pin_project;
use std::error::Error as StdError;
use tonic::body::BoxBody;

mod channel;
mod server;
mod service;
mod tls;

type BoxFuture<T, E> = std::pin::Pin<
    Box<dyn std::future::Future<Output = std::result::Result<T, E>> + Send + 'static>,
>;

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid URI: {0}")]
    InvalidUri(String),
    #[error("Invalid user agent")]
    InvalidUserAgent,
    #[error("HTTP/2 was not negotiated")]
    H2NotNegotiated,
    #[error("Unknown error {0}")]
    Other(#[from] BoxError),
}

impl Error {
    fn new_invalid_uri(detail: String) -> Error {
        Error::InvalidUri(detail)
    }

    fn from_source(source: BoxError) -> Error {
        Error::Other(source)
    }
}

impl From<axum::Error> for Error {
    fn from(e: axum::Error) -> Error {
        Error::Other(Box::new(e))
    }
}

impl From<native_tls::Error> for Error {
    fn from(e: native_tls::Error) -> Error {
        Error::Other(Box::new(e))
    }
}

type Result<T> = std::result::Result<T, Error>;
pub type BoxError = Box<dyn StdError + Send + Sync>;

/// A pin-project compatible `Option`
#[pin_project(project = OptionPinProj)]
enum OptionPin<T> {
    Some(#[pin] T),
    None,
}
