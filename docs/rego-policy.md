# Embedded Rego policy

Navigator evaluates authorization policy in the `portal` process. The compiled policy is the decision point and
`require_policy` is the enforcement point: it receives request metadata plus the authenticated session, evaluates
`data.navigator.authz.allow`, and admits only a boolean `true`. Undefined, non-boolean, or evaluation-error results deny
the request.

Checked-in source: `portal/policy/navigator.rego`; companion tests: `portal/policy/navigator_test.rego`.
`PolicyClient::embedded` compiles that entrypoint once while the web process boots. A malformed policy prevents
readiness rather than creating a permissive runtime path. `PolicyClient::passthrough` is test-only and must be chosen
explicitly by a fixture.

## Rego authoring

The semantic contract — input, default deny, admin bypass, lawyer-tier writes, and project scoping — is canonical in
[`access-model.md`](access-model.md#how-embedded-rego-decides). Rego decides whether a request is allowed; Rust and the
database remain the source of truth for the data being protected.

Use one `allow` rule per intent and retain `default allow := false`. The `cli/tests/regorus_policy.rs` parity gate
compiles the production source with Regorus and runs all checked-in `test_*` rules. Run it with:

```bash
cargo test -p cli --test regorus_policy
```

This keeps the runtime interpreter and the policy tests in one Rust-native gate. Policy changes ship atomically with the
routes they name; there is no independent policy deployment, sidecar, endpoint, or environment variable.

## Rust integration

`portal/src/policy.rs` owns the small boundary:

- `PolicyClient::embedded` compiles the policy at boot.
- `require_policy` builds `input` from the request method, path, and session, then returns `403 Forbidden` unless the
  result is exactly `true`.
- The middleware is applied once at router boundaries, never reimplemented per handler.

## Related

- [`access-model.md`](access-model.md) — role and participation model plus rule semantics.
- [`oidc.md`](oidc.md) — how the session, including `role`, is populated at login.
- [Regorus](https://github.com/microsoft/regorus) — the Rust-native Rego interpreter used by Navigator.
