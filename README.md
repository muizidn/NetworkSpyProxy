# NetworkSpyProxy

A Rust-based HTTPS inspection (MITM) proxy with Swift bindings for macOS/iOS applications. Enables real-time monitoring, interception, and modification of HTTP/HTTPS traffic with rule-based filtering.

## Features

- **HTTPS Interception** — Man-in-the-middle TLS decryption with on-the-fly certificate generation
- **Traffic Inspection** — Inspect full request/response bodies for HTTP and HTTPS traffic
- **WebSocket Support** — Intercept and monitor WebSocket messages
- **Rule-Based Filtering** — Configurable rules to INTERCEPT or TUNNEL traffic based on URI patterns and client process names
- **Swift Bindings** — Native Swift API via C FFI for embedding in macOS/iOS apps
- **Process-Aware Filtering** — Filter traffic by originating client process name

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                    Swift App                         │
│  ┌──────────────────────────────────────────────┐   │
│  │          ProxySwift (Proxy.swift)            │   │
│  └──────────┬───────────────────────────────────┘   │
│             │ C FFI (api.h)                         │
├─────────────┼───────────────────────────────────────┤
│  Rust Core  │                                       │
│  ┌──────────▼───────────────────────────────────┐   │
│  │        network_spy_proxy                     │   │
│  │  ┌───────────┐  ┌───────────────────────┐   │   │
│  │  │  proxy.rs │  │    traffic.rs         │   │   │
│  │  │ (Proxy)   │  │ TrafficInterceptor    │   │   │
│  │  │           │  │ TrafficListener trait │   │   │
│  │  └─────┬─────┘  │ ProxyRule             │   │   │
│  │        │        └───────────┬───────────┘   │   │
│  │  ┌─────▼────────────────────▼───────────┐   │   │
│  │  │          hudsucker (submodule)        │   │   │
│  │  │  MITM Proxy Engine                   │   │   │
│  │  │  ┌──────────┐ ┌──────────────────┐   │   │   │
│  │  │  │Rcgen CA  │ │ OpenSSL CA       │   │   │   │
│  │  │  └──────────┘ └──────────────────┘   │   │   │
│  │  └──────────────────────────────────────┘   │   │
│  └─────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────┘
```

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (edition 2021)
- OpenSSL (via git submodule)

### Build

```bash
# Clone with submodules
git clone --recursive https://github.com/yourusername/NetworkSpyProxy.git
cd NetworkSpyProxy

# Build the Rust library
cargo build

# Generate C API headers (for Swift)
make cbindgen_api
```

### CMake Build

```bash
cmake -B build
cmake --build build --target build_all
```

### Bazel Build

```bash
bazel build //:NetworkSpyProxy
```

## Usage

### Rust

```rust
use network_spy_proxy::proxy::Proxy;
use network_spy_proxy::traffic::{TrafficListener, ProxyRule, ProxyAction};
use std::sync::Arc;
use tokio::sync::RwLock;

struct MyTrafficListener;

impl TrafficListener for MyTrafficListener {
    fn on_request(&self, req: &HttpReq) {
        println!("Request: {} {}", req.method, req.uri);
    }
    fn on_response(&self, req: &HttpReq, res: &HttpRes) {
        println!("Response: {} {}", res.status, req.uri);
    }
}

#[tokio::main]
async fn main() {
    let ca_cert = include_str!("ca/hudsucker.cer");
    let ca_key = include_str!("ca/hudsucker.key");

    let mut proxy = Proxy::new(ca_key, ca_cert, 8080);
    let listener = Arc::new(MyTrafficListener);
    let rules = Arc::new(RwLock::new(vec![
        ProxyRule {
            pattern: "*.example.com".to_string(),
            client: None,
            action: ProxyAction::Intercept,
        }
    ]));

    proxy.run_proxy(listener, rules).await;
}
```

### Swift

```swift
import NetworkSpyProxy

let proxy = Proxy(
    keyPair: caKey,
    caCert: caCert,
    port: 8080
)

proxy.onTraffic = { traffic in
    switch traffic {
    case .request(let req):
        print("Request: \(req.method) \(req.uri)")
    case .response(let req, let res):
        print("Response: \(res.statusCode) \(req.uri)")
    }
}

try proxy.listen(in: rules)
// ...
proxy.unlisten()
```

## Configuration

### Proxy Rules

Rules define which traffic to intercept vs tunnel through:

| Field   | Type   | Description                          |
|---------|--------|--------------------------------------|
| pattern | String | URI pattern (supports `*` wildcards) |
| client  | String | Optional client process name filter  |
| action  | Enum   | `Intercept` or `Tunnel`              |

Examples:
- `"*.google.com"` → intercept all google subdomains
- `"*"` with `client: "curl"` → intercept all curl traffic

## Project Structure

```
NetworkSpyProxy/
├── src/                    # Rust core
│   ├── lib.rs              # Crate root
│   ├── proxy.rs            # Proxy lifecycle
│   ├── traffic.rs          # Traffic interception & rules
│   ├── c/                  # C helper sources
│   └── ca/                 # CA certificate & key
├── hudsucker/              # MITM proxy engine (submodule)
├── openssl/                # OpenSSL (submodule)
├── swift/                  # Swift bindings
│   ├── Sources/
│   │   ├── ProxyRust/      # C FFI bridging
│   │   └── ProxySwift/     # Native Swift API
│   └── Tests/
├── Cargo.toml              # Rust manifest
├── CMakeLists.txt          # CMake build
├── BUILD                   # Bazel build
└── cbindgen.toml           # C header generation
```

## Build Systems

- **Cargo** — Primary Rust build
- **CMake** — C/C++ ecosystem integration
- **Bazel** — Google-scale build system with Swift support

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
