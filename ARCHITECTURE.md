# Architecture

## Overview

NetworkSpyProxy is an HTTPS inspection (MITM) proxy designed for embedding in macOS/iOS applications. The system is composed of a Rust core that handles all proxy logic, and a Swift layer that provides a native Cocoa API via C FFI.

## Layers

```
┌──────────────────────────────────────────────────────────┐
│                    Application Layer                      │
│  (macOS/iOS app using NetworkSpyProxy framework)         │
├──────────────────────────────────────────────────────────┤
│                    Swift Layer                            │
│  Proxy.swift — High-level Swift API wrapping C FFI       │
│    ├── Proxy class (listen/unlisten lifecycle)           │
│    ├── HttpReq / HttpRes (traffic data models)           │
│    └── Traffic enum (callback event type)                │
├──────────────────────────────────────────────────────────┤
│                    C FFI Boundary                         │
│  api.h — C function declarations:                        │
│    proxy_new, proxy_listen, proxy_unlisten,              │
│    proxy_free, get_local_ip                              │
│  module.modulemap — Swift module map for C interop       │
├──────────────────────────────────────────────────────────┤
│                    Rust Core                              │
│  ┌──────────────────────────────────────────────────┐   │
│  │  network_spy_proxy crate                         │   │
│  │                                                  │   │
│  │  proxy.rs                                        │   │
│  │  ├── Proxy::new(key_pair, ca_cert, port)         │   │
│  │  ├── Proxy::run_proxy(listener, rules)           │   │
│  │  └── Proxy::stop_proxy()                         │   │
│  │                                                  │   │
│  │  traffic.rs                                      │   │
│  │  ├── TrafficListener trait                       │   │
│  │  │   ├── on_request(&self, &HttpReq)             │   │
│  │  │   └── on_response(&self, &HttpReq, &HttpRes)  │   │
│  │  ├── TrafficInterceptor                          │   │
│  │  │   └── Implements HttpHandler + WsHandler      │   │
│  │  ├── ProxyRule                                   │   │
│  │  │   ├── pattern: String (wildcard match)        │   │
│  │  │   ├── client: Option<String>                  │   │
│  │  │   └── action: ProxyAction (Intercept|Tunnel)  │   │
│  │  └── Request/Response duplication logic          │   │
│  │                                                  │   │
│  │  c/ip_addr.c                                     │   │
│  │  └── Get local non-loopback IPv4 address         │   │
│  └──────────────────────────────────────────────────┘   │
│                                                          │
│  ┌──────────────────────────────────────────────────┐   │
│  │  hudsucker (git submodule)                       │   │
│  │  MITM proxy engine                               │   │
│  │                                                  │   │
│  │  Proxy builder → Proxy                           │   │
│  │  ├── Binds TCP listener                          │   │
│  │  ├── Accepts connections                         │   │
│  │  ├── Spawns InternalProxy tasks                  │   │
│  │  └── Handles:                                    │   │
│  │      ├── HTTP forwarding                         │   │
│  │      ├── CONNECT tunneling                       │   │
│  │      ├── TLS interception                        │   │
│  │      ├── WebSocket upgrades                      │   │
│  │      └── Graceful shutdown                       │   │
│  │                                                  │   │
│  │  Certificate Authorities:                        │   │
│  │  ├── RcgenAuthority (rcgen crate)                │   │
│  │  │   └── On-the-fly TLS cert generation          │   │
│  │  └── OpensslAuthority (openssl crate)            │   │
│  │      └── On-the-fly TLS cert generation          │   │
│  │                                                  │   │
│  │  Body decoder                                    │   │
│  │  └── Decompression: gzip, deflate, br, zstd      │   │
│  └──────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────┘
```

## Data Flow

### HTTP Request Flow

```
Client                 NetworkSpyProxy              Target Server
  │                          │                          │
  │── HTTP CONNECT ─────────>│                          │
  │                          │─── CONNECT tunnel ──────>│
  │                          │<── 200 Connection OK ────│
  │<── 200 Connection OK ────│                          │
  │                          │                          │
  │── TLS handshake ────────>│                          │
  │  (mitm: proxy terminates │                          │
  │   and re-encrypts with   │                          │
  │   dynamic cert)          │                          │
  │                          │─── TLS handshake ───────>│
  │                          │<── cert ─────────────────│
  │                          │                          │
  │── HTTP request ─────────>│                          │
  │                          ├── TrafficInterceptor:    │
  │                          │  1. Match rules          │
  │                          │  2. Duplicate req        │
  │                          │  3. Notify listener      │
  │                          │  4. Forward req ────────>│
  │                          │<── HTTP response ────────│
  │                          ├── TrafficInterceptor:    │
  │                          │  1. Duplicate res        │
  │                          │  2. Notify listener      │
  │                          │  3. Forward res          │
  │<── HTTP response ────────│                          │
```

### Rule Matching Engine

```
Incoming Request
       │
       ▼
Extract: URI, Host, Client Process Name
       │
       ▼
Iterate ProxyRules (ordered)
       │
       ├── Match pattern with wildcard (*)?
       │   └── No → Next rule
       │
       ├── Client filter present?
       │   └── Yes → Match client process name?
       │       └── No → Next rule
       │
       └── Action:
           ├── INTERCEPT → Duplicate req/res, notify listener, forward
           └── TUNNEL    → Forward without inspection
```

## Build Architecture

```
┌──────────────┐    ┌──────────────────┐    ┌──────────────┐
│   Cargo      │    │     CMake        │    │    Bazel     │
│ (Rust crate) │    │ (C/C++ eco)      │    │ (Swift + FFI)│
│              │    │                  │    │              │
│ cargo build  │    │ cmake target     │    │ bazel build  │
│ cargo test   │    │   cbindgen_api   │    │ :NetworkSpy  │
│              │    │   build_lib      │    │   Proxy      │
│              │    │   build_openssl  │    │              │
└──────┬───────┘    └──────┬───────────┘    └──────┬───────┘
       │                   │                       │
       └───────────────────┼───────────────────────┘
                           ▼
              ┌──────────────────────┐
              │  libnetwork_spy_    │
              │  proxy.a            │
              │  (static library)    │
              └──────┬───────────────┘
                     │
                     ▼
              ┌──────────────────────┐
              │  Swift Framework     │
              │  (via C FFI/api.h)   │
              └──────────────────────┘
```

## Key Design Decisions

1. **Rust for performance & safety** — The proxy hot path (TLS termination, traffic forwarding, body decompression) runs in Rust with zero-cost abstractions.

2. **C FFI boundary** — A minimal C API isolates Swift from Rust ABI instability and enables Bazel/CMake integration without requiring Rust toolchain in the Swift build.

3. **Submodule-based hudsucker** — Pinning a specific hudsucker version as a submodule for reproducible builds and the ability to patch the MITM engine locally.

4. **Dual CA support** — Both `rcgen` and `openssl` certificate authority implementations are available. `rcgen` is used by default (lighter), while `openssl` provides compatibility with existing PKI infrastructure.

5. **Process-aware filtering** — `ProxyRule` optionally matches against client process name, enabling per-application traffic policies (e.g., intercept browser traffic, tunnel system updates).

6. **Request/Response duplication** — Rather than modifying traffic in-place, the interceptor clones each request/response, sends one copy to the listener callback and forwards the other. This prevents listener bugs from affecting proxy functionality.

## Dependencies

| Crate | Purpose |
|-------|---------|
| hudsucker | MITM proxy engine |
| tokio | Async runtime |
| hyper / http | HTTP protocol |
| rustls / tokio-rustls | TLS client/server |
| rcgen | On-the-fly cert generation |
| serde | Rule serialization |
| tracing | Structured logging |
| cc | C source compilation |
| OpenSSL | Vendor TLS library |

## Security Considerations

- The built-in CA key (`src/ca/hudsucker.key`) is for development only. Production deployments MUST generate and use their own CA certificate and key.
- Traffic interception happens at the proxy level; the Rust core does not persist or exfiltrate data.
- The Swift callback receives duplicated traffic data — the listener cannot block or modify the proxied request/response.
