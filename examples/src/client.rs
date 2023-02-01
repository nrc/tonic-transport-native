use crate::proto::hello_client::HelloClient;
use crate::proto::HelloRequest;
use openssl::ssl::{SslConnector, SslFiletype, SslMethod};
use tokio_native_tls::native_tls::NativeConnectorBuilder;
use tonic_transport::Channel;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = SslConnector::builder(SslMethod::tls())?;
    builder.set_ca_file("certs/ca.crt")?;
    builder.set_certificate_chain_file("certs/client.crt")?;
    builder.set_private_key_file("certs/client.key", SslFiletype::PEM)?;
    builder.set_alpn_protos(b"\x02h2")?;

    let sync_connector = NativeConnectorBuilder::new(builder).build()?;

    let channel = Channel::from_static("http://[::1]:50051", sync_connector.into())?
        .tls_verify_domain("localhost".to_owned())
        .connect()
        .await?;

    let mut client = HelloClient::new(channel);
    let request = tonic::Request::new(HelloRequest { number: 42.into() });
    let response = client.count_hello(request).await?;

    println!("{}", response.into_inner().message);

    Ok(())
}
mod proto {
    tonic::include_proto!("hello");
}
