use hudsucker::{
    async_trait::async_trait,
    certificate_authority::OpensslAuthority,
    hyper::{Body, Request, Response},
    openssl::{hash::MessageDigest, pkey::PKey, x509::X509},
    tokio_tungstenite::tungstenite::Message,
    *,
};
use std::{net::{SocketAddr, SocketAddrV4}};
use bytes::Bytes;
use tracing::*;

use crate::traffic::*;

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install CTRL+C signal handler");
}

#[derive(Clone)]
pub struct TrafficHandler {
    pub fn_req: fn(u8, &Box<fn(u8, *mut ReqBody)>, HttpContext, Request<Bytes>),
    pub fn_res: fn(u8, &Box<fn(u8, *mut ResBody)>, HttpContext, Response<Bytes>),
    pub req_callback: Box<fn(u8, *mut ReqBody)>,
    pub res_callback: Box<fn(u8, *mut ResBody)>,
    pub id: u8
}
struct ReqDuplicate {
    origin: Request<Body>,
    duplicate: Request<Bytes>
}

async fn duplicate_req(req: Request<Body>) -> ReqDuplicate {
    let (parts, body) = req.into_parts();
    
    let old_uri = &parts.uri;
    let old_method = &parts.method;
    let old_headers = &parts.headers;
    let old_ver = &parts.version;
    let old_bytes = hyper::body::to_bytes(body).await.unwrap();

    let new_bytes = old_bytes.clone();

    let mut req1 = Request::builder()
        .uri(old_uri)
        .method(old_method)
        .version(*old_ver)
        .body(old_bytes)
        .unwrap();
        for (k,v) in old_headers {
            let k_c = k.clone();
            let v_c = v.clone();
            req1.headers_mut().append(&k_c, v_c);
        }

    let req = Request::from_parts(parts, Body::from(new_bytes));
    
    ReqDuplicate { origin: req, duplicate: req1 }
}

struct ResDuplicate {
    origin: Response<Body>,
    duplicate: Response<Bytes>
}

async fn duplicate_res(res: Response<Body>) -> ResDuplicate {
    let (parts, body) = res.into_parts();
    
    let old_status = &parts.status.clone();
    let old_headers = &parts.headers;
    let old_ver = &parts.version;
    let old_bytes = hyper::body::to_bytes(body).await.unwrap();

    let new_bytes = old_bytes.clone();

    let mut res1 = Response::builder()
        .status(old_status)
        .version(*old_ver)
        .body(old_bytes)
        .unwrap();
        for (k,v) in old_headers {
            let k_c = k.clone();
            let v_c = v.clone();
            res1.headers_mut().append(&k_c, v_c);
        }

    let res = Response::from_parts(parts, Body::from(new_bytes));
    
    ResDuplicate { origin: res, duplicate: res1 }
}

#[async_trait]
impl HttpHandler for TrafficHandler {
    async fn handle_request(
        &mut self,
        _ctx: &HttpContext,
        req: Request<Body>,
    ) -> RequestOrResponse {
        let d = duplicate_req(req).await;
        let orig = d.origin;
        let dup = d.duplicate;

        (self.fn_req)(self.id, &self.req_callback, _ctx.clone(), dup);

        RequestOrResponse::Request(orig)
    }

    async fn handle_response(&mut self, _ctx: &HttpContext, res: Response<Body>) -> Response<Body> {
        let d = duplicate_res(res).await;
        let orig = d.origin;
        let dup = d.duplicate;

        (self.fn_res)(self.id, &self.res_callback, _ctx.clone(), dup);

        orig
    }
}

#[async_trait]
impl WebSocketHandler for TrafficHandler {
    async fn handle_message(&mut self, _ctx: &WebSocketContext, msg: Message) -> Option<Message> {
        println!("{:?}", msg);
        Some(msg)
    }
}

pub struct OpenSSL {
    pub addr: SocketAddrV4
}

impl OpenSSL {
    pub async fn run(&self, handler: TrafficHandler) {
        tracing_subscriber::fmt::init();

        let private_key_bytes: &[u8] = include_bytes!("ca/hudsucker.key");
        let ca_cert_bytes: &[u8] = include_bytes!("ca/hudsucker.cer");
        let private_key =
            PKey::private_key_from_pem(private_key_bytes).expect("Failed to parse private key");
        let ca_cert = X509::from_pem(ca_cert_bytes).expect("Failed to parse CA certificate");

        let ca = OpensslAuthority::new(private_key, ca_cert, MessageDigest::sha256(), 1_000);

        let proxy = Proxy::builder()
            .with_addr(SocketAddr::from(self.addr))
            .with_rustls_client()
            .with_ca(ca)
            .with_http_handler(handler)
            .build();

        if let Err(e) = proxy.start(shutdown_signal()).await {
            println!("==> ERROR{}", e);
        }
    }
}