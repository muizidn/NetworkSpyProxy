# Interception Flow

This document traces an HTTPS request through the entire proxy pipeline, from client connect to Swift callback.

## Complete End-to-End Flow

```
Client                    NetworkSpyProxy                          Target Server
  │                              │                                      │
  │ 1. CONNECT example.com:443  │                                      │
  │────────────────────────────>│                                      │
  │                              │                                      │
  │ 2. InternalProxy::proxy()   │                                      │
  │    method == CONNECT         │                                      │
  │                              │                                      │
  │ 3. TrafficInterceptor::     │                                      │
  │    handle_request()          │                                      │
  │    → duplicate_req()        │                                      │
  │    → listener.request()     │                                      │
  │    → returns CONNECT req    │                                      │
  │                              │                                      │
  │ 4. process_connect():        │                                      │
  │    a) Upgrade connection     │                                      │
  │    b) Peek 4 bytes          │                                      │
  │    c) \x16\x03 → TLS!       │                                      │
  │                              │                                      │
  │ 5. TrafficInterceptor::     │                                      │
  │    should_intercept()        │                                      │
  │    → check_interception()   │                                      │
  │    → match rules            │                                      │
  │    → decide: INTERCEPT      │                                      │
  │                              │                                      │
  │ 6. RcgenAuthority:          │                                      │
  │    gen_server_config(       │                                      │
  │      "example.com")         │                                      │
  │    ├── Check cache           │                                      │
  │    ├── Generate cert         │                                      │
  │    ├── Sign with CA          │                                      │
  │    └── Return ServerConfig  │                                      │
  │                              │                                      │
  │ 7. TlsAcceptor::accept()    │                                      │
  │    ← TLS handshake with     │                                      │
  │      dynamic "example.com"  │                                      │
  │      cert                   │                                      │
  │    → Decrypted stream       │                                      │
  │                              │                                      │
  │ 8. serve_stream():          │                                      │
  │    HTTP service_fn over     │                                      │
  │    the decrypted stream     │                                      │
  │                              │                                      │
  │ 9. Client sends HTTP        │                                      │
  │    GET https://example.com/  │                                      │
  │────────────────────────────>│                                      │
  │                              │                                      │
  │10. InternalProxy::proxy()   │                                      │
  │    (intercepted=true)        │                                      │
  │                              │                                      │
  │11. TrafficInterceptor::      │                                      │
  │    handle_request():         │                                      │
  │    a) duplicate_req()       │                                      │
  │    b) check_interception()  │                                      │
  │    c) listener.request()    │    12. Forward HTTP request          │
  │    d) return modified req   │─────────────────────────────────────>│
  │                              │                                      │
  │                              │    13. Target responds              │
  │                              │<─────────────────────────────────────│
  │                              │                                      │
  │14. TrafficInterceptor::      │                                      │
  │    handle_response():        │                                      │
  │    a) duplicate_res()       │                                      │
  │    b) listener.response()   │                                      │
  │    c) return modified res   │                                      │
  │                              │                                      │
  │15. Response sent to client   │                                      │
  │<────────────────────────────│                                      │
```

## Detailed Step Walkthrough

### Phase 1: TCP Connection & CONNECT

**File**: `hudsucker/src/proxy/mod.rs` — `Proxy::start()`

```
1. TcpListener::bind(127.0.0.1:8080).await
2. loop: listener.accept().await
3. For each connection:
   a. Clone InternalProxy state (CA, client, handlers, etc.)
   b. spawn task → hyper server::auto connection
   c. service_fn → InternalProxy::proxy(req)
```

### Phase 2: CONNECT Processing

**File**: `hudsucker/src/proxy/internal.rs` — `InternalProxy::proxy()`

```rust
// req = CONNECT example.com:443 HTTP/1.1
let req = self.http_handler.handle_request(&ctx, req).await;
//  → TrafficInterceptor::handle_request()
//  → duplicates request
//  → calls listener.request()
//  → returns the CONNECT request unchanged
```

### Phase 3: Interception Decision

**File**: `hudsucker/src/proxy/internal.rs` — `process_connect()`

After the CONNECT handshake (HTTP upgrade), hudsucker reads the first 4 bytes to detect the protocol:

```rust
let mut buffer = [0; 4];
upgraded.read(&mut buffer).await?;

// Check with handler: should we MITM this connection?
if self.http_handler.should_intercept(&self.context(), &req).await {
```

This calls `TrafficInterceptor::should_intercept()` which runs `check_interception()` — the rule matching engine (see [Rule Matching](rule-matching.md)).

### Phase 4: Dynamic Certificate Generation

If interception is approved, and the first bytes look like TLS (`\x16\x03` = TLS ClientHello):

```rust
// Generate or retrieve cached cert for example.com
let server_config = self.ca.gen_server_config(&authority).await;

// Accept TLS connection with the client using our dynamic cert
let tls_stream = TlsAcceptor::from(server_config).accept(upgraded).await?;
```

See [On-The-Fly Certificates](on-the-fly-certificates.md) for details.

### Phase 5: Decrypted Stream Service

**File**: `hudsucker/src/proxy/internal.rs` — `self.serve_stream()`

```rust
async fn serve_stream<I>(self, stream: I, scheme: Scheme, authority: Authority) {
    let service = service_fn(|mut req| {
        // Rewrite the URI with scheme + authority (client sent only path)
        let (mut parts, body) = req.into_parts();
        parts.uri = {
            let mut parts = parts.uri.into_parts();
            parts.scheme = Some(scheme.clone());
            parts.authority = Some(authority.clone());
            Uri::from_parts(parts)?
        };
        req = Request::from_parts(parts, body);

        // Re-enter the proxy pipeline with intercepted=true
        let mut p = self.clone();
        p.intercepted = true;
        p.proxy(req)  // ← recursive call, but now intercepted=true
    });

    // Serve HTTP over the decrypted TLS stream
    self.server.serve_connection_with_upgrades(stream, service).await;
}
```

The key insight: after TLS is stripped, the decrypted HTTP traffic is fed back into the **same proxy pipeline** with `intercepted=true`. This means the `HttpHandler` receives the actual HTTP requests/responses in plaintext.

### Phase 6: Request Interception

**File**: `src/traffic.rs` — `TrafficInterceptor::handle_request()`

```
1. Generate unique ID (atomic u64 counter)
2. Log request if LOG_TRAFFIC_TERMINAL=1
3. duplicate_req(req):
   a. Collect full body into Bytes
   b. Build two identical requests:
      - origin: Body type (for forwarding to upstream)
      - duplicate: Bytes type (for listener callback)
4. check_interception() — re-evaluate rules against URI + host
5. listener.request(id, duplicate, should_intercept, client_addr).await
   → This calls the Swift callback via C FFI
6. Merge any modifications from listener back into origin
7. Return RequestOrResponse::Request(origin) for forwarding
```

#### `duplicate_req()` Detailed Flow

```rust
async fn duplicate_req(req: Request<Body>) -> RequestDuplicate {
    // Split request into parts + body
    let (parts, body) = req.into_parts();

    // Collect the entire body into memory (may decompress)
    let old_bytes = body.collect().await?.to_bytes();

    // Clone bytes for the duplicate
    let new_bytes = old_bytes.clone();

    // Build Bytes-typed request (for FFI / listener)
    let mut req1 = Request::builder()
        .uri(old_uri).method(old_method).version(*old_ver)
        .body(old_bytes).unwrap();
    // Copy headers
    for (k, v) in old_headers { req1.headers_mut().append(k, v); }

    // Rebuild Body-typed request (for forwarding)
    let req = Request::from_parts(parts, Body::from(Full::new(new_bytes)));

    RequestDuplicate { origin: req, duplicate: req1 }
}
```

### Phase 7: Response Interception

**File**: `src/traffic.rs` — `TrafficInterceptor::handle_response()`

```rust
async fn handle_response(&mut self, ctx: &HttpContext, res: Response<Body>) {
    // 1. Duplicate response (same pattern as request)
    let d = duplicate_res(res).await;

    // 2. Send duplicate to listener (Swift callback)
    let modified = listener.response(id, d.duplicate, intercepted, client_addr).await;

    // 3. Merge modifications
    //    (method, uri, version, headers from modified replace origin's)
    let (mut parts, _) = d.origin.into_parts();
    let (m_parts, m_body) = modified.into_parts();
    parts.status = m_parts.status;
    parts.version = m_parts.version;
    parts.headers = m_parts.headers;

    // 4. Return response for forwarding to client
    Response::from_parts(parts, Body::from(Full::new(m_body)))
}
```

### Phase 8: Swift Callback Delivery

When the listener receives the duplicated request/response, it crosses the C FFI boundary:

```
Rust side                          C FFI                          Swift side
  │                                │                                │
  ├─ TrafficInterceptor::          │                                │
  │  handle_request()              │                                │
  │  → listener.request(id,        │                                │
  │      duplicate, ...)           │                                │
  │       │                        │                                │
  │       │  Calls C callback      │                                │
  │       ├────────────────────────> req_callback(id, ptr)          │
  │                                │                                │
  │                                │  Calls Swift closure           │
  │                                ├───────────────────────────────>│
  │                                │                                │
  │                                │  RustTraffic.request(ptr) {    │
  │                                │    ip = req_body_context_ip()  │
  │                                │    req = HttpReq.from(ptr) {   │
  │                                │      uri = req_body_uri()      │
  │                                │      method = req_body_method()│
  │                                │      headers = req_body_headers│
  │                                │      body = req_body_write_body│
  │                                │    }                            │
  │                                │    httpTrafficListener?(traffic)│
  │                                │    req_body_free(ptr)           │
  │                                │  }                              │
  │                                │                                │
  │  Listener returns modified req │                                │
  │<────────────────────────────────────────────────────────────────│
  │                                │                                │
  │  Merge modifications           │                                │
  │  Forward to upstream           │                                │
```

## HTTP vs HTTPS vs WebSocket

| Traffic Type | Interception Path | Protocol Detection |
|---|---|---|
| **HTTP** | Direct forwarding (no CONNECT) | First byte is `G`, `P`, etc. |
| **HTTPS (intercepted)** | CONNECT → TLS detect → cert gen → serve_stream | First 2 bytes `\x16\x03` |
| **HTTPS (tunneled)** | CONNECT → raw TCP byte copy | `should_intercept` returns false |
| **WebSocket** | upgrade_websocket → split streams → forward messages | Upgrade header in HTTP request |

## The Rewind Mechanism

**File**: `hudsucker/src/rewind.rs`

When hudsucker peeks at the first 4 bytes of the upgraded connection to detect TLS, it must put those bytes back so they aren't lost. The `Rewind` wrapper does this:

```rust
let mut buffer = [0; 4];
let bytes_read = upgraded.read(&mut buffer).await?;

// Wrap the stream so the 4 bytes are replayed first
let mut upgraded = Rewind::new(upgraded_io, buffer, bytes_read);

// Now subsequent reads will see [buffer_bytes..][original_stream..]
```

## State Per Connection

Each connection carries important context through the pipeline:

| Context Field | Set When | Meaning |
|---|---|---|
| `client_addr` | Connection accept | `SocketAddr` of the client |
| `intercepted` | `serve_stream()` (true) or default (false) | Whether TLS was stripped |
| `request_id` | First `handle_request` call | Unique `u64` tracking a req/res pair |

## Key Files

| File | Role |
|---|---|
| `src/traffic.rs` | Core interception logic: duplication, rule matching, listener dispatch |
| `src/proxy.rs` | Proxy initialization, wires everything together |
| `hudsucker/src/proxy/internal.rs` | `process_connect`, `serve_stream`, `proxy` — the MITM engine |
| `hudsucker/src/proxy/mod.rs` | `Proxy::start()` — TCP accept loop |
| `hudsucker/src/rewind.rs` | Read-ahead buffer replay for protocol detection |
| `swift/Sources/ProxySwift/Proxy.swift` | Swift-side listener callbacks and data marshalling |
| `swift/Sources/ProxyRust/include/api.h` | C FFI function declarations |
