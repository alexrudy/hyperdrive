//! Protocol describes how http requests and responses are transmitted over a connection.
//!
//! There are three protocols provided here: HTTP/1.1, HTTP/2, and an automatically
//! negotiated protocol which can be either HTTP/1.1 or HTTP/2 based on the connection
//! protocol and ALPN negotiation.

use std::future::Future;

use futures_core::future::BoxFuture;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tower::Service;

use super::connection::ConnectionError;
use super::connection::HttpConnection;
use super::transport::TransportStream;
use super::Connection;
use crate::bridge::io::TokioIo;
use crate::info::HasConnectionInfo;

pub mod auto;
pub use hyper::client::conn::http1;
pub use hyper::client::conn::http2;

/// A request to establish a connection using a specific HTTP protocol
/// over a given transport.
#[derive(Debug)]
pub struct ProtocolRequest<IO: HasConnectionInfo> {
    /// The transport to use for the connection
    pub transport: TransportStream<IO>,

    /// The HTTP protocol to use for the connection
    pub version: HttpProtocol,
}

/// Protocols (like HTTP) define how data is sent and received over a connection.
///
/// A protocol is a service which accepts a [`ProtocolRequest`] and returns a connection.
///
/// The request contains a transport stream and the HTTP protocol to use for the connection.
///
/// The connection is responsible for sending and receiving HTTP requests and responses.
///
///
pub trait Protocol<IO>
where
    IO: HasConnectionInfo,
    Self: Service<ProtocolRequest<IO>, Response = Self::Connection>,
{
    /// Error returned when connection fails
    type Error: std::error::Error + Send + Sync + 'static;

    /// The type of connection returned by this service
    type Connection: Connection;

    /// The type of the handshake future
    type Future: Future<Output = Result<Self::Connection, <Self as Protocol<IO>>::Error>>
        + Send
        + 'static;

    /// Connect to a remote server and return a connection.
    ///
    /// The protocol version is provided to facilitate the correct handshake procedure.
    fn connect(
        &mut self,
        transport: TransportStream<IO>,
        version: HttpProtocol,
    ) -> <Self as Protocol<IO>>::Future;

    /// Poll the protocol to see if it is ready to accept a new connection.
    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), <Self as Protocol<IO>>::Error>>;
}

impl<T, C, IO> Protocol<IO> for T
where
    IO: HasConnectionInfo,
    T: Service<ProtocolRequest<IO>, Response = C> + Send + 'static,
    T::Error: std::error::Error + Send + Sync + 'static,
    T::Future: Send + 'static,
    C: Connection,
{
    type Error = T::Error;
    type Connection = C;
    type Future = T::Future;

    fn connect(
        &mut self,
        transport: TransportStream<IO>,
        version: HttpProtocol,
    ) -> <Self as Protocol<IO>>::Future {
        self.call(ProtocolRequest { transport, version })
    }

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), <Self as Protocol<IO>>::Error>> {
        Service::poll_ready(self, cx)
    }
}

/// The HTTP protocol to use for a connection.
///
/// This differs from the HTTP version in that it is constrained to the two flavors of HTTP
/// protocol, HTTP/1.1 and HTTP/2. HTTP/3 is not yet supported. HTTP/0.9 and HTTP/1.0 are
/// supported by HTTP/1.1.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum HttpProtocol {
    /// Connect using HTTP/1.1
    Http1,

    /// Connect using HTTP/2
    Http2,
}

impl HttpProtocol {
    /// Does this protocol allow multiplexing?
    pub fn multiplex(&self) -> bool {
        matches!(self, Self::Http2)
    }

    /// HTTP Version
    ///
    /// Convert the protocol to an HTTP version.
    ///
    /// For HTTP/1.1, this returns `::http::Version::HTTP_11`.
    /// For HTTP/2, this returns `::http::Version::HTTP_2`.
    pub fn version(&self) -> ::http::Version {
        match self {
            Self::Http1 => ::http::Version::HTTP_11,
            Self::Http2 => ::http::Version::HTTP_2,
        }
    }
}

impl From<::http::Version> for HttpProtocol {
    fn from(version: ::http::Version) -> Self {
        match version {
            ::http::Version::HTTP_11 | ::http::Version::HTTP_10 => Self::Http1,
            ::http::Version::HTTP_2 => Self::Http2,
            _ => panic!("Unsupported HTTP protocol"),
        }
    }
}

impl<IO> tower::Service<ProtocolRequest<IO>> for hyper::client::conn::http1::Builder
where
    IO: HasConnectionInfo + AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Response = HttpConnection;

    type Error = ConnectionError;
    type Future = BoxFuture<'static, Result<HttpConnection, ConnectionError>>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: ProtocolRequest<IO>) -> Self::Future {
        let builder = self.clone();
        let stream = req.transport.into_inner();

        Box::pin(async move {
            let (sender, conn) = builder
                .handshake(TokioIo::new(stream))
                .await
                .map_err(|err| ConnectionError::Handshake(err.into()))?;
            tokio::spawn(async {
                if let Err(err) = conn.await {
                    if err.is_user() {
                        tracing::error!(%err, "h1 connection driver error");
                    } else {
                        tracing::debug!(%err, "h1 connection driver error");
                    }
                }
            });
            Ok(HttpConnection::h1(sender))
        })
    }
}

impl<E, IO> tower::Service<ProtocolRequest<IO>> for hyper::client::conn::http2::Builder<E>
where
    E: hyper::rt::bounds::Http2ClientConnExec<crate::body::Body, TokioIo<IO>>
        + Unpin
        + Send
        + Sync
        + Clone
        + 'static,
    IO: HasConnectionInfo + AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Response = HttpConnection;

    type Error = ConnectionError;
    type Future = BoxFuture<'static, Result<HttpConnection, ConnectionError>>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: ProtocolRequest<IO>) -> Self::Future {
        let builder = self.clone();
        let stream = req.transport.into_inner();

        Box::pin(async move {
            let (sender, conn) = builder
                .handshake(TokioIo::new(stream))
                .await
                .map_err(|err| ConnectionError::Handshake(err.into()))?;
            tokio::spawn(async {
                if let Err(err) = conn.await {
                    if err.is_user() {
                        tracing::error!(%err, "h2 connection driver error");
                    } else {
                        tracing::debug!(%err, "h2 connection driver error");
                    }
                }
            });
            Ok(HttpConnection::h2(sender))
        })
    }
}
