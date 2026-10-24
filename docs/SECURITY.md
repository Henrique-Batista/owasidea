# Security Model (v1.0)

## Threat model

- Plugins are untrusted WASM components.
- Host enforces declared host interfaces (capability broker).
- Manifests are generated from WIT; tampering is detected via checksum.
- WASI preopens limited to declared filesystem permissions.
- Network outbound limited to declared hosts.
- Fuel and memory limits enforced per instance.
- Request timeouts enforced.

## Not yet enforced

- WASI isolation completeness (WASI capabilities still broad).
- Cryptographic plugin signing (only sha256 integrity).
- Remote registry trust.
- Multi-tenant isolation.

## Recommendations

- Run host bound to localhost.
- Review manifests and permissions before distribution.
- Keep plugins bundled at build time.