// Nebula slide navigation for the two per-browser deck faces.
//
// The display face (`…/display/{n}`, `[data-nebula-display]`) is a
// full-screen projector page: click and keyboard both advance. The
// classroom step page (`…/step/{n}`, `[data-nebula-step]`) is a reading
// page: only ArrowLeft/ArrowRight navigate, so Space, PageUp/PageDown,
// and clicks keep their normal scrolling and selection meaning. There is
// no websocket and no cross-window state — each browser navigates on its
// own, so the presenter's laptop and the projector stay independent by
// design.
//
// The prev/next controls are the single source of truth: the handlers
// click those `<a>` elements rather than navigating to a URL read from the
// DOM, so there is no untrusted-string-to-location sink. At the first/last
// slide the corresponding control carries no `data-nebula-nav` marker, so
// the lookup finds nothing and the keystroke falls through untouched.
//
// Inert on every page without one of the two roots.
(function () {
  "use strict";

  var display = document.querySelector("[data-nebula-display]");
  var root = display || document.querySelector("[data-nebula-step]");
  if (!root) {
    return;
  }

  // Returns true when a live control existed and was activated, so the
  // caller only suppresses the key's default action when navigation
  // actually happens (nothing is swallowed at the deck's ends).
  function activate(direction) {
    var link = root.querySelector('a[data-nebula-nav="' + direction + '"]');
    if (link) {
      link.click();
      return true;
    }
    return false;
  }

  document.addEventListener("keydown", function (event) {
    // Shift is excluded alongside the other modifiers: Shift+Arrow extends a
    // text selection on the reading page, so it must never move the deck.
    if (
      event.defaultPrevented ||
      event.metaKey ||
      event.ctrlKey ||
      event.altKey ||
      event.shiftKey
    ) {
      return;
    }
    // A key aimed at a control (the step page's Sections menu, which is a
    // <details> disclosure whose focusable part is its <summary>, or a future
    // search box) is that control's to handle, never navigation.
    var target = event.target;
    if (
      target &&
      (target.isContentEditable ||
        /^(BUTTON|INPUT|SELECT|SUMMARY|TEXTAREA)$/.test(target.tagName))
    ) {
      return;
    }
    switch (event.key) {
      case "ArrowRight":
        if (activate("next")) {
          event.preventDefault();
        }
        break;
      case "ArrowLeft":
        if (activate("prev")) {
          event.preventDefault();
        }
        break;
      case "PageDown":
      case " ":
      case "Spacebar":
        // Projector face only; on the step page these keys keep their
        // reading-page scroll behavior.
        if (display && activate("next")) {
          event.preventDefault();
        }
        break;
      case "PageUp":
        if (display && activate("prev")) {
          event.preventDefault();
        }
        break;
      default:
        break;
    }
  });

  // A click on the slide surface advances, like clicking through a deck —
  // display face only. Clicks on a real control (the chevrons, the exit
  // link) are left to their own href so previous/exit still work.
  var surface = display && display.querySelector("[data-nebula-advance]");
  if (surface) {
    surface.addEventListener("click", function (event) {
      if (event.target.closest("a")) {
        return;
      }
      activate("next");
    });
  }
})();
