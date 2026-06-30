# Unit tests for the Neon Law Navigator authorization policy.
#
# These pin every decision the firm-wide access model promises — the
# allow rules AND the deny cases — directly against the real Rego in
# `opa.yaml` (the test harness extracts the ConfigMap's `navigator.rego`
# and runs `opa test` over both files, so there is exactly one copy of
# the policy and nothing to drift). This is the home for OPA decision
# coverage; the KIND e2e no longer port-forwards a live OPA to assert
# them, because doing so was chronically flaky in CI.
#
# Run locally:  opa test k8s/base/opa/navigator_test.rego <extracted navigator.rego>
# In CI:        cli/tests/opa_policy.rs extracts the policy and shells `opa test`.

package navigator.authz_test

import rego.v1

import data.navigator.authz

# ---------- sessions ----------
# `session` is null when unauthenticated; otherwise it carries the
# singular `role` field OPA evaluates (post `roles[] → role` collapse).

admin_session := {"sub": "x", "email": "a@neonlaw.com", "exp": 9999999999, "role": "admin", "csrf_token": ""}

staff_session := {"sub": "x", "email": "s@neonlaw.com", "exp": 9999999999, "role": "staff", "csrf_token": ""}

client_session := {"sub": "x", "email": "c@example.com", "exp": 9999999999, "role": "client", "csrf_token": ""}

# ---------- admin bypass ----------

test_admin_reaches_portal if {
	authz.allow with input as {"path": ["portal"], "method": "GET", "session": admin_session}
}

test_admin_bypass_reaches_admin_surface if {
	authz.allow with input as {"path": ["portal", "admin", "people"], "method": "GET", "session": admin_session}
}

# ---------- /portal landing ----------

test_client_reaches_portal if {
	authz.allow with input as {"path": ["portal"], "method": "GET", "session": client_session}
}

test_anonymous_denied_on_portal if {
	not authz.allow with input as {"path": ["portal"], "method": "GET", "session": null}
}

# ---------- /portal/projects/* (any authenticated caller) ----------

test_client_reaches_projects if {
	authz.allow with input as {"path": ["portal", "projects"], "method": "GET", "session": client_session}
}

test_anonymous_denied_on_projects if {
	not authz.allow with input as {"path": ["portal", "projects"], "method": "GET", "session": null}
}

test_client_can_approve_plan if {
	authz.allow with input as {"path": ["portal", "projects", "p1", "approve-plan"], "method": "POST", "session": client_session}
}

test_client_can_submit_contract_review if {
	authz.allow with input as {"path": ["portal", "projects", "p1", "contract-review"], "method": "POST", "session": client_session}
}

# ---------- /portal/forms/* (blank public forms) ----------

test_client_can_browse_forms if {
	authz.allow with input as {"path": ["portal", "forms"], "method": "GET", "session": client_session}
}

test_anonymous_denied_on_forms if {
	not authz.allow with input as {"path": ["portal", "forms"], "method": "GET", "session": null}
}

# ---------- /portal/admin/* (staff tier only) ----------

test_staff_reaches_admin_people if {
	authz.allow with input as {"path": ["portal", "admin", "people"], "method": "GET", "session": staff_session}
}

test_client_denied_on_admin_people if {
	not authz.allow with input as {"path": ["portal", "admin", "people"], "method": "GET", "session": client_session}
}

test_anonymous_denied_on_admin if {
	not authz.allow with input as {"path": ["portal", "admin"], "method": "GET", "session": null}
}

# ---------- /mcp (staff tier only) ----------

test_staff_reaches_mcp if {
	authz.allow with input as {"path": ["mcp"], "method": "POST", "session": staff_session}
}

test_client_denied_on_mcp if {
	not authz.allow with input as {"path": ["mcp"], "method": "POST", "session": client_session}
}

# ---------- /api/aida/rpc (staff tier only) ----------

test_staff_reaches_aida_rpc if {
	authz.allow with input as {"path": ["api", "aida", "rpc"], "method": "POST", "session": staff_session}
}

test_client_denied_on_aida_rpc if {
	not authz.allow with input as {"path": ["api", "aida", "rpc"], "method": "POST", "session": client_session}
}

# ---------- /api/* read paths (any authenticated caller) ----------

test_authenticated_get_api_allowed if {
	authz.allow with input as {"path": ["api", "people"], "method": "GET", "session": client_session}
}

test_anonymous_get_api_denied if {
	not authz.allow with input as {"path": ["api", "people"], "method": "GET", "session": null}
}

test_authenticated_post_api_denied if {
	not authz.allow with input as {"path": ["api", "people"], "method": "POST", "session": client_session}
}

# ---------- stateless notation validator (the one public-ish POST) ----------

test_authenticated_can_validate_notation if {
	authz.allow with input as {"path": ["api", "notations", "validate"], "method": "POST", "session": client_session}
}

test_anonymous_denied_notation_validate if {
	not authz.allow with input as {"path": ["api", "notations", "validate"], "method": "POST", "session": null}
}

# ---------- public documentation surfaces (decided by routing, not OPA) ----------

# /openapi.json and the /api-docs Swagger shell are public, but their
# public-ness is NOT this policy's job: `web` mounts them OUTSIDE
# require_policy (see `web::api::doc_routes`), so OPA never sees them.
# These tests pin that the default-deny policy intentionally does NOT
# carry a redundant allow rule for them — re-adding one would duplicate
# the routing decision and reintroduce the lockstep drift that gated
# /api-docs in prod when the binary shipped ahead of this ConfigMap.
# That `web` keeps them reachable without a session is guarded by
# `web/tests/routes.rs::public_doc_surfaces_bypass_opa_when_policy_denies`.
test_policy_does_not_decide_openapi if {
	not authz.allow with input as {"path": ["openapi.json"], "method": "GET", "session": null}
}

test_policy_does_not_decide_api_docs if {
	not authz.allow with input as {"path": ["api-docs"], "method": "GET", "session": null}
}

# The docs shell moved out of the gated `/api/*` prefix; the old
# `/api/docs` path carries no public exemption anymore. An anonymous
# GET must NOT be allowed (the route is gone, but if it ever returned
# the policy must not silently re-open it).
test_anonymous_denied_old_api_docs if {
	not authz.allow with input as {"path": ["api", "docs"], "method": "GET", "session": null}
}
