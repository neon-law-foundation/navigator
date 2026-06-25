//! OpenAPI 3.1 description of the JSON `/api/*` surface.
//!
//! Hand-curated rather than generated so the schema stays
//! deliberately small and free of toolchain ceremony — each entity
//! we expose ends up as one short `components.schemas` entry. When
//! the API grows enough to justify it, a future commit can swap
//! this for a `utoipa`-derived doc without changing the wire path.
//!
//! Scope: this document covers the read-only JSON endpoints under
//! `/api/*`. Every operation requires OIDC authentication — either a
//! browser session cookie (`navigator_session`) issued by the OAuth
//! flow at `/auth/login`, or an upstream-validated JWT / Google OAuth
//! bearer token. The MCP endpoint at `/mcp` is JSON-RPC over HTTP
//! (not REST) and is intentionally NOT documented here — Swagger UI's
//! "Try it out" affordance would mislead callers and risks leaking
//! bearer tokens pasted into a static UI. MCP clients should consult
//! the MCP specification directly.
//!
//! Drift between this document and `api::routes()` is asserted by
//! `web/tests/openapi_drift.rs`.

use serde_json::{json, Value};

use views::brand::FIRM_BRAND;

/// Documentation placeholder used only when neither an explicit
/// override nor a request host is available. Matches the placeholder
/// substitution flow in `examples/deploy/k8s/gke/`.
const PLACEHOLDER_BASE_URL: &str = "https://www.your-domain.example";

/// Resolve the public-facing base URL for the OpenAPI `servers` and
/// `contact` blocks. Precedence, mirroring how the A2A agent card
/// resolves its authority in [`crate::a2a`]:
///
/// 1. `NAV_BASE_URL` env — an explicit operator override.
/// 2. The `authority` from the incoming request's `Host` header — so a
///    deploy surfaces its own host (`www.neonlaw.com` in prod) with
///    zero config and no hard-coded domain in source.
/// 3. The documentation placeholder.
#[must_use]
pub fn base_url_for(authority: Option<&str>) -> String {
    if let Ok(explicit) = std::env::var("NAV_BASE_URL") {
        if !explicit.is_empty() {
            return explicit;
        }
    }
    match authority.filter(|a| !a.is_empty()) {
        Some(authority) => {
            // Loopback hosts are dev-only and never TLS-terminated.
            let scheme = if authority.starts_with("localhost")
                || authority.starts_with("127.0.0.1")
                || authority.starts_with("0.0.0.0")
            {
                "http"
            } else {
                "https"
            };
            format!("{scheme}://{authority}")
        }
        None => PLACEHOLDER_BASE_URL.to_string(),
    }
}

/// The OpenAPI document with the base URL resolved from env / request
/// host. Convenience wrapper over [`document_with_base`] used by the
/// drift test and unit tests, where no request host is available.
#[must_use]
pub fn document() -> Value {
    document_with_base(&base_url_for(None))
}

#[must_use]
#[allow(clippy::too_many_lines)]
pub fn document_with_base(base: &str) -> Value {
    let contact_url = format!("{base}/contact");
    json!({
      "openapi": "3.1.0",
      "info": {
        "title": "Neon Law Navigator API",
        "version": "0.1.0",
        "description":
          "Read-only JSON listings for the Neon Law Navigator domain tables, plus a stateless \
           markdown notation validator. Every `/api/*` endpoint requires OIDC \
           authentication — either a browser session cookie issued by the OAuth flow \
           at `/auth/login`, or a JWT bearer token. The documentation itself stays \
           public: both the OpenAPI document (`/openapi.json`) and the Swagger UI that \
           renders it (`/api/docs`) are reachable without a session, so the schema is \
           discoverable before signing in. The MCP endpoint at `/mcp` (JSON-RPC, \
           Google OAuth bearer) is documented separately.",
        "contact": { "name": FIRM_BRAND.site_name, "url": contact_url }
      },
      "servers": [
        { "url": base, "description": "Production" }
      ],
      "security": [
        { "bearerAuth": [] },
        { "sessionCookie": [] }
      ],
      "paths": {
        "/api/people": {
          "get": {
            "summary": "List all people",
            "responses": {
              "200": { "description": "Person list", "content": { "application/json": {
                "schema": { "type": "array", "items": { "$ref": "#/components/schemas/Person" } }
              } } }
            }
          }
        },
        "/api/people/{id}": {
          "get": {
            "summary": "Get one person by id",
            "parameters": [
              { "name": "id", "in": "path", "required": true,
                "schema": { "type": "string", "format": "uuid" } }
            ],
            "responses": {
              "200": { "description": "Person", "content": { "application/json": {
                "schema": { "$ref": "#/components/schemas/Person" }
              } } },
              "404": { "description": "Not found" }
            }
          }
        },
        "/api/entities": {
          "get": {
            "summary": "List all entities",
            "responses": {
              "200": { "description": "Entity list", "content": { "application/json": {
                "schema": { "type": "array", "items": { "$ref": "#/components/schemas/Entity" } }
              } } }
            }
          }
        },
        "/api/entities/{id}": {
          "get": {
            "summary": "Get one entity by id",
            "parameters": [
              { "name": "id", "in": "path", "required": true,
                "schema": { "type": "string", "format": "uuid" } }
            ],
            "responses": {
              "200": { "description": "Entity", "content": { "application/json": {
                "schema": { "$ref": "#/components/schemas/Entity" }
              } } },
              "404": { "description": "Not found" }
            }
          }
        },
        "/api/jurisdictions": {
          "get": {
            "summary": "List all jurisdictions",
            "responses": {
              "200": { "description": "Jurisdiction list", "content": { "application/json": {
                "schema": { "type": "array",
                            "items": { "$ref": "#/components/schemas/Jurisdiction" } }
              } } }
            }
          }
        },
        "/api/entity-types": {
          "get": {
            "summary": "List all entity types",
            "responses": {
              "200": { "description": "EntityType list", "content": { "application/json": {
                "schema": { "type": "array",
                            "items": { "$ref": "#/components/schemas/EntityType" } }
              } } }
            }
          }
        },
        "/api/notations/validate": {
          "post": {
            "summary": "Lint a markdown notation without saving it",
            "description":
              "Runs the Neon Law Navigator rule engine over the supplied markdown and returns the \
               violations. Stateless: no row is inserted, no template registered. \"Notation\" \
               here is the *markdown notation format* — the file format Templates are written \
               in — not a `notations`-table row and not a persisted Template; nothing is \
               looked up or created, so this is the right call to lint a draft before it \
               exists anywhere. Mirrors `cli validate` rule-set selection: the default uses \
               `navigator_default_rules` (M-family markdown + N-family notation + S101 \
               line length); set `markdown_only: true` to drop the N-family and enable \
               `S102` line packing.",
            "requestBody": {
              "required": true,
              "content": { "application/json": {
                "schema": { "$ref": "#/components/schemas/ValidateRequest" }
              } }
            },
            "responses": {
              "200": { "description": "Lint report", "content": { "application/json": {
                "schema": { "$ref": "#/components/schemas/ValidateResponse" }
              } } }
            }
          }
        }
      },
      "components": {
        "securitySchemes": {
          "bearerAuth": {
            "type": "http",
            "scheme": "bearer",
            "bearerFormat": "JWT",
            "description":
              "OIDC bearer token. In production the token is validated against the \
               configured IdP (Google Identity); in KIND the workspace's Keycloak \
               realm signs HS256 / RS256 JWTs. A browser-initiated alternative — the \
               `navigator_session` cookie set by `/auth/login` — is documented as the \
               `sessionCookie` scheme. Authorization is then delegated to the Open \
               Policy Agent sidecar: the policy in `k8s/base/opa/opa.yaml` allows any \
               authenticated session to call the read-only `/api/*` listings and the \
               stateless `/api/notations/validate` endpoint."
          },
          "sessionCookie": {
            "type": "apiKey",
            "in": "cookie",
            "name": "navigator_session",
            "description":
              "Opaque session cookie set by the OAuth Authorization Code + PKCE flow \
               at `/auth/login`. The same cookie gates the `/portal/*` surface."
          }
        },
        "schemas": {
          "Person": {
            "type": "object",
            "required": ["id", "name", "email", "roles", "inserted_at", "updated_at"],
            "properties": {
              "id":            { "type": "string", "format": "uuid" },
              "name":          { "type": "string" },
              "email":         { "type": "string", "format": "email" },
              "oidc_subject":  { "type": ["string", "null"],
                                 "description": "OIDC `sub` claim once the row is linked." },
              "roles":         { "type": "array",
                                 "items": { "type": "string" },
                                 "description": "Role names; the set OPA evaluates against." },
              "inserted_at":   { "type": "string" },
              "updated_at":    { "type": "string" }
            }
          },
          "Entity": {
            "type": "object",
            "required": ["id", "name", "entity_type_id", "jurisdiction_id",
                         "inserted_at", "updated_at"],
            "properties": {
              "id":              { "type": "string", "format": "uuid" },
              "name":            { "type": "string" },
              "entity_type_id":  { "type": "string", "format": "uuid" },
              "jurisdiction_id": { "type": "string", "format": "uuid" },
              "inserted_at":     { "type": "string" },
              "updated_at":      { "type": "string" }
            }
          },
          "Jurisdiction": {
            "type": "object",
            "required": ["id", "name", "code", "inserted_at", "updated_at"],
            "properties": {
              "id":          { "type": "string", "format": "uuid" },
              "name":        { "type": "string" },
              "code":        { "type": "string",
                               "description": "Short code, e.g. `NV`, `CA`, `US`." },
              "inserted_at": { "type": "string" },
              "updated_at":  { "type": "string" }
            }
          },
          "EntityType": {
            "type": "object",
            "required": ["id", "name", "inserted_at", "updated_at"],
            "properties": {
              "id":          { "type": "string", "format": "uuid" },
              "name":        { "type": "string" },
              "inserted_at": { "type": "string" },
              "updated_at":  { "type": "string" }
            }
          },
          "ValidateRequest": {
            "type": "object",
            "required": ["contents"],
            "properties": {
              "contents":      { "type": "string",
                                 "description": "Raw markdown body, including any YAML frontmatter." },
              "path":          { "type": "string",
                                 "description": "Pretend filename so rules that key off the path \
                                                 (e.g. N103 snake_case) have something to read. \
                                                 Defaults to `notation.md`." },
              "markdown_only": { "type": "boolean",
                                 "description": "Lint with `navigator_markdown_only_rules` instead \
                                                 of the default Neon Law Navigator notation set." }
            },
            "example": {
              "contents":
                "---\ntitle: Trust\ncode: trust\nrespondent_type: entity\nconfidential: false\n\
                 questionnaire:\n  BEGIN:\n    next: END\n  END: {}\n\
                 workflow:\n  BEGIN:\n    next: staff_review\n  \
                 staff_review:\n    next: END\n  END: {}\n---\n\nBody.\n",
              "path": "trust.md"
            }
          },
          "ValidateResponse": {
            "type": "object",
            "required": ["path", "clean", "violations"],
            "properties": {
              "path":       { "type": "string" },
              "clean":      { "type": "boolean" },
              "violations": { "type": "array",
                              "items": { "$ref": "#/components/schemas/ValidationViolation" } }
            },
            "example": { "path": "trust.md", "clean": true, "violations": [] }
          },
          "ValidationViolation": {
            "type": "object",
            "required": ["code", "line", "message"],
            "properties": {
              "code":    { "type": "string", "description": "Rule code, e.g. `S101`, `N101`." },
              "line":    { "type": "integer", "format": "int32" },
              "message": { "type": "string" }
            }
          }
        }
      }
    })
}

/// Every `/api/*` path key declared in [`document`]. Public so the
/// drift test in `web/tests/openapi_drift.rs` can compare it against
/// the routes registered in [`crate::api::routes`].
#[must_use]
pub fn documented_paths() -> Vec<String> {
    let doc = document();
    doc["paths"]
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{base_url_for, document, document_with_base, documented_paths};

    #[test]
    fn base_url_derives_https_from_request_host() {
        // No NAV_BASE_URL set in the test process: a real request host
        // drives the scheme + servers URL, so prod surfaces its own
        // domain without any hard-coded value in source.
        assert_eq!(
            base_url_for(Some("www.neonlaw.com")),
            "https://www.neonlaw.com"
        );
        assert_eq!(
            base_url_for(Some("localhost:8080")),
            "http://localhost:8080"
        );
    }

    #[test]
    fn base_url_falls_back_to_placeholder_without_host() {
        assert_eq!(base_url_for(None), super::PLACEHOLDER_BASE_URL);
    }

    #[test]
    fn document_with_base_threads_host_into_servers_and_contact() {
        let d = document_with_base("https://www.neonlaw.com");
        assert_eq!(d["servers"][0]["url"], "https://www.neonlaw.com");
        assert_eq!(
            d["info"]["contact"]["url"],
            "https://www.neonlaw.com/contact"
        );
    }

    #[test]
    fn document_has_openapi_version_and_paths() {
        let d = document();
        assert_eq!(d["openapi"], "3.1.0");
        assert!(d["paths"]["/api/people"].is_object());
        assert!(d["paths"]["/api/people/{id}"].is_object());
        assert!(d["paths"]["/api/entities"].is_object());
        assert!(d["paths"]["/api/jurisdictions"].is_object());
        assert!(d["paths"]["/api/entity-types"].is_object());
        assert!(d["paths"]["/api/notations/validate"]["post"].is_object());
    }

    #[test]
    fn document_declares_each_schema() {
        let d = document();
        let schemas = &d["components"]["schemas"];
        for name in [
            "Person",
            "Entity",
            "Jurisdiction",
            "EntityType",
            "ValidateRequest",
            "ValidateResponse",
            "ValidationViolation",
        ] {
            assert!(schemas[name].is_object(), "missing schema {name}");
        }
    }

    #[test]
    fn id_schemas_are_uuid_strings_not_int32() {
        let d = document();
        for entity in ["Person", "Entity", "Jurisdiction", "EntityType"] {
            let id = &d["components"]["schemas"][entity]["properties"]["id"];
            assert_eq!(id["type"], "string", "{entity}.id should be string");
            assert_eq!(id["format"], "uuid", "{entity}.id should be uuid");
        }
        for path in ["/api/people/{id}", "/api/entities/{id}"] {
            let params = &d["paths"][path]["get"]["parameters"];
            let id_schema = &params[0]["schema"];
            assert_eq!(id_schema["type"], "string", "{path} id should be string");
            assert_eq!(id_schema["format"], "uuid", "{path} id should be uuid");
        }
    }

    #[test]
    fn top_level_security_requires_auth() {
        let d = document();
        let sec = d["security"].as_array().expect("top-level security array");
        assert!(
            !sec.is_empty(),
            "`/api/*` requires OIDC; the OpenAPI doc must declare it at the top level"
        );
        let has_bearer = sec.iter().any(|req| {
            req.as_object()
                .is_some_and(|m| m.contains_key("bearerAuth"))
        });
        let has_cookie = sec.iter().any(|req| {
            req.as_object()
                .is_some_and(|m| m.contains_key("sessionCookie"))
        });
        assert!(
            has_bearer,
            "bearerAuth must be one of the documented schemes"
        );
        assert!(
            has_cookie,
            "sessionCookie must be one of the documented schemes"
        );
    }

    #[test]
    fn no_operation_overrides_security_to_empty() {
        let d = document();
        let paths = d["paths"].as_object().expect("paths object");
        for (path, methods) in paths {
            for (verb, op) in methods.as_object().expect("methods object") {
                let sec = &op["security"];
                assert!(
                    sec.is_null(),
                    "{verb} {path} must inherit the top-level `security` requirement \
                     (no per-op override); got {sec}"
                );
            }
        }
    }

    #[test]
    fn bearer_and_cookie_schemes_are_declared() {
        let d = document();
        let bearer = &d["components"]["securitySchemes"]["bearerAuth"];
        assert_eq!(bearer["type"], "http");
        assert_eq!(bearer["scheme"], "bearer");
        let cookie = &d["components"]["securitySchemes"]["sessionCookie"];
        assert_eq!(cookie["type"], "apiKey");
        assert_eq!(cookie["in"], "cookie");
        assert_eq!(cookie["name"], "navigator_session");
    }

    #[test]
    fn documented_paths_matches_paths_object() {
        let d = document();
        let mut from_obj: Vec<String> = d["paths"].as_object().unwrap().keys().cloned().collect();
        from_obj.sort();
        let mut from_helper = documented_paths();
        from_helper.sort();
        assert_eq!(from_obj, from_helper);
    }

    #[test]
    fn validate_request_example_is_itself_clean() {
        // The example shipped in the OpenAPI doc is what Swagger's "Try
        // it out" pre-fills. It must lint clean under the default rule
        // set, or the first request a caller sends comes back dirty —
        // a confusing first impression of an endpoint whose whole job
        // is linting. This guards the example against rule drift.
        let d = document();
        let ex = &d["components"]["schemas"]["ValidateRequest"]["example"];
        let contents = ex["contents"].as_str().expect("example.contents string");
        let path = ex["path"].as_str().expect("example.path string");
        let file = rules::SourceFile {
            path: std::path::PathBuf::from(path),
            contents: contents.to_string(),
        };
        let codes: Vec<&str> = rules::navigator_default_rules()
            .iter()
            .flat_map(|r| r.lint(&file))
            .map(|v| v.code)
            .collect();
        assert!(
            codes.is_empty(),
            "OpenAPI ValidateRequest example must lint clean; got {codes:?}"
        );
    }

    #[test]
    fn mcp_is_intentionally_absent() {
        let d = document();
        assert!(
            d["paths"]["/mcp"].is_null(),
            "/mcp is JSON-RPC and out of scope for this OpenAPI doc"
        );
    }
}
