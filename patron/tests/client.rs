use futures_util::StreamExt;
use http::StatusCode;
use hyper_util::rt::TokioIo;
use std::pin::pin;

use patron::conn::duplex::DuplexTransport;
use patron::{HttpConnector, PoolConfig};
type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[tokio::test]
async fn client() -> Result<(), BoxError> {
    let (client, incoming) = braid::duplex::pair("test".parse().unwrap());

    let acceptor = braid::server::Acceptor::from(incoming);

    tokio::spawn(serve_one_h1(acceptor));

    let mut client = patron::Client::new(
        HttpConnector::default(),
        DuplexTransport::new(1024, None, client),
        PoolConfig::default(),
    );

    let resp = client.get("http://test/".parse().unwrap()).await?;

    assert_eq!(resp.status(), StatusCode::OK);

    Ok(())
}

async fn service_ok(
    req: http::Request<hyper::body::Incoming>,
) -> Result<arnold::Response, BoxError> {
    Ok(http::response::Builder::new()
        .status(200)
        .header("O-Host", req.headers().get("Host").unwrap())
        .body(arnold::Body::empty())?)
}

async fn serve_one_h1(acceptor: braid::server::Acceptor) -> Result<(), BoxError> {
    let mut acceptor = pin!(acceptor);
    let stream = acceptor.next().await.ok_or("no connection")??;

    let service = hyper::service::service_fn(service_ok);

    let conn =
        hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(stream), service);

    conn.await?;

    Ok(())
}
