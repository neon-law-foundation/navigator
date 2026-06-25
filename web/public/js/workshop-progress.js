// Workshop slide progress — first-party, same-origin, zero telemetry.
//
// Tracks which slides a learner has viewed entirely in `localStorage`:
// nothing is sent to the server. On a step page it marks the slide seen;
// on the light-table grid it paints a green check on every seen slide,
// updates the "N / M viewed" count, and — once every slide has been
// seen — reveals the certificate request form.
//
// Inert unless a `[data-workshop-progress]` element is on the page, so it
// loads site-wide (like the other first-party scripts) and no-ops
// everywhere else. CSP-safe: no inline script, no external origin.
(function () {
  "use strict";

  // localStorage key for one slide of one workshop. Namespaced so it
  // never collides with anything else the site might store.
  function slideKey(slug, n) {
    return "nav:workshop:" + slug + ":slide:" + n;
  }

  function isSeen(slug, n) {
    try {
      return window.localStorage.getItem(slideKey(slug, n)) === "1";
    } catch (e) {
      // Private mode / disabled storage: degrade to "nothing seen".
      return false;
    }
  }

  function markSeen(slug, n) {
    try {
      window.localStorage.setItem(slideKey(slug, n), "1");
    } catch (e) {
      // No storage: progress simply doesn't persist. Not an error.
    }
  }

  // A step page: record this slide as seen, then show its check badge.
  function initStep(root) {
    var slug = root.getAttribute("data-workshop-slug");
    var n = root.getAttribute("data-slide");
    if (!slug || !n) {
      return;
    }
    markSeen(slug, n);
    var badge = root.querySelector("[data-slide-seen-badge]");
    if (badge) {
      badge.hidden = false;
    }
  }

  // The light table: paint checks, update the count, unlock the cert form.
  function initLightTable(root) {
    var slug = root.getAttribute("data-workshop-slug");
    var total = parseInt(root.getAttribute("data-total"), 10) || 0;
    if (!slug) {
      return;
    }
    var seen = 0;
    var thumbs = root.querySelectorAll("[data-slide]");
    for (var i = 0; i < thumbs.length; i++) {
      var thumb = thumbs[i];
      var n = thumb.getAttribute("data-slide");
      if (isSeen(slug, n)) {
        seen++;
        var badge = thumb.querySelector("[data-slide-seen-badge]");
        if (badge) {
          badge.hidden = false;
        }
      }
    }
    var count = root.querySelector("[data-progress-count]");
    if (count) {
      count.textContent = seen + " / " + total + " viewed";
    }
    if (total > 0 && seen >= total) {
      var gate = root.querySelector("[data-cert-gate]");
      if (gate) {
        gate.hidden = false;
      }
    }
  }

  function init() {
    var roots = document.querySelectorAll("[data-workshop-progress]");
    for (var i = 0; i < roots.length; i++) {
      var root = roots[i];
      var kind = root.getAttribute("data-workshop-progress");
      if (kind === "step") {
        initStep(root);
      } else if (kind === "lighttable") {
        initLightTable(root);
      }
    }
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
