use http::Uri;
use http_body_util::BodyExt as _;
use patron::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let mut client = Client::new_tcp_http();

    let uri: Uri = "https://www.google.com".parse()?;
    let res = client.get(uri.clone()).await?;

    println!("1 Response: {} - {:?}", res.status(), res.version());

    for (name, value) in res.headers() {
        if let Ok(value) = value.to_str() {
            println!("  {}: {}", name, value);
        }
    }

    let r2 = client.get(uri.clone()).await?;
    println!("2 Response: {}", r2.status());

    let mut body = res.into_body();

    let mut total = 0usize;
    while let Some(Ok(frame)) = body.frame().await {
        if let Some(data) = frame.data_ref() {
            total += data.len();
        }
    }
    println!("Recieved {} body bytes", total);
    drop(r2);

    let r3 = client.get(uri).await?;
    println!("3 Response: {}", r3.status());

    Ok(())
}
