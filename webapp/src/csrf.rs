//! The session CSRF token as an injected request-context value (#641 Phase 3).
//!
//! CRUD admin pages post their mutations through native HTML forms that carry a
//! hidden `_csrf` field, exactly as the pages did. The token lives on the
//! server's session, which the `webapp` crate cannot see (it depends on `store`,
//! not `portal`), so — mirroring how `web`/`portal` injects [`crate::people::
//! ViewerRole`] — a portal middleware reads the session's CSRF token and inserts
//! this wasm-safe newtype as a request extension. A `#[server]` function extracts
//! it and threads it into the rendered form.

use serde::{Deserialize, Serialize};

/// The session's CSRF token, injected into the request by the portal
/// `inject_csrf_token` layer. Empty when there is no session (e.g. the
/// direct-mount SSR tests), which is harmless — those requests never post.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CsrfToken(pub String);
