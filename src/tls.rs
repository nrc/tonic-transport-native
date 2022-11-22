use crate::server::Connected;
use crate::service::io::BoxedIo;
use crate::{Error, Result};
use http::Uri;
use std::{fmt, sync::Arc};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_native_tls::TlsStream;

/// Represents a X509 certificate.
#[derive(Debug, Clone)]
pub enum Certificate {
    Pem(Vec<u8>),
    Der(Vec<u8>),
}

impl Certificate {
    /// Parse a PEM encoded X509 Certificate.
    ///
    /// The provided PEM should include at least one PEM encoded certificate.
    pub fn from_pem(pem: impl AsRef<[u8]>) -> Self {
        let pem = pem.as_ref().into();
        Self::Pem(pem)
    }

    pub fn from_der(der: impl AsRef<[u8]>) -> Self {
        let der = der.as_ref().into();
        Self::Der(der)
    }
}

#[derive(Clone)]
pub(crate) struct TlsConnector {
    connector: Arc<tokio_native_tls::TlsConnector>,
    domain: Arc<String>,
}

impl TlsConnector {
    pub(crate) fn new(connector: Arc<tokio_native_tls::TlsConnector>, domain: String) -> Self {
        TlsConnector {
            connector,
            domain: Arc::new(domain),
        }
    }

    pub(crate) async fn connect<I>(&self, io: I) -> Result<BoxedIo>
    where
        I: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        let tls_io = {
            let io = self.connector.connect(&self.domain, io).await?;

            match io.get_ref().negotiated_alpn()? {
                Some(b) if b == b"h2" => (),
                _ => return Err(Error::H2NotNegotiated),
            };

            BoxedIo::new(io)
        };

        Ok(tls_io)
    }
}

impl fmt::Debug for TlsConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsConnector").finish()
    }
}

/// Configures TLS settings for endpoints.
#[derive(Clone)]
pub struct ClientTlsConfig {
    domain: Option<String>,
    connector: Arc<tokio_native_tls::TlsConnector>,
}

impl fmt::Debug for ClientTlsConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientTlsConfig")
            .field("domain", &self.domain)
            .field("connector", &self.connector)
            .finish()
    }
}

impl ClientTlsConfig {
    /// Creates a new `ClientTlsConfig` using NativeTls.
    pub fn new(connector: tokio_native_tls::TlsConnector) -> Self {
        ClientTlsConfig {
            domain: None,
            connector: Arc::new(connector),
        }
    }

    /// Sets the domain name against which to verify the server's TLS certificate.
    pub fn domain_name(self, domain_name: impl Into<String>) -> Self {
        ClientTlsConfig {
            domain: Some(domain_name.into()),
            ..self
        }
    }

    pub(crate) fn tls_connector(&self, uri: Uri) -> Result<TlsConnector> {
        let domain = match &self.domain {
            None => uri
                .host()
                .ok_or_else(|| Error::new_invalid_uri(uri.to_string()))?
                .to_string(),
            Some(domain) => domain.clone(),
        };
        Ok(TlsConnector::new(self.connector.clone(), domain))
    }
}

#[derive(Clone)]
pub struct TlsAcceptor {
    inner: Arc<tokio_native_tls::TlsAcceptor>,
}

impl TlsAcceptor {
    pub fn new(acceptor: Arc<tokio_native_tls::TlsAcceptor>) -> Self {
        Self { inner: acceptor }
    }

    pub(crate) async fn accept<IO>(&self, io: IO) -> Result<TlsStream<IO>>
    where
        IO: AsyncRead + AsyncWrite + Connected + Unpin + Send + 'static,
    {
        let acceptor = self.inner.clone();
        acceptor.accept(io).await.map_err(Into::into)
    }
}

impl fmt::Debug for TlsAcceptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsAcceptor").finish()
    }
}
