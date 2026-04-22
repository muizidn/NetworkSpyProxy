use std::future::Future;

use hudsucker::{
    hyper::{Request, Response},
    tokio_tungstenite::tungstenite::Message,
    *,
};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use tracing::{warn, info};

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

use async_trait::async_trait;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[async_trait]
pub trait TrafficListener: Sync + Send {
    async fn request(&self, id: u64, request: Request<Bytes>, intercepted: bool, client_addr: String) -> Request<Bytes>;
    async fn response(&self, id: u64, response: Response<Bytes>, intercepted: bool, client_addr: String) -> Response<Bytes>;
    async fn get_client_name(&self, _client_addr: &str) -> String { "Unknown".to_string() }
    async fn should_intercept(&self, _uri: &str, _host: &str, _client_addr: &str) -> bool { true }
}

#[derive(Clone)]
pub struct TrafficInterceptor {
    listener: Arc<dyn TrafficListener>,
    proxy_intercept_list: Arc<RwLock<Vec<String>>>,
    request_id: Option<u64>,
    log_terminal: bool,
    log_interception_logic: bool,
    skipped: bool,
}

impl TrafficInterceptor {
    pub fn new(listener: Arc<dyn TrafficListener>, proxy_intercept_list: Arc<RwLock<Vec<String>>>) -> Self {
        let log_terminal = std::env::var("LOG_TRAFFIC_TERMINAL").map(|v| v == "1").unwrap_or(false);
        let log_interception_logic = std::env::var("PROXY_INTERCEPTION_LOGIC_LOG").map(|v| v == "1").unwrap_or(false);

        if cfg!(debug_assertions) {
            if !log_terminal {
                println!("\x1b[32m[INFO]\x1b[0m Traffic terminal logging is disabled. Enable with LOG_TRAFFIC_TERMINAL=1");
            }
            if !log_interception_logic {
                println!("\x1b[32m[INFO]\x1b[0m Interception logic logging is disabled. Enable with PROXY_INTERCEPTION_LOGIC_LOG=1");
            }
        }

        TrafficInterceptor { listener, proxy_intercept_list, request_id: None, log_terminal, log_interception_logic, skipped: false }
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

fn wildcard_match(pattern: &str, text: &str) -> bool {
    if !pattern.contains('*') {
        return text.contains(pattern);
    }
    
    let parts: Vec<&str> = pattern.split('*').collect();
    if !text.starts_with(parts[0]) {
        return false;
    }
    
    let mut current_pos = parts[0].len();
    for i in 1..parts.len() {
        if parts[i].is_empty() {
            if i == parts.len() - 1 {
                return true; // Ends with *
            }
            continue;
        }
        if let Some(found_pos) = text[current_pos..].find(parts[i]) {
            current_pos += found_pos + parts[i].len();
        } else {
            return false;
        }
    }
    current_pos == text.len() || pattern.ends_with('*')
}

async fn check_interception(
    intercepted: bool,
    uri: &str,
    host: &str,
    proxy_intercept_list: &Arc<RwLock<Vec<String>>>,
    listener: &Arc<dyn TrafficListener>,
    client_addr: &str,
    log_logic: bool,
) -> bool {
    if intercepted {
        return true;
    }

    let proxy_list_guard = proxy_intercept_list.read().await;
    let mut should_intercept = false;

    if !proxy_list_guard.is_empty() {
        let mut client_name = None;
        for rule in proxy_list_guard.iter() {
            if rule.is_empty() { continue; }

            if rule.starts_with("client:") {
                let pattern = &rule[7..];
                if client_name.is_none() {
                    client_name = Some(listener.get_client_name(client_addr).await);
                }
                if let Some(name) = &client_name {
                    if wildcard_match(&pattern.to_lowercase(), &name.to_lowercase()) {
                        if log_logic {
                            println!("\x1b[32m[INTERCEPT]\x1b[0m Rule matched! Client: {} matches pattern: {}", name, pattern);
                        }
                        should_intercept = true;
                        break;
                    }
                }
            } else {
                let rule_has_protocol = rule.contains("://");
                
                // 1. Try direct match (covers strict protocol rules and raw host:port matches)
                if wildcard_match(rule, uri) || wildcard_match(rule, host) {
                    if log_logic {
                        println!("\x1b[32m[INTERCEPT]\x1b[0m Rule matched! matches: {}", rule);
                    }
                    should_intercept = true;
                    break;
                }

                // 2. If the rule is a "flexible" rule (no protocol), try matching against stripped targets
                if !rule_has_protocol {
                    let clean_uri = if let Some(pos) = uri.find("://") {
                        &uri[pos + 3..]
                    } else {
                        uri
                    };
                    let clean_host = if let Some(pos) = host.find("://") {
                        &host[pos + 3..]
                    } else {
                        host
                    };

                    if wildcard_match(rule, clean_uri) || wildcard_match(rule, clean_host) {
                        if log_logic {
                            println!("\x1b[32m[INTERCEPT]\x1b[0m Flexible rule matched! Rule: {} matches Clean URI/Host", rule);
                        }
                        should_intercept = true;
                        break;
                    }
                }
            }
        }
    } else if log_logic {
        println!("\x1b[33m[SKIP]\x1b[0m Allow list is empty. Skipping: {}", uri);
    }

    if should_intercept {
        if !listener.should_intercept(uri, host, client_addr).await {
            if log_logic {
                println!("\x1b[31m[REJECT]\x1b[0m Rule matched but listener REJECTED: {}", uri);
            }
            should_intercept = false;
        }
    }

    if !should_intercept && !intercepted && !proxy_list_guard.is_empty() && log_logic {
        println!("\x1b[33m[SKIP]\x1b[0m No rules matched for: {} (Host: {})", uri, host);
        println!("\x1b[34m[PROXY_LIST_GUARD]\x1b[0m Rules: {:?}", proxy_list_guard);
    }

    should_intercept
}

impl HttpHandler for TrafficInterceptor {
    fn handle_request(&mut self, _ctx: &HttpContext, req: Request<Body> ) -> impl Future<Output = RequestOrResponse> + Send {
        let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
        self.request_id = Some(id);

        let listener = Arc::clone(&self.listener);
        let client_addr = _ctx.client_addr.to_string();
        let intercepted = _ctx.intercepted;
        let log_terminal = self.log_terminal;
        let proxy_intercept_list = Arc::clone(&self.proxy_intercept_list);
        let log_logic = self.log_interception_logic;

        async move {
            if log_terminal {
                println!("\x1b[32m[REQUEST  #{}]\x1b[0m {} {}", id, req.method(), req.uri());
            }

            let d = duplicate_req(req).await;
            
            // Re-calculate interception status for the listener to ensure CONNECT 
            // requests correctly reflect their "to-be-intercepted" state.
            // Documentation: 
            // - If a CONNECT request is NOT to be intercepted, intercepted=false will be sent to the listener.
            // - If a CONNECT request IS to be intercepted, intercepted=true will be sent, 
            //   allowing the listener to filter it out from the UI since decrypted traffic follows.
            let uri = d.duplicate.uri().to_string();
            let host = d.duplicate.headers().get("host").and_then(|h| h.to_str().ok()).map(|s| s.to_string()).unwrap_or_default();
            let should_intercept = check_interception(intercepted, &uri, &host, &proxy_intercept_list, &listener, &client_addr, log_logic).await;

            let modified = listener.request(id, d.duplicate, should_intercept, client_addr).await;

            let (mut parts, _) = d.origin.into_parts();
            let (m_parts, m_body) = modified.into_parts();
            parts.method = m_parts.method;
            parts.uri = m_parts.uri;
            parts.version = m_parts.version;
            parts.headers = m_parts.headers;

            RequestOrResponse::Request(Request::from_parts(parts, Body::from(Full::new(m_body))))
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
            let modified = listener.response(id, d.duplicate, intercepted, client_addr).await;

            let (mut parts, _) = d.origin.into_parts();
            let (m_parts, m_body) = modified.into_parts();
            parts.status = m_parts.status;
            parts.version = m_parts.version;
            parts.headers = m_parts.headers;

            Response::from_parts(parts, Body::from(Full::new(m_body)))
        }
    }

    fn should_intercept(&mut self, _ctx: &HttpContext, req: &Request<Body>) -> impl Future<Output = bool> + Send {
        let intercepted = _ctx.intercepted;
        let uri = req.uri().to_string();
        let host = req.headers().get("host").and_then(|h| h.to_str().ok()).map(|s| s.to_string()).unwrap_or_default();
        let proxy_intercept_list = Arc::clone(&self.proxy_intercept_list);
        let listener = Arc::clone(&self.listener);
        let client_addr = _ctx.client_addr.to_string();
        let log_logic = self.log_interception_logic;

        async move {
            check_interception(intercepted, &uri, &host, &proxy_intercept_list, &listener, &client_addr, log_logic).await
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