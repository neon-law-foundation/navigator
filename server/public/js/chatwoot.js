// Boot the Chatwoot support-chat widget.
//
// This bootstrap lives in a first-party file rather than an inline <script>
// because every rendered page carries `script-src 'self' 'nonce-…'`: a
// same-origin file is admitted by `'self'` with no nonce, so the widget does
// not have to reach into the per-response nonce the render middleware mints
// for Dioxus's hydration scripts. The vendor SDK it appends is the only
// off-origin script the policy names.
//
// The inbox token and installation origin arrive as `data-` attributes on this
// element (see `portal::chatwoot::ChatwootWidget::script_tag`), which keeps
// this file static and cacheable and keeps per-deployment configuration in the
// deployment's environment where the rest of it lives.
(function () {
  "use strict";

  // Set during execution of a classic script, deferred ones included. Absent
  // only if this file is ever loaded some other way, in which case there is no
  // configuration to read and nothing to do.
  var loader = document.currentScript;
  if (!loader) {
    return;
  }

  var websiteToken = loader.getAttribute("data-website-token");
  var baseUrl = loader.getAttribute("data-base-url");
  if (!websiteToken || !baseUrl) {
    return;
  }

  // Read by the SDK at `run()` time. `launcherTitle` is empty on purpose: the
  // bubble carries no label, so the widget adds one control to the page rather
  // than a second piece of copy competing with the page's own.
  window.chatwootSettings = {
    position: "right",
    type: "standard",
    launcherTitle: "",
  };

  var sdk = document.createElement("script");
  sdk.src = baseUrl + "/packs/js/sdk.js";
  sdk.async = true;
  sdk.onload = function () {
    window.chatwootSDK.run({ websiteToken: websiteToken, baseUrl: baseUrl });
  };
  document.head.appendChild(sdk);
})();
