# On-The-Fly Certificate Generation

## The Problem

To perform HTTPS interception (MITM), the proxy must terminate the TLS connection from the client. The client's TLS handshake expects the target server's certificate, but the proxy doesn't have it. The proxy needs to dynamically generate a valid-looking certificate for *any* domain the client visits.

## The Solution

A **CA certificate** is pre-installed on the client device (or simulator). The proxy uses this CA to dynamically sign certificates for any domain on-the-fly. Since the client trusts the CA, it accepts these dynamically-generated certificates.

```
Client trust store               Proxy                     Real Server
  │                                │                          │
  │  [Trusts "Hudsucker CA"]       │                          │
  │                                │                          │
  │── CONNECT example.com:443 ────>│                          │
  │                                │                          │
  │         <──────────────────────│                          │
  │   TLS handshake with          │                          │
  │   "example.com" cert          │                          │
  │   signed by "Hudsucker CA"    │                          │
  │         ──────────────────────>                          │
  │                                │                          │
  │                                │── CONNECT example.com ──>│
  │                                │<── real TLS handshake ───│
```

## The CA Certificate Chain

The CA material lives in `src/ca/`:

```
src/ca/
├── hudsucker.cer    — PEM-encoded X.509 CA certificate
└── hudsucker.key    — PEM-encoded RSA private key
```

During development, these are the "Hudsucker Industries" CA pair. **Never use these in production** — generate your own.

## How Dynamic Certificate Generation Works

### Overview (`RcgenAuthority`)

The `RcgenAuthority` in `hudsucker/src/certificate_authority/rcgen_authority.rs` uses the `rcgen` crate to dynamically generate TLS certificates signed by the CA.

### Step-by-Step

#### Step 1: Parse the CA key pair (`src/proxy.rs`)

```rust
let key_pair = KeyPair::from_pem(self.key_pair)?;    // CA private key
let issuer = Issuer::from_ca_cert_pem(self.ca_cert, key_pair)?;  // CA cert + key
let ca = RcgenAuthority::new(issuer, 1_000, aws_lc_rs::default_provider());
```

- `KeyPair::from_pem` deserializes the RSA private key
- `Issuer::from_ca_cert_pem` combines the CA certificate with the key — this issuer will sign all dynamically generated certs
- `RcgenAuthority::new(cache_size, provider)` initializes the authority with:
  - An in-memory LRU cache (moka) for generated `ServerConfig`s
  - The TLS crypto provider (`aws_lc_rs`)

#### Step 2: Generate a certificate (`rcgen_authority.rs:gen_cert`)

When a client connects to a new domain, `gen_server_config` is called:

```rust
fn gen_cert(&self, authority: &Authority) -> CertificateDer<'static> {
    let mut params = CertificateParams::default();

    // Random serial number (prevents cert fingerprinting)
    params.serial_number = Some(rng().random::<u64>().into());

    // Valid from 60 seconds ago (clock skew tolerance)
    let not_before = OffsetDateTime::now_utc() - Duration::seconds(NOT_BEFORE_OFFSET);
    params.not_before = not_before;
    params.not_after = not_before + Duration::seconds(TTL_SECS);  // 1 year

    // CN = the domain being visited
    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::CommonName, authority.host());
    params.distinguished_name = distinguished_name;

    // SAN = the domain being visited (required by modern browsers)
    params.subject_alt_names.push(SanType::DnsName(
        Ia5String::try_from(authority.host())?,
    ));

    // Key usage for TLS server authentication
    params.key_usages = vec![DigitalSignature, KeyEncipherment];
    params.extended_key_usages = vec![ServerAuth];

    // Sign with the CA and return DER-encoded certificate
    params.signed_by(self.issuer.key(), &self.issuer)?.into()
}
```

The generated certificate contains:

| Field | Value | Purpose |
|---|---|---|
| Serial Number | Random `u64` | Unique per cert; prevents tracking |
| CN | `authority.host()` | e.g., `example.com` |
| SAN | `authority.host()` | Required by modern browsers/TLS |
| Issuer | CA subject | e.g., "Hudsucker Industries" |
| Not Before | 60s in past | Clock skew tolerance |
| Not After | 1 year from now | Long-lived but bounded |
| Key Usage | DigitalSignature, KeyEncipherment | Server auth |
| EKU | ServerAuth | Server authentication |

#### Step 3: Build a `ServerConfig` (`rcgen_authority.rs:gen_server_config`)

```rust
async fn gen_server_config(&self, authority: &Authority) -> Arc<ServerConfig> {
    // Check cache first
    if let Some(cached) = self.cache.get(authority).await {
        return cached;
    }

    // Generate certificate
    let certs = vec![self.gen_cert(authority)];

    // Build rustls ServerConfig
    let mut server_cfg = ServerConfig::builder_with_provider(Arc::clone(&self.provider))
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(certs, self.private_key.clone_key())?;

    // Advertise HTTP/1.1 and optionally HTTP/2
    server_cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    // Cache for reuse
    let server_cfg = Arc::new(server_cfg);
    self.cache.insert(authority.clone(), Arc::clone(&server_cfg)).await;

    server_cfg
}
```

### Caching

Generated certificates are cached in a `moka` LRU cache configured at init time:

| Parameter | Value | Meaning |
|---|---|---|
| `cache_size` | `1_000` (default) | Max 1000 unique domain certs cached |
| `TTL` | 182.5 days | Cached certs expire after half their validity |

If the same domain is visited again, the cached `Arc<ServerConfig>` is reused — no new cert generation required.

### The Interception Handshake (`internal.rs`)

When hudsucker's `process_connect` decides to intercept:

```rust
// Client sent TLS ClientHello (first 2 bytes = 0x16 0x03)
if buffer[..2] == *b"\x16\x03" {
    // Generate (or retrieve cached) ServerConfig for this domain
    let server_config = self.ca.gen_server_config(&authority).await;

    // Perform TLS handshake with the client using our dynamic cert
    let tls_stream = TlsAcceptor::from(server_config)
        .accept(upgraded)
        .await?;

    // Now traffic is decrypted — serve HTTP over this stream
    self.serve_stream(TokioIo::new(tls_stream), Scheme::HTTPS, authority).await?;
}
```

### Certificate Validation Chain

```
Client verifies:
  ┌──────────────────────────┐
  │ "example.com" cert       │  ← dynamically generated
  │   Signed by:             │
  │   "Hudsucker Industries" │
  └──────────┬───────────────┘
             │ signed by
             ▼
  ┌──────────────────────────┐
  │ "Hudsucker Industries"   │  ← CA cert (hudsucker.cer)
  │   Self-signed root       │      Must be trusted by client
  └──────────────────────────┘
```

### OpensslAuthority

The alternative implementation uses OpenSSL instead of `rcgen`:

```rust
// hudsucker/src/certificate_authority/openssl_authority.rs
pub struct OpensslAuthority {
    key: openssl::pkey::PKey<openssl::pkey::Private>,
    ca_cert: openssl::x509::X509,
    cache: Cache<Authority, Arc<ServerConfig>>,
    provider: Arc<CryptoProvider>,
}
```

It achieves the same result — on-the-fly certificate generation — but uses OpenSSL's X.509 APIs. It is enabled via the `openssl-ca` feature flag in hudsucker.

### Production Considerations

1. **Generate your own CA**: Never use the bundled `hudsucker.key` and `hudsucker.cer` in production.
   ```bash
   openssl req -x509 -newkey rsa:4096 -keyout my-ca-key.pem \
     -out my-ca-cert.pem -days 3650 -nodes \
     -subj "/CN=My Custom CA"
   ```

2. **Distribute the CA cert**: Install the CA certificate on each device that uses the proxy (Settings > Profile on iOS, Keychain on macOS).

3. **Cache sizing**: Adjust `cache_size` based on the number of unique domains your users visit.

4. **Cert validity**: The 1-year TTL is generous. Shorter lifetimes (e.g., 7 days) are more secure but regenerate more often.

## Key Files

| File | Role |
|---|---|
| `hudsucker/src/certificate_authority/mod.rs` | `CertificateAuthority` trait definition |
| `hudsucker/src/certificate_authority/rcgen_authority.rs` | `RcgenAuthority` — rcgen-based implementation |
| `hudsucker/src/certificate_authority/openssl_authority.rs` | `OpensslAuthority` — OpenSSL-based implementation |
| `hudsucker/src/proxy/internal.rs` | `process_connect` — where TLS interception happens |
| `src/ca/hudsucker.cer` | Development CA certificate |
| `src/ca/hudsucker.key` | Development CA private key |
| `src/proxy.rs` | Wires the CA into the proxy |
