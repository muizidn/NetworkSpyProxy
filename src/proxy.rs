use bytes::Bytes;
use hudsucker::{
    certificate_authority::RcgenAuthority,
    hyper::{Request, Response},
    rcgen::{CertificateParams, KeyPair},
    tokio_tungstenite::tungstenite::Message,
    *,
};
use std::{net::SocketAddr, sync::{atomic::{AtomicBool, Ordering}, Arc}};
use tokio::{runtime::{Builder, Runtime}, sync::Notify};
use tracing::*;

use crate::traffic::{TrafficInterceptor, TrafficListener};

pub struct Proxy {
    key_pair: &'static str,
    ca_cert: &'static str,
    port: u16,
    notify: Arc<Notify>,
    is_running: Arc<AtomicBool>,
}

impl Proxy {
    pub fn new(key_pair: &'static str, ca_cert: &'static str, port: u16) -> Self {
        Proxy {
            key_pair,
            ca_cert,
            port,
            notify: Arc::new(Notify::new()),
            is_running: Arc::new(AtomicBool::new(true)),
        }
    }

    async fn shutdown_signal(&self) {
        self.notify.notified().await;
    }

    pub fn run_proxy(&mut self, listener: Arc<dyn TrafficListener + Send + Sync>) {
        tracing_subscriber::fmt::init();

        let runtime = Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("network-spy-proxy")
            .thread_stack_size(3 * 1024 * 1024)
            .enable_io()
            .enable_time()
            .build()
            .unwrap();

        let key_pair = KeyPair::from_pem(self.key_pair).expect("Failed to parse private key");
        let ca_cert = CertificateParams::from_ca_cert_pem(self.ca_cert)
            .expect("Failed to parse CA certificate")
            .self_signed(&key_pair)
            .expect("Failed to sign CA certificate");

        let ca = RcgenAuthority::new(key_pair, ca_cert, 1_000);

        let traffic = TrafficInterceptor::new(listener);

        let proxy = hudsucker::Proxy::builder()
            .with_addr(SocketAddr::from(([127, 0, 0, 1], self.port)))
            .with_rustls_client()
            .with_ca(ca)
            .with_http_handler(traffic.clone())
            .with_websocket_handler(traffic.clone())
            // FIXME: I don't know how to fix yet 🥹
            // .with_graceful_shutdown(self.shutdown_signal())
            .build();

            runtime.block_on(async {
                if let Err(e) = proxy.start().await {
                    error!("{}", e);
                }
            });
    }

    pub fn stop_proxy(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
        self.notify.notify_one();
    }
}

// struct MyTrafficListener;

// impl TrafficListener for MyTrafficListener {
//     fn request(&self, id: u64, request: Request<Bytes>) {
//         println!("Received request with id {}: {:?}", id, request);
//     }

//     fn response(&self, id: u64, response: Response<Bytes>) {
//         println!("Sending response with id {}: {:?}", id, response);
//     }
// }

// fn main() {
//     let proxy = Proxy::new("path/to/cert".to_string(), 3000);
//     let listener = Arc::new(MyTrafficListener);
//     proxy.run_proxy(listener);
// }
