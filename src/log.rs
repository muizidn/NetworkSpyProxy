use hudsucker::{
    certificate_authority::RcgenAuthority,
    hyper::{Request, Response},
    rcgen::{CertificateParams, KeyPair},
    tokio_tungstenite::tungstenite::Message,
    *,
};
use std::net::SocketAddr;
use tracing::*;
use tokio::runtime::{Runtime, Builder};

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install CTRL+C signal handler");
}

#[derive(Clone)]
struct LogHandler;

impl HttpHandler for LogHandler {
    async fn handle_request(
        &mut self,
        _ctx: &HttpContext,
        req: Request<Body>,
    ) -> RequestOrResponse {
        println!("{:?}", req);
        req.into()
    }

    async fn handle_response(&mut self, _ctx: &HttpContext, res: Response<Body>) -> Response<Body> {
        println!("{:?}", res);
        res
    }
}

impl WebSocketHandler for LogHandler {
    async fn handle_message(&mut self, _ctx: &WebSocketContext, msg: Message) -> Option<Message> {
        println!("{:?}", msg);
        Some(msg)
    }
}


pub fn run_proxy() {
    tracing_subscriber::fmt::init();

    let runtime = Builder::new_current_thread()
        .worker_threads(4)
        .thread_name("network-spy-proxy")
        .thread_stack_size(3 * 1024 * 1024)
        .enable_io()
        .enable_time()
        .build()
        .unwrap();

    let key_pair = include_str!("ca/hudsucker.key");
    let ca_cert = include_str!("ca/hudsucker.cer");
    let key_pair = KeyPair::from_pem(key_pair).expect("Failed to parse private key");
    let ca_cert = CertificateParams::from_ca_cert_pem(ca_cert)
        .expect("Failed to parse CA certificate")
        .self_signed(&key_pair)
        .expect("Failed to sign CA certificate");

    let ca = RcgenAuthority::new(key_pair, ca_cert, 1_000);

    let proxy = Proxy::builder()
        .with_addr(SocketAddr::from(([127, 0, 0, 1], 3000)))
        .with_rustls_client()
        .with_ca(ca)
        .with_http_handler(LogHandler)
        .with_websocket_handler(LogHandler)
        .with_graceful_shutdown(shutdown_signal())
        .build();

    runtime
    .block_on(async {
        if let Err(e) = proxy.start().await {
            error!("{}", e);
        }
    });
}
