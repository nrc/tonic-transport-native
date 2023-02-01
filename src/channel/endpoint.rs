use crate::{service, tls, BoxError, Channel, Error, Result};

use bytes::Bytes;
use http::{uri::Uri, HeaderValue};
use std::{convert::TryInto, fmt, time::Duration};
use tokio_native_tls::TlsConnector;
use tower::make::MakeConnection;

/// Channel builder.
///
/// This struct is used to build and configure HTTP/2 channels.
#[derive(Clone)]
pub struct Endpoint {
    pub(crate) uri: Uri,
    pub(crate) tls: TlsConnector,
    pub(crate) tls_verify_domain: Option<String>,
    pub(crate) origin: Option<Uri>,
    pub(crate) user_agent: Option<HeaderValue>,
    pub(crate) timeout: Option<Duration>,
    pub(crate) concurrency_limit: Option<usize>,
    pub(crate) rate_limit: Option<(u64, Duration)>,
    pub(crate) buffer_size: Option<usize>,
    pub(crate) init_stream_window_size: Option<u32>,
    pub(crate) init_connection_window_size: Option<u32>,
    pub(crate) tcp_keepalive: Option<Duration>,
    pub(crate) tcp_nodelay: bool,
    pub(crate) http2_keep_alive_interval: Option<Duration>,
    pub(crate) http2_keep_alive_timeout: Option<Duration>,
    pub(crate) http2_keep_alive_while_idle: Option<bool>,
    pub(crate) connect_timeout: Option<Duration>,
    pub(crate) http2_adaptive_window: Option<bool>,
}

impl Endpoint {
    // TODO doesn't need to return a Result
    pub fn new(uri: Uri, tls: TlsConnector) -> Result<Self> {
        Ok(Self {
            uri,
            tls,
            tls_verify_domain: None,
            origin: None,
            user_agent: None,
            concurrency_limit: None,
            rate_limit: None,
            timeout: None,
            buffer_size: None,
            init_stream_window_size: None,
            init_connection_window_size: None,
            tcp_keepalive: None,
            tcp_nodelay: true,
            http2_keep_alive_interval: None,
            http2_keep_alive_timeout: None,
            http2_keep_alive_while_idle: None,
            connect_timeout: None,
            http2_adaptive_window: None,
        })
    }

    /// Convert an `Endpoint` from a static string.
    ///
    /// # Panics
    ///
    /// This function panics if the argument is an invalid URI.
    ///
    /// ```
    /// # use tonic::transport::Endpoint;
    /// Endpoint::from_static("https://example.com");
    /// ```
    pub fn from_static(s: &'static str, tls: TlsConnector) -> Result<Self> {
        let uri = Uri::from_static(s);
        Self::new(uri, tls)
    }

    /// Convert an `Endpoint` from shared bytes.
    ///
    /// ```
    /// # use tonic::transport::Endpoint;
    /// Endpoint::from_shared("https://example.com".to_string());
    /// ```
    pub fn from_shared(s: impl Into<Bytes>, tls: TlsConnector) -> Result<Self> {
        let uri =
            Uri::from_maybe_shared(s.into()).map_err(|e| Error::new_invalid_uri(e.to_string()))?;
        Self::new(uri, tls)
    }

    pub fn from_string(s: String, tls: TlsConnector) -> Result<Self> {
        Self::from_shared(s.into_bytes(), tls)
    }

    /// Set a custom user-agent header.
    ///
    /// `user_agent` will be prepended to Tonic's default user-agent string (`tonic/x.x.x`).
    /// It must be a value that can be converted into a valid  `http::HeaderValue` or building
    /// the endpoint will fail.
    /// ```
    /// # use tonic::transport::Endpoint;
    /// # let mut builder = Endpoint::from_static("https://example.com");
    /// builder.user_agent("Greeter").expect("Greeter should be a valid header value");
    /// // user-agent: "Greeter tonic/x.x.x"
    /// ```
    pub fn user_agent<T>(self, user_agent: T) -> Result<Self>
    where
        T: TryInto<HeaderValue>,
    {
        user_agent
            .try_into()
            .map(|ua| Endpoint {
                user_agent: Some(ua),
                ..self
            })
            .map_err(|_| Error::InvalidUserAgent)
    }

    /// Set a custom origin.
    ///
    /// Override the `origin`, mainly useful when you are reaching a Server/LoadBalancer
    /// which serves multiple services at the same time.
    /// It will play the role of SNI (Server Name Indication).
    ///
    /// ```
    /// # use tonic::transport::Endpoint;
    /// # let mut builder = Endpoint::from_static("https://proxy.com");
    /// builder.origin("https://example.com".parse().expect("http://example.com must be a valid URI"));
    /// // origin: "https://example.com"
    /// ```
    pub fn origin(self, origin: Uri) -> Self {
        Endpoint {
            origin: Some(origin),
            ..self
        }
    }

    /// Set a domain for TLS verification.
    ///
    /// The domain name is used to verify the server's TLS certificate. If no domain is specified,
    /// then the domain from the channel's URI is used.
    pub fn tls_verify_domain(self, tls_verify_domain: String) -> Self {
        Endpoint {
            tls_verify_domain: Some(tls_verify_domain),
            ..self
        }
    }

    /// Apply a timeout to each request.
    ///
    /// ```
    /// # use tonic::transport::Endpoint;
    /// # use std::time::Duration;
    /// # let mut builder = Endpoint::from_static("https://example.com");
    /// builder.timeout(Duration::from_secs(5));
    /// ```
    ///
    /// # Notes
    ///
    /// This does **not** set the timeout metadata (`grpc-timeout` header) on
    /// the request, meaning the server will not be informed of this timeout,
    /// for that use [`Request::set_timeout`].
    ///
    /// [`Request::set_timeout`]: crate::Request::set_timeout
    pub fn timeout(self, dur: Duration) -> Self {
        Endpoint {
            timeout: Some(dur),
            ..self
        }
    }

    /// Apply a timeout to connecting to the uri.
    ///
    /// Defaults to no timeout.
    ///
    /// ```
    /// # use tonic::transport::Endpoint;
    /// # use std::time::Duration;
    /// # let mut builder = Endpoint::from_static("https://example.com");
    /// builder.connect_timeout(Duration::from_secs(5));
    /// ```
    pub fn connect_timeout(self, dur: Duration) -> Self {
        Endpoint {
            connect_timeout: Some(dur),
            ..self
        }
    }

    /// Set whether TCP keepalive messages are enabled on accepted connections.
    ///
    /// If `None` is specified, keepalive is disabled, otherwise the duration
    /// specified will be the time to remain idle before sending TCP keepalive
    /// probes.
    ///
    /// Default is no keepalive (`None`)
    ///
    pub fn tcp_keepalive(self, tcp_keepalive: Option<Duration>) -> Self {
        Endpoint {
            tcp_keepalive,
            ..self
        }
    }

    /// Apply a concurrency limit to each request.
    ///
    /// ```
    /// # use tonic::transport::Endpoint;
    /// # let mut builder = Endpoint::from_static("https://example.com");
    /// builder.concurrency_limit(256);
    /// ```
    pub fn concurrency_limit(self, limit: usize) -> Self {
        Endpoint {
            concurrency_limit: Some(limit),
            ..self
        }
    }

    /// Apply a rate limit to each request.
    ///
    /// ```
    /// # use tonic::transport::Endpoint;
    /// # use std::time::Duration;
    /// # let mut builder = Endpoint::from_static("https://example.com");
    /// builder.rate_limit(32, Duration::from_secs(1));
    /// ```
    pub fn rate_limit(self, limit: u64, duration: Duration) -> Self {
        Endpoint {
            rate_limit: Some((limit, duration)),
            ..self
        }
    }

    /// Sets the [`SETTINGS_INITIAL_WINDOW_SIZE`][spec] option for HTTP2
    /// stream-level flow control.
    ///
    /// Default is 65,535
    ///
    /// [spec]: https://http2.github.io/http2-spec/#SETTINGS_INITIAL_WINDOW_SIZE
    pub fn initial_stream_window_size(self, sz: impl Into<Option<u32>>) -> Self {
        Endpoint {
            init_stream_window_size: sz.into(),
            ..self
        }
    }

    /// Sets the max connection-level flow control for HTTP2
    ///
    /// Default is 65,535
    pub fn initial_connection_window_size(self, sz: impl Into<Option<u32>>) -> Self {
        Endpoint {
            init_connection_window_size: sz.into(),
            ..self
        }
    }

    /// Set the value of `TCP_NODELAY` option for accepted connections. Enabled by default.
    pub fn tcp_nodelay(self, enabled: bool) -> Self {
        Endpoint {
            tcp_nodelay: enabled,
            ..self
        }
    }

    /// Set http2 KEEP_ALIVE_INTERVAL. Uses `hyper`'s default otherwise.
    pub fn http2_keep_alive_interval(self, interval: Duration) -> Self {
        Endpoint {
            http2_keep_alive_interval: Some(interval),
            ..self
        }
    }

    /// Set http2 KEEP_ALIVE_TIMEOUT. Uses `hyper`'s default otherwise.
    pub fn keep_alive_timeout(self, duration: Duration) -> Self {
        Endpoint {
            http2_keep_alive_timeout: Some(duration),
            ..self
        }
    }

    /// Set http2 KEEP_ALIVE_WHILE_IDLE. Uses `hyper`'s default otherwise.
    pub fn keep_alive_while_idle(self, enabled: bool) -> Self {
        Endpoint {
            http2_keep_alive_while_idle: Some(enabled),
            ..self
        }
    }

    /// Sets whether to use an adaptive flow control. Uses `hyper`'s default otherwise.
    pub fn http2_adaptive_window(self, enabled: bool) -> Self {
        Endpoint {
            http2_adaptive_window: Some(enabled),
            ..self
        }
    }

    /// Create a channel from this config.
    pub async fn connect(&self) -> Result<Channel> {
        let mut http = hyper::client::connect::HttpConnector::new();
        http.enforce_http(false);
        http.set_nodelay(self.tcp_nodelay);
        http.set_keepalive(self.tcp_keepalive);

        let connector = service::connector(http, self.tls_connector()?);

        if let Some(connect_timeout) = self.connect_timeout {
            let mut connector = hyper_timeout::TimeoutConnector::new(connector);
            connector.set_connect_timeout(Some(connect_timeout));
            Channel::connect(connector, self.clone()).await
        } else {
            Channel::connect(connector, self.clone()).await
        }
    }

    /// Create a channel from this config.
    ///
    /// The channel returned by this method does not attempt to connect to the endpoint until first
    /// use.
    pub fn connect_lazy(&self) -> Result<Channel> {
        let mut http = hyper::client::connect::HttpConnector::new();
        http.enforce_http(false);
        http.set_nodelay(self.tcp_nodelay);
        http.set_keepalive(self.tcp_keepalive);

        let connector = service::connector(http, self.tls_connector()?);

        if let Some(connect_timeout) = self.connect_timeout {
            let mut connector = hyper_timeout::TimeoutConnector::new(connector);
            connector.set_connect_timeout(Some(connect_timeout));
            Ok(Channel::new(connector, self.clone()))
        } else {
            Ok(Channel::new(connector, self.clone()))
        }
    }

    /// Connect with a custom connector.
    ///
    /// This allows you to build a [Channel](struct.Channel.html) that uses a non-HTTP transport.
    /// See the `uds` example for an example on how to use this function to build channel that
    /// uses a Unix socket transport.
    ///
    /// The [`connect_timeout`](Endpoint::connect_timeout) will still be applied.
    pub async fn connect_with_connector<C>(&self, connector: C) -> Result<Channel>
    where
        C: MakeConnection<Uri> + Send + 'static,
        C::Connection: Unpin + Send + 'static,
        C::Future: Send + 'static,
        BoxError: From<C::Error> + Send + 'static,
    {
        let connector = service::connector(connector, self.tls_connector()?);

        if let Some(connect_timeout) = self.connect_timeout {
            let mut connector = hyper_timeout::TimeoutConnector::new(connector);
            connector.set_connect_timeout(Some(connect_timeout));
            Channel::connect(connector, self.clone()).await
        } else {
            Channel::connect(connector, self.clone()).await
        }
    }

    /// Connect with a custom connector lazily.
    ///
    /// This allows you to build a [Channel](struct.Channel.html) that uses a non-HTTP transport
    /// connect to it lazily.
    ///
    /// See the `uds` example for an example on how to use this function to build channel that
    /// uses a Unix socket transport.
    pub fn connect_with_connector_lazy<C>(&self, connector: C) -> Result<Channel>
    where
        C: MakeConnection<Uri> + Send + 'static,
        C::Connection: Unpin + Send + 'static,
        C::Future: Send + 'static,
        BoxError: From<C::Error> + Send + 'static,
    {
        let connector = service::connector(connector, self.tls_connector()?);

        Ok(Channel::new(connector, self.clone()))
    }

    pub(crate) fn tls_connector(&self) -> Result<tls::TlsConnector> {
        let domain = match &self.tls_verify_domain {
            None => self
                .uri
                .host()
                .ok_or_else(|| Error::new_invalid_uri(self.uri.to_string()))?
                .to_string(),
            Some(domain) => domain.clone(),
        };
        Ok(tls::TlsConnector::new(self.tls.clone(), domain))
    }

    /// Get the endpoint uri.
    ///
    /// ```
    /// # use tonic::transport::Endpoint;
    /// # use http::Uri;
    /// let endpoint = Endpoint::from_static("https://example.com");
    ///
    /// assert_eq!(endpoint.uri(), &Uri::from_static("https://example.com"));
    /// ```
    pub fn uri(&self) -> &Uri {
        &self.uri
    }
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Endpoint").finish()
    }
}
