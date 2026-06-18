//! Shared scaffolding for the end-to-end *journey* runners — the specs
//! that follow one client and one lawyer across the full arc of a
//! representation (intake → portal → work product → signature → filing /
//! close) rather than pinning one surface.
//!
//! Each `tests/<journey>.rs` still owns its own `cucumber::World` and
//! step set; this module carries only the mechanics more than one
//! journey would otherwise duplicate: standing up the seeded app,
//! creating the client Person, driving the admin walker over real HTTP,
//! and a worker-shaped runtime for the staff-side workflow signals the
//! web surfaces don't expose.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sea_orm::{ActiveModelTrait, ActiveValue};
use store::{entity, seed, Db};
use tower::ServiceExt;
use web::session::{SessionData, SESSION_COOKIE_NAME};
use web::{policy::PolicyClient, SessionStore};
use workflows::{DispatchingRuntime, InMemoryRuntime};

use crate::{app_state, body_string, fs_storage, in_memory_db};

/// The CSRF token every minted journey session carries; the client POST
/// surfaces echo it back in the form body.
pub const JOURNEY_CSRF: &str = "journey-csrf";

/// The durable wiring one journey drives the firm and the client through.
pub struct Journey {
    pub app: axum::Router,
    pub db: Db,
    /// The shared in-memory journal that both the web app's dispatching
    /// runtime (inside `AppState`) and [`Journey::worker`] read and
    /// write, so a walker drive and a manual worker signal see one state.
    pub runtime: Arc<InMemoryRuntime>,
    pub storage: Arc<dyn cloud::StorageService>,
    /// The concrete billing stub wired into the app's `billing_provider`,
    /// held so a journey can assert what the matter-close fee recorded.
    pub billing: Arc<web::billing::StubBillingProvider>,
    /// The concrete signature stub wired into the app's
    /// `signature_provider`, held so a journey can assert the envelope's
    /// recipient routing and the bytes that were sent.
    pub signature: Arc<web::signature::StubSignatureProvider>,
    /// The session store wired into the app, so a journey can mint a
    /// cookie session for a client Person and drive the client-facing
    /// portal surfaces (intake, review) as that human.
    pub sessions: SessionStore,
}

/// One captured HTTP response: status, `Location` (the walker redirects
/// after every answer), and the body as a string.
pub struct Captured {
    pub status: StatusCode,
    pub location: Option<String>,
    pub body: String,
}

impl Journey {
    /// Seed the canonical catalog and build the real `web` router over an
    /// in-memory app state, sharing one [`InMemoryRuntime`] journal.
    pub async fn open(suite: &str) -> Self {
        let db = in_memory_db().await;
        let storage = fs_storage(suite).await;
        seed::seed_canonical(&db, &storage)
            .await
            .expect("seed canonical");
        let runtime = Arc::new(InMemoryRuntime::new());
        let sessions = SessionStore::new("test-session-key-not-for-production");
        let mut state = app_state(
            db.clone(),
            runtime.clone(),
            storage.clone(),
            PolicyClient::passthrough(),
            None,
            sessions.clone(),
        );
        // Override the app's billing + signature providers with ones we
        // keep concrete handles to, so a journey can assert the
        // matter-close fee and the e-signature envelope.
        let billing = Arc::new(web::billing::StubBillingProvider::new());
        state.billing_provider = billing.clone();
        let signature = Arc::new(web::signature::StubSignatureProvider::new());
        state.signature_provider = signature.clone();
        let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
        Self {
            app,
            db,
            runtime,
            storage,
            billing,
            signature,
            sessions,
        }
    }

    /// The `Cookie` header value for a session as `person` (their role +
    /// id), carrying [`JOURNEY_CSRF`]. The basis for the client-facing
    /// portal drives.
    fn cookie_for(&self, person: &entity::person::Model) -> String {
        let session = SessionData {
            sub: format!("kc-uuid-{}", person.email),
            email: Some(person.email.clone()),
            person_id: Some(person.id),
            exp: web::session::now_unix_secs() + 600,
            role: person.role,
            csrf_token: JOURNEY_CSRF.into(),
            source: web::session::SessionSource::Browser,
        };
        format!("{SESSION_COOKIE_NAME}={}", self.sessions.encode(&session))
    }

    /// `GET path` as `person` over a real cookie session — the client
    /// portal surfaces (intake, review) gate on the session + project ACL.
    pub async fn client_get(&self, person: &entity::person::Model, path: &str) -> Captured {
        self.send(
            Request::builder()
                .uri(path)
                .header("cookie", self.cookie_for(person))
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    /// `POST path` (form-encoded) as `person`. The CSRF token is appended
    /// automatically so the middleware accepts the write.
    pub async fn client_post(
        &self,
        person: &entity::person::Model,
        path: &str,
        body: &str,
    ) -> Captured {
        let body = if body.is_empty() {
            format!("_csrf={JOURNEY_CSRF}")
        } else {
            format!("{body}&_csrf={JOURNEY_CSRF}")
        };
        self.send(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("cookie", self.cookie_for(person))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
    }

    /// A worker-shaped runtime (in-process dispatch + matter close +
    /// compliance `filings`) over the SAME journal the web app uses, so a
    /// runner can drive the staff-side workflow signals the web surfaces
    /// don't expose — e.g. recording the Secretary-of-State filing once
    /// the client has signed.
    #[must_use]
    pub fn worker(&self) -> DispatchingRuntime {
        DispatchingRuntime::new(
            self.runtime.clone(),
            Arc::new(web::email::CapturingEmail::new()),
            self.storage.clone(),
        )
        .with_db(self.db.clone())
    }

    /// `GET path` as an anonymous client (no auth) — for the public,
    /// client-facing surfaces (marketing, the `/es` funnel).
    pub async fn visit(&self, path: &str) -> Captured {
        self.send(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
    }

    /// `GET path` as the firm (admin passthrough auth).
    pub async fn staff_get(&self, path: &str) -> Captured {
        self.send(
            Request::builder()
                .uri(path)
                .header("authorization", "Bearer dev")
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    /// `POST path` (form-encoded) as the firm.
    pub async fn staff_post(&self, path: &str, body: String) -> Captured {
        self.send(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("authorization", "Bearer dev")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
    }

    async fn send(&self, req: Request<Body>) -> Captured {
        let resp = self.app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let location = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(ToString::to_string);
        let body = body_string(resp).await;
        Captured {
            status,
            location,
            body,
        }
    }
}

/// Create a client Person with a display name and email, in the firm's
/// `client` role, so the portal and the signature manifest read the
/// human's real name rather than their email.
pub async fn client(db: &Db, name: &str, email: &str) -> entity::person::Model {
    entity::person::ActiveModel {
        name: ActiveValue::Set(name.into()),
        email: ActiveValue::Set(email.into()),
        role: ActiveValue::Set(entity::person::Role::Client),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert client person")
}

/// Open a matter (Project) for `person_id` in the firm's `client`
/// participation, returning the project id. The demand-side mirror of the
/// admin retainer-walk's project bootstrap, for journeys that drive the
/// notation directly through the worker rather than the web walker.
pub async fn matter(db: &Db, person_id: uuid::Uuid, name: &str) -> uuid::Uuid {
    let project_id = entity::project::ActiveModel {
        name: ActiveValue::Set(name.into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(db).await),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert project")
    .id;
    entity::person_project_role::ActiveModel {
        person_id: ActiveValue::Set(person_id),
        project_id: ActiveValue::Set(project_id),
        participation: ActiveValue::Set("client".into()),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert person_project_role");
    project_id
}

/// Encode one walker answer as the `value=` body the step form expects.
#[must_use]
pub fn answer_body(value: &str) -> String {
    format!("value={}", crate::form_encode(value))
}
