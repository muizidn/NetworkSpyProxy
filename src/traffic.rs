use std::future::Future;

use hudsucker::{
    hyper::{Request, Response},
    tokio_tungstenite::tungstenite::Message,
    *,
};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use tracing::warn;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

use async_trait::async_trait;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[async_trait]
pub trait TrafficListener: Sync + Send {
    async fn request(&self, id: u64, request: Request<Bytes>, intercepted: bool, client_addr: String);
    async fn response(&self, id: u64, response: Response<Bytes>, intercepted: bool, client_addr: String);
}

#[derive(Clone)]
pub struct TrafficInterceptor {
    listener: Arc<dyn TrafficListener>,
    allow_list: Arc<RwLock<Vec<String>>>,
    request_id: Option<u64>,
    log_terminal: bool,
}

impl TrafficInterceptor {
    pub fn new(listener: Arc<dyn TrafficListener>, allow_list: Arc<RwLock<Vec<String>>>) -> Self {
        let log_terminal = std::env::var("LOG_TRAFFIC_TERMINAL").map(|v| v == "1").unwrap_or(false);
        TrafficInterceptor { listener, allow_list, request_id: None, log_terminal }
    }
}
struct RequestDuplicate {
    origin: Request<Body>,
    duplicate: Request<Bytes>
}

async fn duplicate_req(req: Request<Body>) -> RequestDuplicate {
    let (parts, body) = req.into_parts();
    
    let old_uri = &parts.uri;
    let old_method = &parts.method;
    let old_headers = &parts.headers;
    let old_ver = &parts.version;
    let old_bytes = match body.collect().await {
        Ok(b) => b.to_bytes(),
        Err(e) => {
            warn!("Failed to collect request body: {}", e);
            Bytes::new()
        }
    };
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

    let req = Request::from_parts(parts, Body::from(Full::new(new_bytes)));
    
    RequestDuplicate { origin: req, duplicate: req1 }
}

struct ResponseDuplicate {
    origin: Response<Body>,
    duplicate: Response<Bytes>
}

async fn duplicate_res(res: Response<Body>) -> ResponseDuplicate {
    let (parts, body) = res.into_parts();
    
    let old_status = &parts.status.clone();
    let old_headers = &parts.headers;
    let old_ver = &parts.version;
    let old_bytes = match body.collect().await {
        Ok(b) => b.to_bytes(),
        Err(e) => {
            warn!("Failed to collect response body: {}", e);
            Bytes::new()
        }
    };

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

    let res = Response::from_parts(parts, Body::from(Full::new(new_bytes)));
    
    ResponseDuplicate { origin: res, duplicate: res1 }
}

impl HttpHandler for TrafficInterceptor {
    fn handle_request(&mut self, _ctx: &HttpContext, req: Request<Body> ) -> impl Future<Output = RequestOrResponse> + Send {
        let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
        self.request_id = Some(id);

        let listener = Arc::clone(&self.listener);
        let client_addr = _ctx.client_addr.to_string();
        let intercepted = _ctx.intercepted;
        let log_terminal = self.log_terminal;

        async move {
            if log_terminal {
                println!("\x1b[32m[REQUEST  #{}]\x1b[0m {} {}", id, req.method(), req.uri());
            }

            let d = duplicate_req(req).await;
            listener.request(id, d.duplicate, intercepted, client_addr).await;

            RequestOrResponse::Request(d.origin)
        }
    }

    fn handle_response(&mut self, _ctx: &HttpContext, res: Response<Body>) -> impl Future<Output = Response<Body>> + Send {
        let id = self.request_id.unwrap_or_else(|| NEXT_ID.fetch_add(1, Ordering::SeqCst));

        let listener = Arc::clone(&self.listener);
        let client_addr = _ctx.client_addr.to_string();
        let intercepted = _ctx.intercepted;
        let log_terminal = self.log_terminal;

        async move {
            if log_terminal {
                println!("\x1b[34m[RESPONSE #{}]\x1b[0m {}", id, res.status());
            }

            let d = duplicate_res(res).await;
            listener.response(id, d.duplicate, intercepted, client_addr).await;

            d.origin
        }
    }

    fn should_intercept(&mut self, _ctx: &HttpContext, req: &Request<Body>) -> impl Future<Output = bool> + Send {
        let intercepted = _ctx.intercepted;
        let uri = req.uri().to_string();
        let host = req.headers().get("host").and_then(|h| h.to_str().ok()).map(|s| s.to_string()).unwrap_or_default();
        let allow_list = Arc::clone(&self.allow_list);

        async move {
            if intercepted {
                return true;
            }

            let allow_list_guard = allow_list.read().await;

            for domain in allow_list_guard.iter() {
                if uri.contains(domain) || host.contains(domain) {
                    return true;
                }
            }

            warn!("Interception NOT allowed for domain in URI: {} or Host: {}. Tunneling instead.", uri, host);
            false
        }
    }
}

impl WebSocketHandler for TrafficInterceptor {
    fn handle_message(&mut self, _ctx: &WebSocketContext, msg: Message) -> impl Future<Output = Option<Message>> + Send {
        let log_terminal = self.log_terminal;
        let msg_clone = msg.clone();
        
        async move {
            if log_terminal {
                println!("\x1b[35m[WS MESSAGE]\x1b[0m {:?}", msg_clone);
            }
            Some(msg_clone)
        }
    }
}