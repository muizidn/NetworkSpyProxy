use hudsucker::{
    certificate_authority::RcgenAuthority,
    rcgen::{CertificateParams, KeyPair},
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
        .with_graceful_shutdown(shutdown_signal())
        .build();

    runtime
    .block_on(async {
        if let Err(e) = proxy.start().await {
            error!("{}", e);
        }
    });
}
