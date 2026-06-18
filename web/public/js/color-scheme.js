// color-scheme — first-party OS-driven dark mode. No toggle.
//
// Bootstrap 5.3 reads `data-bs-theme` on <html> for its dark token
// mapping, but its CSS only understands the literal values `light` and
// `dark` — `auto` is inert in CSS. The server renders `data-bs-theme="auto"`
// as a no-JS marker; this script resolves it to `light`/`dark` from the
// operating-system preference (`prefers-color-scheme`) and keeps it in sync
// when the OS flips (e.g. a scheduled day/night switch) without a reload.
//
// Loaded SYNCHRONOUSLY (not deferred) at the very top of <head> so the
// attribute is set before first paint — a deferred script would flash the
// light theme first. It runs on every page and needs no markup hook. With
// JavaScript off, `data-bs-theme="auto"` degrades to Bootstrap's default
// light theme. No framework — native DOM + matchMedia only. CSP-safe: an
// external `'self'` script, since the policy forbids inline scripts.

(function () {
  "use strict";

  var mq = window.matchMedia("(prefers-color-scheme: dark)");

  function apply() {
    document.documentElement.setAttribute(
      "data-bs-theme",
      mq.matches ? "dark" : "light"
    );
  }

  // Resolve immediately, before the body parses and paints.
  apply();

  // Track live OS changes. `addEventListener` is the modern API;
  // `addListener` is the deprecated fallback for older Safari.
  if (typeof mq.addEventListener === "function") {
    mq.addEventListener("change", apply);
  } else if (typeof mq.addListener === "function") {
    mq.addListener(apply);
  }
})();
