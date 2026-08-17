//! Client entry point for the wasm bundle.
//!
//! `dx` compiles this binary to `wasm32-unknown-unknown` with the `web`
//! feature; in the browser it launches [`webapp::App`], which hydrates the
//! server-rendered markup `web` produced. Navigator's server is the `web`
//! crate, not this binary — `web` links `webapp` as a library — so with no
//! platform feature selected (the workspace default) `main` is intentionally
//! empty and `cargo build --workspace` stays green.

fn main() {
    #[cfg(feature = "web")]
    dioxus::launch(webapp::App);
}
