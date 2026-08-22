// Start the Chatwoot support-chat widget.
//
// This runs as the second of two deferred scripts the render middleware emits
// (see `portal::chatwoot::ChatwootWidget::script_tags`). Deferred classic
// scripts execute in document order, so the vendor `sdk.js` before it has
// already defined `window.chatwootSDK` by the time this runs — which is why
// this file creates no script element and builds no URL. An earlier version
// appended the vendor script itself and assigned `script.src` from a `data-`
// attribute; that is a script-injection sink to any taint analysis, and it was
// unnecessary once the ordering guarantee does the same job.
//
// It stays a separate first-party file rather than an inline block because
// every page carries `script-src 'self' 'nonce-…'` and the nonce is minted per
// response, after the component tree renders; `'self'` admits this file with no
// nonce at all.
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

  // The base URL is DOM text, and `run()` hands it to vendor code that builds
  // the widget iframe's `src` from it. Removing the dynamic `<script>` append
  // took away the script-injection sink, but not the reason to check the value:
  // it is still a URL this page causes a browser to fetch.
  // `ChatwootWidget::from_lookup` already refuses anything that is not an
  // absolute `http(s)` origin, and the check is repeated here rather than
  // assumed from the producer, so the loader is safe to read on its own terms.
  function isSafeInstallationOrigin(value) {
    return /^https?:\/\/[^\s/?#"'<>\\]+$/.test(value);
  }

  if (!isSafeInstallationOrigin(baseUrl)) {
    return;
  }

  // The vendor script is the previous deferred element, so this is only false
  // when it failed to load — an offline visitor, or a blocked request. Leave
  // the page alone rather than throwing.
  if (!window.chatwootSDK) {
    return;
  }

  // Read by the SDK during `run()`. `launcherTitle` is empty on purpose: the
  // bubble carries no label, so the widget adds one control to the page rather
  // than a second piece of copy competing with the page's own.
  window.chatwootSettings = {
    position: "right",
    type: "standard",
    launcherTitle: "",
  };

  window.chatwootSDK.run({ websiteToken: websiteToken, baseUrl: baseUrl });
})();
