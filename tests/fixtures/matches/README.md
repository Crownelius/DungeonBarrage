# Shared match fixtures

Each fixture directory contains exact UTF-8/LF JSON request and production-response bytes plus a
test-only manifest of semantic expectations. Direct Rust tests decode the same request files that C2
feeds unchanged through the C ABI and C3 will feed through `LocalMatchSession`.

Rules:

- Request files are one compact JSON object followed by exactly one LF, with no BOM or CR.
- Wire objects use the normative camel-case field names and explicit nullable fields from
  docs/CLIENT_SPEC.md.
- Production response files are compact JSON followed by exactly one LF and are compared
  byte-for-byte against the Rust serializer.
- fixture.json is test metadata, not a gameplay wire envelope.
- Hashes are frozen only after meaningful movement/projectile assertions pass.
- Response fixtures come only from the production C2 serializer; tests do not maintain a second,
  test-only wire implementation.
- A behavior change updates a frozen hash only in a separately explained change.
