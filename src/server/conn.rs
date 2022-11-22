use hyper::server::conn::AddrStream;
use std::net::SocketAddr;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_native_tls::TlsStream;

use crate::{Certificate, Result};
use std::sync::Arc;

/// Trait that connected IO resources implement and use to produce info about the connection.
///
/// The goal for this trait is to allow users to implement
/// custom IO types that can still provide the same connection
/// metadata.
///
/// # Example
///
/// The `ConnectInfo` returned will be accessible through [request extensions][ext]:
///
/// ```
/// use tonic::{Request, transport::server::Connected};
///
/// // A `Stream` that yields connections
/// struct MyConnector {}
///
/// // Return metadata about the connection as `MyConnectInfo`
/// impl Connected for MyConnector {
///     type ConnectInfo = MyConnectInfo;
///
///     fn connect_info(&self) -> Self::ConnectInfo {
///         MyConnectInfo {}
///     }
/// }
///
/// #[derive(Clone)]
/// struct MyConnectInfo {
///     // Metadata about your connection
/// }
///
/// // The connect info can be accessed through request extensions:
/// # fn foo(request: Request<()>) {
/// let connect_info: &MyConnectInfo = request
///     .extensions()
///     .get::<MyConnectInfo>()
///     .expect("bug in tonic");
/// # }
/// ```
///
/// [ext]: crate::Request::extensions
pub trait Connected {
    /// The connection info type the IO resources generates.
    // all these bounds are necessary to set this as a request extension
    type ConnectInfo: Clone + Send + Sync + 'static;

    /// Create type holding information about the connection.
    fn connect_info(&self) -> Result<Self::ConnectInfo>;
}

/// Connection info for standard TCP streams.
///
/// This type will be accessible through [request extensions][ext] if you're using the default
/// non-TLS connector.
///
/// See [`Connected`] for more details.
///
/// [ext]: crate::Request::extensions
#[derive(Debug, Clone)]
pub struct TcpConnectInfo {
    remote_addr: Option<SocketAddr>,
}

impl TcpConnectInfo {
    /// Return the remote address the IO resource is connected too.
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.remote_addr
    }
}

impl Connected for AddrStream {
    type ConnectInfo = TcpConnectInfo;

    fn connect_info(&self) -> Result<Self::ConnectInfo> {
        Ok(TcpConnectInfo {
            remote_addr: Some(self.remote_addr()),
        })
    }
}

impl Connected for TcpStream {
    type ConnectInfo = TcpConnectInfo;

    fn connect_info(&self) -> Result<Self::ConnectInfo> {
        Ok(TcpConnectInfo {
            remote_addr: self.peer_addr().ok(),
        })
    }
}

impl Connected for tokio::io::DuplexStream {
    type ConnectInfo = ();

    fn connect_info(&self) -> Result<Self::ConnectInfo> {
        Ok(())
    }
}

impl<T> Connected for TlsStream<T>
where
    T: Connected + AsyncRead + AsyncWrite + Unpin,
{
    type ConnectInfo = TlsConnectInfo<T::ConnectInfo>;

    fn connect_info(&self) -> Result<Self::ConnectInfo> {
        let inner = self.get_ref().get_ref().get_ref().connect_info()?;

        let cert = if let Ok(Some(cert)) = self.get_ref().peer_certificate() {
            Some(Arc::new(Certificate::from_der(cert.to_der()?)))
        } else {
            None
        };

        Ok(TlsConnectInfo { inner, cert })
    }
}

/// Connection info for TLS streams.
///
/// This type will be accessible through [request extensions][ext] if you're using a TLS connector.
///
/// See [`Connected`] for more details.
///
/// [ext]: crate::Request::extensions
#[derive(Debug, Clone)]
pub struct TlsConnectInfo<T> {
    inner: T,
    cert: Option<Arc<Certificate>>,
}

impl<T> TlsConnectInfo<T> {
    /// Get a reference to the underlying connection info.
    pub fn get_ref(&self) -> &T {
        &self.inner
    }

    /// Get a mutable reference to the underlying connection info.
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// Return the peer's leaf TLS certificate.
    pub fn peer_cert(&self) -> Option<Arc<Certificate>> {
        self.cert.clone()
    }
}
