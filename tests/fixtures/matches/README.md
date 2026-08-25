# Shared match fixtures

Each fixture directory contains exact UTF-8/LF JSON request bytes plus a test-only manifest
of semantic expectations. Direct Rust tests decode the same request files that C2 will feed
unchanged through the C ABI and C3 will feed through LocalMatchSession.

Rules:

- Request files are one compact JSON object followed by exactly one LF, with no BOM or CR.
- Wire objects use the normative camel-case field names and explicit nullable fields from
  docs/CLIENT_SPEC.md.
- fixture.json is test metadata, not a gameplay wire envelope.
- Hashes are frozen only after meaningful movement/projectile assertions pass.
- Full byte-for-byte expected response envelopes are added in C2, when the production Rust
  serializer exists. C1 does not fabricate wire bytes from a test-only serializer.
- A behavior change updates a frozen hash only in a separately explained change.
