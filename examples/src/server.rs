use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};
use tokio_native_tls::native_tls::NativeAcceptorBuilder;
use tonic::{Request, Response, Status};
use tonic_transport::Server;

use crate::proto::hello_server::{Hello, HelloServer};
use crate::proto::{HelloRequest, HelloResponse};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls())?;
    builder.set_ca_file("certs/ca.crt")?;
    builder.set_certificate_chain_file("certs/server.crt")?;
    builder.set_private_key_file("certs/server.key", SslFiletype::PEM)?;
    builder.set_alpn_protos(b"\x02h2")?;
    builder.set_alpn_select_callback(|_, _| Ok(b"h2"));

    let tls = NativeAcceptorBuilder::new(builder).build()?;

    let addr = "[::1]:50051".parse()?;
    let greeter = MyGreeter::default();

    Server::builder(tls.into())
        .add_service(HelloServer::new(greeter))
        .serve(addr)
        .await?;

    Ok(())
}

#[derive(Debug, Default)]
pub struct MyGreeter {}

#[tonic::async_trait]
impl Hello for MyGreeter {
    async fn count_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloResponse>, Status> {
        println!("Got a request: {:?}", request);

        let reply = HelloResponse {
            message: format!("Hello {}!", request.into_inner().number).into(),
        };

        Ok(Response::new(reply))
    }
}

mod proto {
    tonic::include_proto!("hello");
}
