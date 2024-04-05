#[cfg(feature = "tls")]
use rustls::ClientConfig;

use crate::client::{conn::http::HttpConnectionBuilder, Client};

#[cfg(feature = "tls")]
use crate::client::default_tls_config;

#[derive(Debug)]
pub struct Builder {
    tcp: crate::client::conn::TcpConnectionConfig,
    #[cfg(feature = "tls")]
    tls: Option<ClientConfig>,
    pool: Option<crate::client::pool::Config>,
    conn: crate::client::conn::http::HttpConnectionBuilder,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            #[cfg(feature = "stream")]
            tcp: Default::default(),
            #[cfg(feature = "tls")]
            tls: Some(default_tls_config()),
            pool: Some(Default::default()),
            conn: Default::default(),
        }
    }
}

impl Builder {
    #[cfg(feature = "stream")]
    pub fn tcp(&mut self) -> &mut crate::client::conn::TcpConnectionConfig {
        &mut self.tcp
    }

    #[cfg(feature = "tls")]
    pub fn with_tls(&mut self, config: ClientConfig) -> &mut Self {
        self.tls = Some(config);
        self
    }

    pub fn pool(&mut self) -> &mut Option<crate::client::pool::Config> {
        &mut self.pool
    }

    pub fn conn(&mut self) -> &mut crate::client::conn::http::HttpConnectionBuilder {
        &mut self.conn
    }
}

impl Builder {
    pub fn build(self) -> Client<HttpConnectionBuilder> {
        Client {
            #[cfg(feature = "tls")]
            transport: crate::client::conn::TcpConnector::new(
                self.tcp,
                self.tls.unwrap_or_else(super::default_tls_config),
            ),

            #[cfg(not(feature = "tls"))]
            transport: crate::client::conn::TcpConnector::new(self.tcp),

            protocol: HttpConnectionBuilder::default(),
            pool: self.pool.map(crate::client::pool::Pool::new),
        }
    }
}
