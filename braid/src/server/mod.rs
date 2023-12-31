//! Server side of the Braid stream
//!
//! The server and client are differentiated for TLS support, but otherwise,
//! TCP and Duplex streams are the same whether they are server or client.

use std::io;

use pin_project::pin_project;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpStream, UnixStream};

use crate::core::{Braid, BraidCore};
use crate::duplex::DuplexStream;
use crate::info::{Connection as HasConnectionInfo, ConnectionInfo, SocketAddr};
use crate::tls::info::TlsConnectionInfoReciever;
use crate::tls::server::TlsStream;

mod acceptor;
mod connector;

pub use acceptor::Acceptor;
pub use connector::{Connection, StartConnectionInfoLayer, StartConnectionInfoService};

#[derive(Debug, Clone)]
enum ConnectionInfoState {
    Handshake(TlsConnectionInfoReciever),
    Connected(ConnectionInfo),
}

impl ConnectionInfoState {
    async fn recv(&self) -> io::Result<ConnectionInfo> {
        match self {
            ConnectionInfoState::Handshake(rx) => rx.recv().await,
            ConnectionInfoState::Connected(info) => Ok(info.clone()),
        }
    }
}

/// An async generator of new connections
pub trait Accept {
    /// The connection type for this acceptor
    type Conn: AsyncRead + AsyncWrite + Send + Unpin + 'static;

    /// The error type for this acceptor
    type Error: Into<Box<dyn std::error::Error + Send + Sync>>;

    /// Poll for a new connection
    fn poll_accept(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<Self::Conn, Self::Error>>;
}

/// Dispatching wrapper for potential stream connection types for clients
#[derive(Debug)]
#[pin_project]
pub struct Stream {
    info: ConnectionInfoState,

    #[pin]
    inner: Braid<TlsStream<BraidCore>>,
}

impl Stream {
    /// Get the connection info for this stream
    ///
    /// This will block until the handshake completes for
    /// TLS connections.
    pub async fn info(&self) -> io::Result<ConnectionInfo> {
        match &self.info {
            ConnectionInfoState::Handshake(rx) => rx.recv().await,
            ConnectionInfoState::Connected(info) => Ok(info.clone()),
        }
    }

    /// Get the remote address for this stream.
    ///
    /// This can be done before the TLS handshake completes.
    pub fn remote_addr(&self) -> &SocketAddr {
        match &self.info {
            ConnectionInfoState::Handshake(rx) => rx.remote_addr(),
            ConnectionInfoState::Connected(info) => info.remote_addr(),
        }
    }

    /// Finish the TLS handshake now, driving the connection to completion.
    ///
    /// This is a no-op for non-TLS connections.
    pub async fn finish_handshake(&mut self) -> io::Result<()> {
        match self.inner {
            Braid::Tls(ref mut stream) => stream.finish_handshake().await,
            _ => Ok(()),
        }
    }
}

impl HasConnectionInfo for Stream {
    fn info(&self) -> ConnectionInfo {
        match &self.info {
            ConnectionInfoState::Handshake(_) => {
                panic!("connection info is not avaialble before the handshake completes")
            }
            ConnectionInfoState::Connected(info) => info.clone(),
        }
    }
}

impl From<TlsStream<BraidCore>> for Stream {
    fn from(stream: TlsStream<BraidCore>) -> Self {
        Stream {
            info: ConnectionInfoState::Handshake(stream.rx.clone()),
            inner: Braid::Tls(stream),
        }
    }
}

impl From<TcpStream> for Stream {
    fn from(stream: TcpStream) -> Self {
        Stream {
            info: ConnectionInfoState::Connected(<TcpStream as HasConnectionInfo>::info(&stream)),
            inner: stream.into(),
        }
    }
}

impl From<DuplexStream> for Stream {
    fn from(stream: DuplexStream) -> Self {
        Stream {
            info: ConnectionInfoState::Connected(<DuplexStream as HasConnectionInfo>::info(
                &stream,
            )),
            inner: stream.into(),
        }
    }
}

impl From<UnixStream> for Stream {
    fn from(stream: UnixStream) -> Self {
        Stream {
            info: ConnectionInfoState::Connected(stream.info()),
            inner: stream.into(),
        }
    }
}

impl From<BraidCore> for Stream {
    fn from(stream: BraidCore) -> Self {
        Stream {
            info: ConnectionInfoState::Connected(stream.info()),
            inner: stream.into(),
        }
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.project().inner.poll_read(cx, buf)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        self.project().inner.poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        self.project().inner.poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        self.project().inner.poll_shutdown(cx)
    }
}
