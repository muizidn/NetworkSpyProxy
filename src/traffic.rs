use core::str;
use std::hash::{DefaultHasher, Hash, Hasher};

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

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub trait TrafficListener: Sync + Send {
    fn request(&self, id: u64, request: Request<Bytes>, intercepted: bool, client_addr: String);
    fn response(&self, id: u64, response: Response<Bytes>, intercepted: bool, client_addr: String);
}

#[derive(Clone)]
pub struct TrafficInterceptor {
    listener: Arc<dyn TrafficListener>,
    allow_list: Arc<RwLock<Vec<String>>>,
    request_id: Option<u64>,
}

impl TrafficInterceptor {
    pub fn new(listener: Arc<dyn TrafficListener>, allow_list: Arc<RwLock<Vec<String>>>) -> Self {
        TrafficInterceptor { listener, allow_list, request_id: None }
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
    let old_bytes = body.collect().await.unwrap().to_bytes();
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
    let old_bytes = body.collect().await.unwrap().to_bytes();

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
    async fn handle_request(&mut self, _ctx: &HttpContext, req: Request<Body> ) -> RequestOrResponse {
        let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
        self.request_id = Some(id);

        let d = duplicate_req(req).await;
        let original_request = d.origin;
        let duplicated_request = d.duplicate;

        self.listener.request(id, duplicated_request, _ctx.intercepted, _ctx.client_addr.to_string());

        RequestOrResponse::Request(original_request)
    }

    async fn handle_response(&mut self, _ctx: &HttpContext, res: Response<Body>) -> Response<Body> {
        let id = self.request_id.unwrap_or_else(|| NEXT_ID.fetch_add(1, Ordering::SeqCst));

        let d = duplicate_res(res).await;
        let original_response = d.origin;
        let duplicated_response = d.duplicate;

        self.listener.response(id, duplicated_response, _ctx.intercepted, _ctx.client_addr.to_string());

        original_response
    }

    async fn should_intercept(&mut self, _ctx: &HttpContext, req: &Request<Body>) -> bool {
        if _ctx.intercepted {
            return true;
        }

        let uri = req.uri().to_string();
        let host = req.headers().get("host").and_then(|h| h.to_str().ok()).unwrap_or("");
        let allow_list = self.allow_list.read().await;

        for domain in allow_list.iter() {
            if uri.contains(domain) || host.contains(domain) {
                return true;
            }
        }

        warn!("Interception NOT allowed for domain in URI: {} or Host: {}. Tunneling instead.", uri, host);
        false
    }
}

impl WebSocketHandler for TrafficInterceptor {
    async fn handle_message(&mut self, _ctx: &WebSocketContext, msg: Message) -> Option<Message> {
        println!("{:?}", msg);
        Some(msg)
    }
}