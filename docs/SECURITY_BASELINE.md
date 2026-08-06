# Dungeon Barrage Security Baseline

**Status:** Binding engineering baseline
**Owner:** Backend / platform
**Related:** [PLATFORM_STRATEGY.md](./PLATFORM_STRATEGY.md) §14, [PROGRESSION.md](./PROGRESSION.md) §6, [adr/0001-rust-wasm-core.md](./adr/0001-rust-wasm-core.md)

Product-owner directive: *"This should not create ANY security vulnerabilities for the user, or
the host. This is the most important task."*

No document can guarantee zero vulnerabilities. What this baseline does is remove whole classes of
them by construction, make the remainder detectable, and make every control testable in CI. Where
a control is aspirational rather than implemented, it is marked **PLANNED**.

## 1. Scope note on SOC 2

SOC 2 is an **audited attestation of an organization's controls**, not a property of source code.
It requires a defined system boundary, documented policies, evidence collected over an observation
window (typically 3–12 months for Type II), and an independent CPA firm's report. It cannot be
achieved by writing code, and this project cannot be described as "SOC 2 compliant" until such an
audit is completed.

What engineering can do — and what this document commits to — is build so that an audit is
achievable rather than a rewrite. The controls below are organized by the Trust Services Criteria
they map to, so the evidence exists when it is needed.

| TSC | Criterion | Where addressed |
|---|---|---|
| CC6.1 | Logical access controls | §4 Identity, §5 Authorization |
| CC6.6 | Boundary protection | §3 Transport, §6 Input validation |
| CC6.7 | Data in transit | §3 Transport |
| CC6.8 | Malicious software prevention | §8 Supply chain |
| CC7.1 | Vulnerability detection | §8 Supply chain, §10 CI gates |
| CC7.2 | Monitoring and anomaly detection | §9 Audit and telemetry |
| CC7.3–7.4 | Incident response | §11 **PLANNED** |
| CC8.1 | Change management | §10 CI gates, §12 Release |
| A1.2 | Availability / backup | §12 **PLANNED** |
| C1.1–C1.2 | Confidentiality | §7 Data minimization |
| P-series | Privacy | §7 Data minimization |

Organizational controls an auditor will also require — security policy set, risk assessment,
vendor management, background checks, security training, formal access reviews — are **out of
engineering scope** and are the product owner's responsibility to establish. They are listed in
§13 so they are not discovered late.

## 2. Trust boundary

There is exactly one trust boundary, and the client is on the untrusted side of it.

```
UNTRUSTED                          │  TRUSTED
  browser / extension / desktop    │   match room process
  player input, aim, camera        │   HTTP profile API
  cosmetic selection               │   database
  ─ everything the player controls │   ─ everything that decides outcomes
```

The authoritative side owns: turn phase and timers, match seed and every random draw, legal
loadouts and definition versions, ammunition and durability, projectile paths, collision, terrain
occupancy, damage, knockback, status, elimination, match results, XP, currency, ownership, and
purchases.

The client owns: presentation, local settings, uncommitted aim, camera, cosmetic animation.

**Never accepted from a client, under any framing:** a hit, a damage number, a terrain change, an
ammunition count, a timer value, a level, an XP or currency total, an ownership assertion, a
purchase confirmation, another player's command, or a completed-match result.

## 3. Transport

- TLS 1.2+ for all HTTP; WSS for all realtime. No plaintext fallback, including in development
  against remote services.
- HSTS with a ≥1 year `max-age` and `includeSubDomains` once the domain is stable.
- Strict CSP on the web shell: no `unsafe-inline`, no `unsafe-eval`, explicit
  `connect-src`/`script-src` allowlists. WASM requires `wasm-unsafe-eval` in `script-src` — that
  directive permits WebAssembly compilation only and does **not** re-enable `eval`; it is added
  narrowly and never broadened to `unsafe-eval`.
- `frame-ancestors 'none'`, `X-Content-Type-Options: nosniff`, `Referrer-Policy:
  strict-origin-when-cross-origin`, `Permissions-Policy` denying unused features.
- CORS: explicit origin allowlist. Never `Access-Control-Allow-Origin: *` on any authenticated
  endpoint, and never reflect the `Origin` header.
- WebSocket upgrades validate `Origin` server-side. Browsers do not apply CORS to WebSockets, so
  origin checking is the server's job or the socket is cross-site-forgeable.

## 4. Identity and sessions

- Internal `playerId` is a server-generated opaque UUID. It is never a Steam ID, an email, a
  ChatGPT/workspace identity header, or any external subject. External identities map to it
  through provider records (`PLATFORM_STRATEGY.md` §8).
- Guest sessions are first-class and are the default entry path. No account is required to play.
- Session tokens: short-lived, audience-scoped, rotated on privilege change. Match tokens are
  scoped to a single `matchId` and expire with the match.
- Cookies carrying session material: `HttpOnly`, `Secure`, `SameSite=Lax` minimum
  (`Strict` where the flow permits).
- Token secrets and signing keys come from the environment or a managed secret store. A secret in
  the repository is a **build failure** (§10), not a code-review comment.

## 5. Authorization

Every authoritative operation re-derives authority server-side from the session. It never trusts
an ID in the request body.

Checked on every match command, in order: session validity → room membership → current phase →
turn ownership → state version → deadline → equipped slot and definition version → ammunition →
input ranges → status restrictions → `commandId` uniqueness.

A command failing any check is rejected with a categorized reason, counted, and cannot mutate
state. Out-of-turn and cross-player commands are rejected at the membership/ownership step and are
security events, not gameplay errors.

## 6. Input validation

- Every network message is schema-validated before use — shape, type, required fields, and
  **numeric bounds**. Unknown fields are rejected, not ignored, on authoritative paths.
- Hard caps on message size, message rate per connection, and connections per source, enforced
  before parsing. An oversized frame is dropped without allocation.
- All gameplay scalars are quantized integers at the protocol boundary (angle in millidegrees,
  power in basis points). No client-supplied floating point enters the simulation.
- The Rust core `forbid`s `unsafe_code` (ADR 0001), so a malformed command cannot produce memory
  corruption — the worst case is a rejected command or a caught panic.
- Panics at the WASM/FFI boundary are caught and converted to errors. A panic must never
  terminate a room process holding other players' matches.
- **No remote code execution surface by design:** weapon and cosmetic definitions are *data*
  referencing a fixed vocabulary of reviewed behavior identifiers. There is no scripting language,
  no `eval`, no dynamic import of downloaded code, and no user-uploaded assets
  (`PRODUCT_SPEC.md` §1 non-goals). A definition naming an unknown behavior fails validation at
  load time.
- Database access is exclusively parameterized (Drizzle query builder). Raw string-concatenated
  SQL is prohibited and grep-gated in CI.
- All player-authored text — display names, chat — is length-bounded, rejected on control
  characters, and rendered as text. React escapes by default; `dangerouslySetInnerHTML` is
  prohibited and grep-gated.

## 7. Data minimization

- Collect only what identity, moderation, progression, and operations require.
- Email and provider identifiers **never** appear in match payloads, event logs, or replays.
  Matches carry opaque player IDs only.
- Replays are reconstructed from authoritative commands and events — never client video, never
  screen capture.
- Logs are structured and scrubbed: no tokens, no secrets, no email addresses, no full IPs in
  application logs (truncate or hash for abuse correlation).
- Retention windows are defined per data class before public launch, with deletion honored on
  account closure. **PLANNED** — required before any public account launch.
- Privacy and retention disclosures published before public account or extension launch.

## 8. Supply chain

- `cargo deny` gates advisories, licenses, and duplicate versions. **CI gate.**
- `npm audit` at a defined severity threshold, plus lockfile integrity. **CI gate.**
- Lockfiles committed; installs use `npm ci` / `--locked`. No floating versions in production
  builds.
- New third-party dependencies require justification — a dependency in the authoritative path is
  a trust decision, not a convenience.
- Chrome MV3, if ever built, packages all executable code and WASM locally. Remote endpoints serve
  authenticated data and assets, never code (`PLATFORM_STRATEGY.md` §10).
- Electron, if ever built: `nodeIntegration: false`, `contextIsolation: true`, sandbox enabled,
  narrow allowlisted preload bridge, all bridge data validated.

## 9. Audit and telemetry

Security events are recorded on a channel **separate from** gameplay and chat events, with
append-only semantics:

- Authentication success/failure, session issuance and revocation.
- Authorization denials, including out-of-turn and cross-player command attempts.
- Rejected commands by category, with rate anomalies.
- State-hash mismatch between authoritative and expected.
- Economy: every ledger mutation, every idempotency-key replay hit, every admin adjustment with
  actor identity.
- Rate-limit and message-size violations.
- Administrative actions, always with actor, target, and reason.

Alerting focuses on: repeated hash mismatch, elevated rejected-command anomalies, economy
integrity faults (§6.2 of `PROGRESSION.md`), authentication brute-force patterns, reconnect
failure spikes, and durable-write failure.

## 10. CI gates

A change cannot merge unless all of these pass. These are the enforceable core of this document.

| Gate | Enforces |
|---|---|
| `cargo clippy -- -D warnings` | Lint cleanliness in the core |
| `forbid(unsafe_code)` present in `db-sim-core` | ADR 0001 memory-safety invariant |
| `cargo test` | Simulation correctness |
| TS↔Rust parity harness | ADR 0001 determinism contract |
| `cargo deny check` | Advisories, licenses, duplicate crates |
| `npm audit` threshold | JS dependency advisories |
| Secret scan | No credentials, keys, or tokens in the tree |
| Grep gate: no raw SQL concatenation | §6 injection |
| Grep gate: no `dangerouslySetInnerHTML`, no `eval` | §6 XSS/RCE |
| `tsc --noEmit` and ESLint | Type and lint correctness |
| Security-header assertion test | §3 headers actually served |

## 11. Incident response — **PLANNED**

Required before public launch: severity definitions, on-call and escalation path, communication
templates, a rehearsed containment procedure (revoke sessions, disable a mode, roll back content),
and post-incident review with tracked remediation.

## 12. Availability and change management — **PLANNED**

Required before public launch: automated database backups with **rehearsed restore** (an untested
backup is not a backup), deployment rollback procedure, content-version rollback that does not
reinterpret completed replays, room-draining deploys, and protocol/client version-range
negotiation with a clear update requirement for out-of-range clients.

## 13. Organizational controls — product-owner responsibility

Not engineering scope; listed so they are not discovered during an audit:

information security policy set · risk assessment and treatment · vendor/subprocessor management ·
personnel screening and security training · formal periodic access reviews · asset inventory ·
business continuity and disaster recovery plans · defined system boundary and control objectives ·
engagement of an independent CPA firm and an observation window.

## 14. Non-negotiables

1. The client never decides an outcome.
2. The authoritative core contains no `unsafe` and no floating point.
3. No secret is ever committed.
4. No remotely-delivered executable code, in any target.
5. Every economy mutation is idempotent and append-only.
6. No control in this document is weakened to make a deadline. It is weakened by an ADR, in the
   open, with the risk stated — or not at all.
