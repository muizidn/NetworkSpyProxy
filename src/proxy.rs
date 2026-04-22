use hudsucker::{
    certificate_authority::RcgenAuthority,
    rcgen::{KeyPair, Issuer},
    rustls::crypto::aws_lc_rs,
};
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::sync::{Notify, RwLock};
use tracing::*;

use crate::traffic::{TrafficInterceptor, TrafficListener, ProxyRule};

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

    pub async fn run_proxy(
        &mut self,
        listener: Arc<dyn TrafficListener + Send + Sync>,
        allow_list: Arc<RwLock<Vec<ProxyRule>>>,
    ) {
        // tracing_subscriber::fmt::init(); // Handled by caller or globally

        let key_pair = KeyPair::from_pem(self.key_pair).expect("Failed to parse private key");
        let issuer = Issuer::from_ca_cert_pem(self.ca_cert, key_pair).expect("Failed to parse CA certificate");
        let ca = RcgenAuthority::new(issuer, 1_000, aws_lc_rs::default_provider());
 
        let traffic = TrafficInterceptor::new(listener, allow_list);

        let proxy = hudsucker::Proxy::builder()
            .with_addr(SocketAddr::from(([127, 0, 0, 1], self.port)))
            .with_ca(ca)
            .with_rustls_connector(aws_lc_rs::default_provider())
            .with_http_handler(traffic.clone())
            .with_websocket_handler(traffic.clone())
            .build()
            .expect("Failed to build proxy");

        if let Err(e) = proxy.start().await {
            error!("{}", e);
        }
    }

    pub fn stop_proxy(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
        self.notify.notify_one();
    }
}
