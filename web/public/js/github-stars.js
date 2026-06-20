(() => {
  function formatStars(count) {
    return new Intl.NumberFormat(undefined, {
      maximumFractionDigits: count >= 1000 ? 1 : 0,
      notation: count >= 10000 ? "compact" : "standard",
    }).format(count);
  }

  async function hydrate(button) {
    const countEl = button.querySelector("[data-github-star-count]");
    if (!countEl) {
      return;
    }

    try {
      const response = await fetch("/github-stars", {
        headers: { Accept: "application/json" },
      });
      if (!response.ok) {
        return;
      }

      const payload = await response.json();
      const count = Number(payload.stargazers_count);
      if (!Number.isFinite(count) || count < 0) {
        return;
      }

      const formatted = formatStars(count);
      countEl.textContent = `${formatted} stars`;
      countEl.hidden = false;

      const label = button.querySelector("[data-github-star-label]")?.textContent?.trim();
      if (label) {
        button.setAttribute("aria-label", `${label} (${formatted} stars)`);
      }
    } catch (_) {
      // Leave the static CTA intact when the network or GitHub is unavailable.
    }
  }

  function hydrateAll() {
    document.querySelectorAll("[data-github-star-count]").forEach((countEl) => {
      const button = countEl.closest("a");
      if (button) {
        hydrate(button);
      }
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", hydrateAll, { once: true });
  } else {
    hydrateAll();
  }
})();
