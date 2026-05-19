# FFI Boundary: Rust → C → Swift

## Overview

NetworkSpyProxy exposes its Rust core to Swift through a C FFI (Foreign Function Interface). This allows macOS/iOS applications to use the proxy as a native Swift framework without requiring the Rust toolchain.

```
┌──────────────────────────────────────────────┐
│              Swift Application                │
│  ┌────────────────────────────────────────┐  │
│  │  ProxySwift (Proxy.swift)              │  │
│  │  - Proxy class (lifecycle)             │  │
│  │  - HttpReq / HttpRes (data models)     │  │
│  │  - Traffic enum (callback events)      │  │
│  └──────────────┬─────────────────────────┘  │
│                 │ Swift calls                 │
│  ┌──────────────▼─────────────────────────┐  │
│  │  ProxyRust (C module map)              │  │
│  │  - module.modulemap                     │  │
│  │  - api.h (C declarations)              │  │
│  │  - dummy.c (placeholder)               │  │
│  └──────────────┬─────────────────────────┘  │
│                 │ C function calls            │
│  ┌──────────────▼─────────────────────────┐  │
│  │  Rust static library                   │  │
│  │  libnetwork_spy_proxy.a                │  │
│  │  - Compiled with cargo                 │  │
│  │  - Exports C-compatible functions      │  │
│  └─────────────────────────────────────────┘  │
└──────────────────────────────────────────────┘
```

## The C API Header (`api.h`)

**File**: `swift/Sources/ProxyRust/include/api.h`

The header defines the entire interface between Rust and Swift:

```c
#include <stdint.h>
#include <stdlib.h>

// Opaque types — Swift never sees the internals
typedef struct Proxy Proxy;
typedef struct ReqBody ReqBody;
typedef struct ResBody ResBody;

// Proxy configuration
typedef struct ProxyArg {
    uint8_t ip_v4_addr[4];  // IPv4 address bytes
    uint16_t port;          // Listen port
} ProxyArg;

// Proxy lifecycle
struct Proxy* proxy_new(struct ProxyArg arg);
void proxy_listen(struct Proxy* ptr,
    void (**req_callback)(uint8_t, struct ReqBody*),
    void (**res_callback)(uint8_t, struct ResBody*),
    uint8_t id);
void proxy_unlisten(struct Proxy* ptr, uint8_t id);
void proxy_free(struct Proxy* ptr);

// Request body inspection
char* req_body_http_uri(struct ReqBody* ptr);
char* req_body_http_method(struct ReqBody* ptr);
char* req_body_http_headers(struct ReqBody* ptr);
char* req_body_http_version(struct ReqBody* ptr);
uintptr_t req_body_http_body_len(struct ReqBody* ptr);
void req_body_http_write_body(struct ReqBody* ptr, uint8_t* data);
char* req_body_http_context_ip(struct ReqBody* ptr);
uint16_t req_body_http_context_port(struct ReqBody* ptr);
void req_body_free(struct ReqBody* ptr);

// Response body inspection
uint16_t res_body_http_status(struct ResBody* ptr);
char* res_body_http_version(struct ResBody* ptr);
char* res_body_http_headers(struct ResBody* ptr);
uintptr_t res_body_http_body_len(struct ResBody* ptr);
void res_body_http_write_body(struct ResBody* ptr, uint8_t* data);
char* res_body_http_context_ip(struct ResBody* ptr);
uint16_t res_body_http_context_port(struct ResBody* ptr);
void res_body_free(struct ResBody* ptr);

// Utility
char* get_ip_address(void);
void get_ip_address_free(char* ptr);
```

## Module Map

**File**: `swift/Sources/ProxyRust/include/module.modulemap`

```objc
module ProxyRust {
    header "api.h"
    export *
}
```

This tells Swift's Clang importer to expose all declarations from `api.h` as the `ProxyRust` module. Swift imports it with:

```swift
import ProxyRust
```

## Data Flow Across the Boundary

### Proxy Lifecycle

```
Swift                              C FFI                              Rust
  │                                │                                │
  │ proxy_new(arg) ───────────────>│                                │
  │                                │  ┌─ allocate Proxy struct      │
  │                                │  ├─ store port                 │
  │                                │  └─ return ptr                 │
  │<── OpaquePointer ──────────────│                                │
  │                                │                                │
  │ proxy_listen(ptr,              │                                │
  │   req_callback,                │  ┌─ start tokio runtime        │
  │   res_callback,                │  ├─ spawn proxy task           │
  │   id) ────────────────────────>│  ├─ store callbacks            │
  │                                │  └─ return                     │
  │                                │                                │
  │ ... traffic flows ...          │                                │
  │                                │                                │
  │                                │  Traffic detected:             │
  │                                │  ┌─ build ReqBody/ResBody      │
  │                                │  ├─ call req_callback(id, ptr) │
  │  req_callback(id, ptr) ───────│──│──────────────────────────────│
  │  │                            │                                │
  │  ├─ HttpReq.from(ptr)         │                                │
  │  │  ├─ req_body_http_uri() ───│──│─────────────────────────────>│
  │  │  │                         │  │  return char*               │
  │  │  │<── char* ──────────────│──│──────────────────────────────│
  │  │  ├─ req_body_http_method() │                                │
  │  │  ├─ req_body_http_headers()│                                │
  │  │  ├─ req_body_http_body_len()│                               │
  │  │  ├─ req_body_http_write_body()│                             │
  │  │  └─ req_body_free(ptr) ───│──│─────────────────────────────>│
  │  └─ httpTrafficListener?()   │    free ReqBody                  │
  │                                │                                │
  │ proxy_unlisten(ptr, id) ──────>│  ┌─ signal shutdown            │
  │                                │  └─ return                     │
  │                                │                                │
  │ proxy_free(ptr) ──────────────>│  ┌─ deallocate Proxy           │
  │                                │  └─ return                     │
```

### Marshallers: String Return Values

Strings returned from Rust are heap-allocated C strings (`char*`). Each string has a corresponding `_free` function to release the memory:

```c
char* req_body_http_uri(struct ReqBody* ptr);
void req_body_http_uri_free(char* ptr);  // Must call after use
```

In Swift:

```swift
let uri = req_body_http_uri(ptr)!       // Get C string
defer { req_body_http_uri_free(uri) }   // Ensure cleanup
let url = String(cString: uri)           // Convert to Swift String
```

### Marshallers: Body Data

The body is transferred in two steps because it's binary data (not null-terminated):

```c
uintptr_t req_body_http_body_len(struct ReqBody* ptr);
void req_body_http_write_body(struct ReqBody* ptr, uint8_t* data);
```

Swift side:

```swift
let bodySize = req_body_http_body_len(ptr)
let bodyMut = UnsafeMutablePointer<UInt8>.allocate(capacity: Int(bodySize))
defer { bodyMut.deallocate() }
req_body_http_write_body(ptr, bodyMut)
let body = Data(bytes: UnsafeRawPointer(bodyMut), count: Int(bodySize))
```

### Marshallers: Headers

Headers are transferred as a single `\r\n`-separated string:

```c
// Returns "Content-Type: text/html\r\nCache-Control: no-cache\r\n"
char* req_body_http_headers(struct ReqBody* ptr);
```

Swift parsing:

```swift
let headersStr = String(cString: req_body_http_headers(ptr))
let headers: [HeaderPair] = headersStr
    .split(separator: "\r\n")
    .map { pair in
        let p = pair.split(separator: ":")
        let key = String(p.first ?? "").trimmingCharacters(in: .whitespaces)
        let value = String(p.count == 2 ? p[1] : "").trimmingCharacters(in: .whitespaces)
        return HeaderPair((key, value))
    }
```

## Callback Mechanism

### Rust Side (conceptual — in `traffic.rs`)

The `TrafficInterceptor` holds a C function pointer for the callback:

```rust
pub struct TrafficInterceptor {
    listener: Arc<dyn TrafficListener>,
    // ...
}
```

When the `TrafficListener` receives a request/response, it calls through the C FFI:

```rust
// The trait implementation calls C function pointers
// stored in the inner listener
let req_callback: extern "C" fn(u8, *const ReqBody) = ...;
let ptr: *const ReqBody = ...;
req_callback(id, ptr);
```

### Swift Side

```swift
private typealias RustCallback = @convention(c) (UInt8, OpaquePointer?) -> Void

// Set up callbacks
var req_callback: RustCallback? = { id, reqPtr in
    guard let ctx = Proxy.context[id] else { return }
    RustTraffic.request(ptr: reqPtr!) { ip, traffic in
        ctx.queue.async {
            ctx.proxy.httpTrafficListener?(traffic)
            ctx.proxy.clientListListener?(ip)
        }
    }
}

// Pass to Rust
proxy_listen(self.proxy, &req_callback, &res_callback, self.currentId)
```

The `@convention(c)` attribute ensures the closure uses the C ABI so Rust can call it directly.

## Threading Model

```
┌──────────────────────────────────────────────────┐
│                   Swift App (Main Thread)         │
│  - UI updates                                     │
│  - httpTrafficListener callback (dispatch to      │
│    user-specified DispatchQueue)                  │
├──────────────────────────────────────────────────┤
│       DispatchQueue.global(qos: .background)      │
│  - proxy_listen() — blocking Rust call            │
│  - Runs the tokio runtime                         │
│  - Calls Rust callbacks from async tasks          │
├──────────────────────────────────────────────────┤
│                Rust (tokio runtime)               │
│  - Async I/O (hyper, tokio)                       │
│  - TLS handshakes                                 │
│  - TrafficInterceptor handlers                    │
│  - C callback invocation                          │
└──────────────────────────────────────────────────┘
```

The `listen(in:)` method spawns the Rust runtime on a background global queue to avoid blocking the main thread. Callbacks are dispatched to a user-specified `DispatchQueue` for thread-safe handling.

## Build Integration

### Swift Package (`swift/Package.swift`)

```swift
// macro to import as system library
targets: [
    .systemLibrary(
        name: "ProxyRust",
        path: "Sources/ProxyRust",
        pkgConfig: "network_spy_proxy"
    ),
    .target(
        name: "ProxySwift",
        dependencies: ["ProxyRust"]
    ),
]
```

### Bazel (`BUILD`)

```python
cc_import(
    name = "cc",
    hdrs = ["include/api.h"],
    static_library = ":create_staticlib_with_cargo"
)

swift_c_module(
    name = "NetworkSpyProxyRust",
    deps = [":cc"],
    module_map = "include/module.modulemap",
    module_name = "NetworkSpyProxyRust"
)

swift_library(
    name = "NetworkSpyProxy",
    srcs = glob(["swift/Source/*.swift"]),
    deps = [":NetworkSpyProxyRust"]
)
```

### CMake

```cmake
add_custom_target(cbindgen_api
    COMMAND cbindgen --config cbindgen.toml --crate network_spy_proxy
            --lang c --output include/api.h
)

add_custom_target(build_lib_release
    COMMAND cargo build --release --target aarch64-apple-darwin
)
```

## Memory Management Rules

| Object | Allocated by | Freed by | Function |
|---|---|---|---|
| `Proxy` | Rust (`proxy_new`) | Rust (`proxy_free`) | `proxy_free(ptr)` |
| `ReqBody` | Rust (interceptor) | Swift (`req_body_free`) | `req_body_free(ptr)` |
| `ResBody` | Rust (interceptor) | Swift (`res_body_free`) | `res_body_free(ptr)` |
| URI string | Rust | Swift | `req_body_http_uri_free(ptr)` |
| Method string | Rust | Swift | `req_body_http_method_free(ptr)` |
| Headers string | Rust | Swift | `req_body_http_headers_free(ptr)` |
| Version string | Rust | Swift | `req_body_http_version_free(ptr)` |
| Body data | Swift (buffer) | Swift | `bodyMut.deallocate()` |
| IP string | Rust | Swift | `req_body_http_context_ip_free(ptr)` |

**Rule**: Memory allocated by Rust must be freed by Rust's `_free` function. Memory allocated by Swift must be freed by Swift.

## Key Files

| File | Role |
|---|---|
| `swift/Sources/ProxyRust/include/api.h` | C API declarations |
| `swift/Sources/ProxyRust/include/module.modulemap` | Swift module map |
| `swift/Sources/ProxyRust/dummy.c` | Placeholder for SwiftPM target |
| `swift/Sources/ProxySwift/Proxy.swift` | Swift wrapper class |
| `swift/Package.swift` | Swift Package manifest |
| `BUILD` | Bazel build with Swift + C rules |
| `CMakeLists.txt` | CMake `cbindgen_api` target for header generation |
| `cbindgen.toml` | cbindgen config (empty — API is manually maintained) |
