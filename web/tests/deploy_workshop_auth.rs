//! Grounding test for the "Sign-in" section of the deploy workshop.
//!
//! `web/content/workshops/navigator/DEPLOY.md` now teaches the auth
//! stance: Navigator delegates identity to an OIDC-compatible provider
//! (Keycloak / Auth0 / Okta / GCP Identity Platform) and **never stores
//! a password**. That prose is a public promise, so nothing stops it
//! drifting from reality — a password column added to `persons`, a
//! hashing crate pulled into the graph, a renamed env var, a discovery
//! mechanism the code no longer uses.
//!
//! These tests pin the section to the code the same way
//! `third_party_catalog.rs` pins the vendor table and
//! `cli`'s `devx::gcp::deploy_workshop_prose_matches_the_dry_run_pipeline` pins
//! the provisioning steps: every claim the workshop makes about sign-in
//! must be true of the code that ships in this commit.

use std::path::Path;

/// Read a repo-root file relative to this crate (`web/` → workspace root
/// is one level up), matching the convention `third_party_catalog.rs`
/// and the docs loader use.
fn repo_file(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {} — {e}", path.display()))
}

/// The body of the workshop's `## Sign-in:` section — everything from
/// that heading to the next `##`.
fn signin_section() -> String {
    let deploy = repo_file("web/content/workshops/navigator/DEPLOY.md");
    let after = deploy
        .split_once("## Sign-in")
        .expect("DEPLOY.md must carry a `## Sign-in` section")
        .1;
    // Stop at the next top-level section heading.
    match after.split_once("\n## ") {
        Some((body, _)) => body.to_string(),
        None => after.to_string(),
    }
}

#[test]
fn every_oauth_env_var_the_workshop_names_exists_in_env_example() {
    // Each `OAUTH_*` token the prose prints must be a real key in the
    // committed env contract — so the four-variable wiring the workshop
    // teaches can't drift from `.env.example`.
    let section = signin_section();
    let env_example = repo_file(".env.example");

    let mut named: Vec<&str> = section
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|t| t.starts_with("OAUTH_"))
        .collect();
    named.sort_unstable();
    named.dedup();

    assert!(
        named.len() >= 4,
        "the workshop must name the four OAUTH_* variables that wire OIDC, found {named:?}",
    );
    for var in &named {
        assert!(
            env_example.contains(var),
            "DEPLOY.md names `{var}`, but `.env.example` has no such key — \
             the workshop has drifted from the env contract",
        );
    }
}

#[test]
fn workshop_oidc_mechanism_matches_the_oauth_code() {
    // The discovery URL and the two route paths the prose teaches must be
    // the same literals the OIDC flow actually uses, so the "how sign-in
    // works" narrative stays bound to `web/src/oauth.rs`.
    let section = signin_section();
    let oauth_rs = repo_file("web/src/oauth.rs");

    for token in [
        "/.well-known/openid-configuration",
        "/auth/login",
        "/auth/callback",
    ] {
        assert!(
            section.contains(token),
            "DEPLOY.md sign-in section must mention `{token}`",
        );
        assert!(
            oauth_rs.contains(token),
            "DEPLOY.md names `{token}`, but `web/src/oauth.rs` does not use it — prose drifted from code",
        );
    }
}

#[test]
fn never_store_passwords_promise_holds_in_the_code() {
    // The workshop promises, in print, that Navigator never stores a
    // password. Bind that promise to two facts in the source tree: the
    // `persons` entity has no password field, and no password-hashing
    // crate is in the dependency graph. The day someone adds either, the
    // promise is false and this test fails — forcing the doc and the
    // decision to be revisited together.
    let section = signin_section();
    assert!(
        section.contains("never store") || section.contains("never stores"),
        "the workshop section must state the no-password-storage promise",
    );

    let person = repo_file("store/src/entity/person.rs").to_lowercase();
    assert!(
        !person.contains("password"),
        "`persons` entity now mentions `password` — the workshop's no-password promise is broken",
    );

    let lockfile = repo_file("Cargo.lock");
    for crate_name in ["argon2", "bcrypt", "scrypt", "pbkdf2", "password-hash"] {
        let stanza = format!("name = \"{crate_name}\"");
        assert!(
            !lockfile.contains(&stanza),
            "`{crate_name}` is now in Cargo.lock — Navigator is storing/hashing passwords, \
             contradicting the deploy workshop. Update the workshop or reconsider the design.",
        );
    }
}

#[test]
fn workshop_keeps_a_no_code_email_password_path_named() {
    // The email/password-without-Google guidance must keep naming a
    // hosted-login OIDC provider that works with zero code changes, so
    // the "no Google account required" promise can't silently collapse to
    // Google-only. Keycloak is that path and is the IdP the KIND loop
    // already runs.
    let section = signin_section();
    assert!(
        section.contains("Keycloak"),
        "the sign-in section must name Keycloak — the verified zero-code email/password OIDC path",
    );
    assert!(
        section.contains("email/password"),
        "the sign-in section must address the email/password (no-Google) front door",
    );
}
