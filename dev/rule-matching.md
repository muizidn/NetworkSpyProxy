# Rule Matching Engine

## Overview

The rule matching engine (`check_interception` in `src/traffic.rs`) determines whether a given connection should be **intercepted** (decrypted and inspected) or **tunneled** (passed through without inspection). It is the core decision-making component of NetworkSpyProxy.

## ProxyRule Structure

```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProxyRule {
    pub pattern: String,        // URI/domain pattern (supports *)
    pub client: Option<String>, // Client process name filter
    pub action: String,         // "INTERCEPT" or "TUNNEL"
}
```

Rules are serializable (serde) and passed as a `Vec<ProxyRule>` wrapped in `Arc<RwLock<...>>` so they can be updated at runtime.

## Wildcard Matching Algorithm

The `wildcard_match` function implements glob-style pattern matching:

```rust
fn wildcard_match(pattern: &str, text: &str) -> bool {
    // No wildcard → substring match
    if !pattern.contains('*') {
        return text.contains(pattern);
    }

    // Split pattern on '*'
    let parts: Vec<&str> = pattern.split('*').collect();

    // Must start with the prefix before first '*'
    if !text.starts_with(parts[0]) {
        return false;
    }

    // "*" matches everything
    if pattern == "*" {
        return true;
    }

    // Sequential matching: each part must appear in order
    let mut current_pos = 0;
    for (i, part) in pattern.split('*').enumerate() {
        if part.is_empty() { continue; }
        if let Some(pos) = text[current_pos..].find(part) {
            // First part must match at position 0 (unless pattern starts with *)
            if i == 0 && pos != 0 && !pattern.starts_with('*') {
                return false;
            }
            current_pos += pos + part.len();
        } else {
            return false;
        }
    }

    // Must consume entire text, or pattern ends with *
    current_pos == text.len() || pattern.ends_with('*')
}
```

### Match Examples

| Pattern | Text | Result | Reason |
|---|---|---|---|
| `example.com` | `example.com` | Match | Substring match |
| `*.example.com` | `api.example.com` | Match | `*` matches `api` |
| `*.example.com` | `example.com` | No match | No prefix before first `*` |
| `*` | `anything.at.all` | Match | Universal wildcard |
| `google.*` | `google.com` | Match | Suffix wildcard |
| `api.*.com` | `api.github.com` | Match | Middle wildcard |
| `*secure*` | `https://secure.example.com` | Match | Wildcard at start |

## Decision Flow: `check_interception()`

```
check_interception(intercepted, uri, host, rules, listener, client_addr)
  │
  ├─ Step 1: Read rules under read lock
  │
  ├─ Step 2: If rules list is empty → return false (TUNNEL all)
  │
  ├─ Step 3: Iterate rules (first match wins)
  │    │
  │    ├─ 3a. Pattern matching — tries these targets:
  │    │    ├── raw uri (e.g., "https://api.example.com/path")
  │    │    ├── raw host (e.g., "api.example.com:443")
  │    │    └── cleaned (strip "://" prefix) uri & host
  │    │
  │    ├─ 3b. Client matching — if rule has client field:
  │    │    ├── "*" or empty → match all clients
  │    │    ├── specific name (e.g., "curl") → match via
  │    │    │   case-insensitive wildcard against client
  │    │    │   process name from listener.get_client_name()
  │    │    └── None (no client field) → always matches
  │    │
  │    ├─ 3c. Both pattern AND client must match → rule applies
  │    │
  │    └─ If match found:
  │         ├── action = "INTERCEPT" → check listener.should_intercept()
  │         │   ├── if true → return true (INTERCEPT)
  │         │   └── if false → return false (TUNNEL, listener vetoed)
  │         └── action = "TUNNEL" → return false
  │
  └─ Step 4: No rule matched → return false (TUNNEL by default)
```

### Key Logic

```rust
async fn check_interception(
    intercepted: bool,
    uri: &str,
    host: &str,
    proxy_intercept_list: &Arc<RwLock<Vec<ProxyRule>>>,
    listener: &Arc<dyn TrafficListener>,
    client_addr: &str,
    log_logic: bool,
) -> bool {
    let proxy_list_guard = proxy_intercept_list.read().await;
    let mut final_action = "TUNNEL".to_string();
    let mut matched = false;

    if proxy_list_guard.is_empty() {
        // Empty list → no rules → tunnel everything
        return false;
    }

    for rule in proxy_list_guard.iter() {
        // Pattern matching
        let mut pattern_match = false;
        if wildcard_match(&rule.pattern, uri)
            || wildcard_match(&rule.pattern, host)
        {
            pattern_match = true;
        }

        // Try cleaned targets (strip protocol prefix)
        if !pattern_match && !rule.pattern.contains("://") {
            let clean_uri = uri.trim_start_matches("https://")
                              .trim_start_matches("http://");
            let clean_host = host.trim_start_matches("https://")
                                .trim_start_matches("http://");
            if wildcard_match(&rule.pattern, clean_uri)
                || wildcard_match(&rule.pattern, clean_host)
            {
                pattern_match = true;
            }
        }

        // Client matching
        let mut client_match = false;
        if let Some(client_pattern) = &rule.client {
            if client_pattern.trim().is_empty() || client_pattern == "*" {
                client_match = true;  // No client constraint
            } else {
                let client_name = listener.get_client_name(client_addr).await;
                if wildcard_match(&client_pattern.to_lowercase(),
                                  &client_name.to_lowercase()) {
                    client_match = true;
                }
            }
        } else {
            client_match = true;  // Rule has no client field
        }

        // Both must match
        if pattern_match && client_match {
            final_action = rule.action.clone();
            matched = true;
            break;  // First match wins
        }
    }

    // Action decision
    if matched && final_action == "INTERCEPT" {
        // Listener can still veto
        return listener.should_intercept(uri, host, client_addr).await;
    }

    false  // Default: tunnel
}
```

## Rule Examples

### Intercept Everything

```json
[
  { "pattern": "*", "client": null, "action": "INTERCEPT" }
]
```

### Intercept Specific Domains

```json
[
  { "pattern": "*.google.com", "client": null, "action": "INTERCEPT" },
  { "pattern": "*.facebook.com", "client": null, "action": "INTERCEPT" }
]
```

### Intercept by Client Process

```json
[
  { "pattern": "*", "client": "curl", "action": "INTERCEPT" },
  { "pattern": "*", "client": "Safari", "action": "INTERCEPT" }
]
```

### Mixed Rules (First Match Wins)

```json
[
  { "pattern": "*.bank.com", "client": null, "action": "TUNNEL" },
  { "pattern": "*", "client": null, "action": "INTERCEPT" }
]
```

Bank traffic is tunneled (not inspected), everything else is intercepted.

## The Two-Stage Decision

The interception decision actually happens **twice** per connection:

### Stage 1: `should_intercept()` — CONNECT Layer

Called by hudsucker's `process_connect()` to decide whether to MITM the TLS connection:

```rust
// In TrafficInterceptor (proxy.rs → traffic.rs → hudsucker internal.rs)
fn should_intercept(&mut self, ctx: &HttpContext, req: &Request<Body>)
    -> impl Future<Output = bool>
{
    // Runs check_interception() against the CONNECT host
    // Decides: TLS intercept or raw TCP tunnel
}
```

### Stage 2: `handle_request()` — HTTP Layer

Called for each HTTP request within an intercepted connection:

```rust
fn handle_request(&mut self, ctx: &HttpContext, req: Request<Body>)
    -> impl Future<Output = RequestOrResponse>
{
    // Even inside an intercepted connection, re-evaluate:
    // Should this specific request be reported to the listener?
    // The connection is already MITM'd, but we can still skip
    // the callback for specific URIs.
}
```

This two-stage design means:
- Stage 1 decides **if we decrypt** (heavyweight — TLS termination)
- Stage 2 decides **if we notify** the listener (lightweight — just a callback)

## Environment Variables for Debugging

| Variable | Effect |
|---|---|
| `LOG_TRAFFIC_TERMINAL=1` | Log all requests/responses to stdout |
| `PROXY_INTERCEPTION_LOGIC_LOG=1` | Log detailed rule matching decisions |

## Key Files

| File | Role |
|---|---|
| `src/traffic.rs` | `ProxyRule`, `wildcard_match`, `check_interception` |
| `hudsucker/src/proxy/internal.rs` | `process_connect` — calls `should_intercept` |
