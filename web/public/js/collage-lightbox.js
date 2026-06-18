// collage-lightbox — first-party click-to-zoom for blog photo collages.
//
// The collage tiles (`.blog-collage img`) are square-cropped with
// `object-fit: cover` so portrait and landscape photos read as one tidy
// grid — but that crop hides the edges of a photo (heads, people at the
// margins). This upgrades each tile into a button that opens the FULL,
// uncropped image in a lightbox overlay (`object-fit: contain`, so
// nothing is cut off). Close with a click on the backdrop, the ✕ button,
// or Escape.
//
// Inert on every page that has no `.blog-collage` (it only wires up when
// a collage is present), and progressive: with JS off the tiles are
// still the images they always were. No framework — native DOM only.

(function () {
  "use strict";

  function init() {
    var tiles = document.querySelectorAll(".blog-collage img");
    if (!tiles.length) {
      return;
    }

    // One overlay, reused for every tile. Built lazily on first open.
    var overlay = null;
    var fullImg = null;
    var caption = null;
    var lastFocused = null;

    function build() {
      overlay = document.createElement("div");
      overlay.className = "collage-lightbox";
      overlay.setAttribute("role", "dialog");
      overlay.setAttribute("aria-modal", "true");
      overlay.hidden = true;

      var closeBtn = document.createElement("button");
      closeBtn.type = "button";
      closeBtn.className = "collage-lightbox__close";
      closeBtn.setAttribute("aria-label", "Close");
      closeBtn.innerHTML = "&times;";

      var figure = document.createElement("figure");
      figure.className = "collage-lightbox__figure";

      fullImg = document.createElement("img");
      fullImg.className = "collage-lightbox__img";
      fullImg.alt = "";

      caption = document.createElement("figcaption");
      caption.className = "collage-lightbox__caption";

      figure.appendChild(fullImg);
      figure.appendChild(caption);
      overlay.appendChild(closeBtn);
      overlay.appendChild(figure);
      document.body.appendChild(overlay);

      // Backdrop click closes; a click on the image itself does not.
      overlay.addEventListener("click", function (e) {
        if (e.target === fullImg) {
          return;
        }
        close();
      });
      closeBtn.addEventListener("click", close);
    }

    function open(img) {
      if (!overlay) {
        build();
      }
      lastFocused = document.activeElement;
      fullImg.src = img.currentSrc || img.src;
      fullImg.alt = img.alt || "";
      caption.textContent = img.alt || "";
      caption.hidden = !img.alt;
      overlay.hidden = false;
      document.body.classList.add("collage-lightbox-open");
      overlay.querySelector(".collage-lightbox__close").focus();
      document.addEventListener("keydown", onKey);
    }

    function close() {
      if (!overlay || overlay.hidden) {
        return;
      }
      overlay.hidden = true;
      document.body.classList.remove("collage-lightbox-open");
      document.removeEventListener("keydown", onKey);
      fullImg.removeAttribute("src");
      if (lastFocused && typeof lastFocused.focus === "function") {
        lastFocused.focus();
      }
    }

    function onKey(e) {
      if (e.key === "Escape") {
        close();
      }
    }

    tiles.forEach(function (img) {
      img.setAttribute("role", "button");
      img.setAttribute("tabindex", "0");
      img.setAttribute("aria-haspopup", "dialog");
      var hint = img.alt ? "Enlarge: " + img.alt : "Enlarge photo";
      img.setAttribute("aria-label", hint);
      img.addEventListener("click", function () {
        open(img);
      });
      img.addEventListener("keydown", function (e) {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          open(img);
        }
      });
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
