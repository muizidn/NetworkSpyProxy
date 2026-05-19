# Hudsucker's Role in NetworkSpyProxy

## What is Hudsucker?

Hudsucker is a MITM (Man-in-the-Middle) HTTP/S proxy library written in Rust. It is included as a vendored git submodule at `hudsucker/` (fork at `github.com/muizidn/hudsucker`). It provides the core proxy engine — TCP listener, TLS interception, HTTP forwarding, and WebSocket proxying — while NetworkSpyProxy layers traffic inspection logic on top.

## Hudsucker's Components

```
┌─────────────────────────────────────────────────┐
│                 Hudsucker                         │
│                                                   │
│  ┌───────────────────────────────────────────┐   │
│  │  Proxy struct (proxy/mod.rs)              │   │
│  │  └── TCP listener loop                    │   │
│  │  └── Connection accept & dispatch         │   │
│  ├───────────────────────────────────────────┤   │
│  │  InternalProxy (proxy/internal.rs)        │   │
│  │  └── HTTP forwarding                      │   │
│  │  └── CONNECT tunneling                    │   │
│  │  └── TLS interception (MITM)              │   │
│  │  └── WebSocket upgrading & forwarding     │   │
│  ├───────────────────────────────────────────┤   │
│  │  CertificateAuthority (trait)             │   │
│  │  └── RcgenAuthority (rcgen-based)         │   │
│  │  └── OpensslAuthority (OpenSSL-based)     │   │
│  ├───────────────────────────────────────────┤   │
│  │  HttpHandler (trait)                      │   │
│  │  WebSocketHandler (trait)                 │   │
│  ├───────────────────────────────────────────┤   │
│  │  Body (body.rs) — custom body type        │   │
│  │  Decoder (decoder.rs) — gzip/deflate/br   │   │
│  └───────────────────────────────────────────┘   │
└─────────────────────────────────────────────────┘
```

## How NetworkSpyProxy Uses Hudsucker

### 1. Proxy Initialization (`src/proxy.rs`)

```rust
// 1. Parse the CA key pair from PEM
let key_pair = KeyPair::from_pem(self.key_pair)?;

// 2. Create an Issuer from the CA cert + key
let issuer = Issuer::from_ca_cert_pem(self.ca_cert, key_pair)?;

// 3. Build the Certificate Authority (rcgen-based)
let ca = RcgenAuthority::new(issuer, 1_000, aws_lc_rs::default_provider());

// 4. Create the traffic interceptor (our custom handler)
let traffic = TrafficInterceptor::new(listener, allow_list);

// 5. Build and start the hudsucker Proxy
let proxy = hudsucker::Proxy::builder()
    .with_addr(SocketAddr::from(([127, 0, 0, 1], self.port)))
    .with_ca(ca)
    .with_rustls_connector(aws_lc_rs::default_provider())
    .with_http_handler(traffic.clone())
    .with_websocket_handler(traffic.clone())
    .build()?;

proxy.start().await?;
```

The `Proxy` is constructed using hudsucker's type-state builder:

| Builder Step | What it configures |
|---|---|
| `with_addr` | Bind address (`127.0.0.1:port`) |
| `with_ca` | Certificate authority for dynamic TLS certs |
| `with_rustls_connector` | TLS client connector for upstream connections |
| `with_http_handler` | Our `TrafficInterceptor` (implements `HttpHandler`) |
| `with_websocket_handler` | Our `TrafficInterceptor` (implements `WebSocketHandler`) |

### 2. Connection Lifecycle (hudsucker's `Proxy::start`)

```
Proxy::start()
  │
  ├─ loop: listener.accept()
  │    │
  │    ├─ spawn task per connection
  │    │    │
  │    │    └─ hyper server::auto connection
  │    │         │
  │    │         └─ service_fn → InternalProxy::proxy()
  │    │              │
  │    │              ├─ HttpHandler::handle_request(request)
  │    │              │    │
  │    │              │    ├─ If CONNECT → process_connect()
  │    │              │    ├─ If WebSocket upgrade → upgrade_websocket()
  │    │              │    └─ Else → forward HTTP, then handle_response()
  │    │              │
  │    │              └─ HttpHandler::handle_response(response)
  │    │
  │    └─ ... next connection
```

### 3. The Two Traits NetworkSpyProxy Implements

#### `HttpHandler`

NetworkSpyProxy's `TrafficInterceptor` implements three methods:

```rust
impl HttpHandler for TrafficInterceptor {
    // Called for every HTTP request.
    // Can return either a modified Request or a Response (to short-circuit).
    fn handle_request(&mut self, ctx: &HttpContext, req: Request<Body>)
        -> impl Future<Output = RequestOrResponse>;

    // Called for every HTTP response.
    fn handle_response(&mut self, ctx: &HttpContext, res: Response<Body>)
        -> impl Future<Output = Response<Body>>;

    // Called to decide if a CONNECT tunnel should be intercepted (MITM'd).
    fn should_intercept(&mut self, ctx: &HttpContext, req: &Request<Body>)
        -> impl Future<Output = bool>;
}
```

#### `WebSocketHandler`

```rust
impl WebSocketHandler for TrafficInterceptor {
    // Called for each WebSocket message in both directions.
    fn handle_message(&mut self, ctx: &WebSocketContext, msg: Message)
        -> impl Future<Output = Option<Message>>;
}
```

### 4. Connection Routing in `InternalProxy::proxy()`

```rust
pub(crate) async fn proxy(mut self, req: Request<Incoming>)
    -> Result<Response<Body>, Infallible>
{
    let ctx = self.context();

    // Step 1: Let the handler inspect/modify the request
    let req = match self.http_handler.handle_request(&ctx, req.map(Body::from)).await {
        RequestOrResponse::Request(req) => req,
        RequestOrResponse::Response(res) => return Ok(res),  // Short-circuit
    };

    // Step 2: Route based on method
    if req.method() == Method::CONNECT {
        Ok(self.process_connect(req))          // Tunnel or MITM
    } else if hyper_tungstenite::is_upgrade_request(&req) {
        Ok(self.upgrade_websocket(req))        // WebSocket
    } else {
        // Regular HTTP: forward to upstream
        let res = self.client.request(normalize_request(req)).await;
        match res {
            Ok(res) => Ok(self.http_handler.handle_response(&ctx, res.map(Body::from)).await),
            Err(err) => Ok(self.http_handler.handle_error(&ctx, err).await),
        }
    }
}
```

### 5. CONNECT Processing — The MITM Decision

When hudsucker receives a `CONNECT` request, `process_connect` is called:

```rust
fn process_connect(mut self, mut req: Request<Body>) -> Response<Body> {
    // 1. Extract the target authority (host:port)
    let authority = req.uri().authority().cloned()?;

    // 2. Upgrade the connection (perform HTTP CONNECT handshake)
    let upgraded = hyper::upgrade::on(&mut req).await?;

    // 3. Peek at the first 4 bytes to determine protocol
    let mut buffer = [0; 4];
    upgraded.read(&mut buffer).await?;
    let mut upgraded = Rewind::new(upgraded_io, buffer, bytes_read);

    // 4. Ask the handler: should we intercept?
    if self.http_handler.should_intercept(&self.context(), &req).await {
        if buffer == *b"GET " {
            // Plain HTTP over CONNECT (WebSocket) → serve without TLS
            self.serve_stream(stream, Scheme::HTTP, authority).await?;
        } else if buffer[..2] == *b"\x16\x03" {
            // TLS ClientHello → MITM with dynamic cert
            let server_config = self.ca.gen_server_config(&authority).await;
            let tls_stream = TlsAcceptor::from(server_config).accept(upgraded).await?;
            self.serve_stream(tls_stream, Scheme::HTTPS, authority).await?;
        }
    } else {
        // Tunnel mode: raw TCP byte copy
        let mut server = TcpStream::connect(authority.as_ref()).await?;
        tokio::io::copy_bidirectional(&mut upgraded, &mut server).await?;
    }
}
```

### 6. How NetworkSpyProxy Extends Hudsucker

| Aspect | Hudsucker provides | NetworkSpyProxy adds |
|---|---|---|
| Request handling | `HttpHandler` trait | `TrafficInterceptor` with duplication + listener callback |
| Interception decision | `should_intercept` returning bool | Rule-based matching (`ProxyRule`) with wildcard + client filtering |
| Traffic observation | Handler can modify traffic | Duplication: one copy to listener, one forwarded |
| WebSocket support | `WebSocketHandler` trait | Logging pass-through |
| Certificate generation | `RcgenAuthority` / `OpensslAuthority` | Pre-loaded CA cert + key from `src/ca/` |
| Language binding | N/A | C FFI + Swift wrapper |

## Key Files

| File | Role |
|---|---|
| `hudsucker/src/proxy/mod.rs` | `Proxy` struct, TCP accept loop, `start()` |
| `hudsucker/src/proxy/internal.rs` | `InternalProxy`, CONNECT/MITM/tunnel logic |
| `hudsucker/src/proxy/builder.rs` | Type-state `ProxyBuilder` |
| `hudsucker/src/certificate_authority/mod.rs` | `CertificateAuthority` trait |
| `hudsucker/src/certificate_authority/rcgen_authority.rs` | `RcgenAuthority` implementation |
| `hudsucker/src/lib.rs` | Re-exports, `HttpHandler` + `WebSocketHandler` traits |
| `hudsucker/src/body.rs` | Custom `Body` type |
| `src/proxy.rs` | NetworkSpyProxy's `Proxy` wrapper — wires hudsucker |
| `src/traffic.rs` | `TrafficInterceptor` — implements hudsucker's traits |
