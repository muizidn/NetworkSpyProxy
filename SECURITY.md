# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

We take the security of NetworkSpyProxy seriously. If you believe you have found a security vulnerability, please **do not** open a public issue.

Instead, report it privately by emailing the project maintainer or opening a [GitHub Security Advisory](https://github.com/muizidn/NetworkSpyProxy/security/advisories/new).

### What to include

- Type of vulnerability
- Steps to reproduce
- Affected versions
- Any potential impact

### Response timeline

- **Acknowledgment**: within 48 hours
- **Initial assessment**: within 5 business days
- **Fix timeline**: communicated after assessment

## Security Considerations

### CA Certificate

The repository includes a development CA certificate and key at `src/ca/`. **Do not use these in production.** Generate your own CA:

```bash
openssl req -x509 -newkey rsa:4096 -keyout ca-key.pem -out ca-cert.pem \
  -days 365 -nodes -subj "/CN=My Custom CA"
```

### Traffic Interception

NetworkSpyProxy is a MITM proxy. Use it only:

- On networks you own or have explicit permission to monitor
- For debugging your own applications
- In compliance with applicable laws and regulations

### Data Handling

The proxy does not persist intercepted traffic to disk. The `TrafficListener` callback receives duplicated data in memory — ensure your listener implementation handles data securely and does not log sensitive information.

### Dependencies

Keep dependencies up to date. The project uses:

- **hudsucker** (MITM engine) — a vendored submodule; update it regularly
- **OpenSSL** — a vendored submodule; monitor for CVEs at https://openssl.org/news/vulnerabilities.html
- **Rust crates** — run `cargo audit` regularly to check for known vulnerabilities

## Disclosure Policy

- Vulnerabilities will be disclosed after a fix is released
- Contributors will be credited if they wish
- We follow responsible disclosure practices
