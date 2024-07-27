use core::{hash, str};
use std::hash::{DefaultHasher, Hash, Hasher};

use hudsucker::{
    hyper::{Request, Response},
    tokio_tungstenite::tungstenite::Message,
    *,
};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};

use std::sync::Arc;

pub trait TrafficListener: Sync + Send {
    fn request(&self, id: u64, request: Request<Bytes>);
    fn response(&self, id: u64, response: Response<Bytes>);
}

#[derive(Clone)]
pub struct TrafficInterceptor {
    listener: Arc<dyn TrafficListener>,
}

impl TrafficInterceptor {
    pub fn new(listener: Arc<dyn TrafficListener>) -> Self {
        TrafficInterceptor { listener }
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
        let d = duplicate_req(req).await;
        let original_request = d.origin;
        let duplicated_request = d.duplicate;

        let mut hasher = DefaultHasher::new();
        _ctx.hash(&mut hasher);
        self.listener.request(hasher.finish(), duplicated_request);

        RequestOrResponse::Request(original_request)
    }

    async fn handle_response(&mut self, _ctx: &HttpContext, res: Response<Body>) -> Response<Body> {
        let d = duplicate_res(res).await;
        let original_response = d.origin;
        let duplicated_response = d.duplicate;

        let mut hasher = DefaultHasher::new();
        _ctx.hash(&mut hasher);
        self.listener.response(hasher.finish(), duplicated_response);

        original_response
    }
}

impl WebSocketHandler for TrafficInterceptor {
    async fn handle_message(&mut self, _ctx: &WebSocketContext, msg: Message) -> Option<Message> {
        println!("{:?}", msg);
        Some(msg)
    }
}