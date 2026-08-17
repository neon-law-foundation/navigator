// First-party: the "Copy as Markdown" button on workshop and show-and-tell
// pages. Fetches the page's `.md` twin and writes the body to the clipboard.
//
// Replaces the Alpine `x-data` / `x-on:click` / `x-text` island that used to do
// this inline. It was Alpine's last consumer in the tree, and one copy button
// is not worth shipping a reactivity framework for.
//
// Inert unless a `[data-copy-markdown]` button is present, so it costs nothing
// on every other page. Progressive enhancement: with JavaScript off the button
// is absent entirely (the visible `.md` link beside it is the fallback, and it
// is the same canonical URL).
(() => {
  const RESET_AFTER_MS = 2000;

  async function copy(button) {
    const href = button.dataset.copyMarkdown;
    const label = button.querySelector("[data-copy-markdown-label]");
    if (!href || !label) {
      return;
    }

    // The label only changes once the clipboard write resolves, so a failed
    // fetch or a denied permission leaves the button reading "Copy as
    // Markdown" rather than falsely claiming success.
    try {
      const response = await fetch(href);
      if (!response.ok) {
        return;
      }
      await navigator.clipboard.writeText(await response.text());
    } catch {
      return;
    }

    const original = label.textContent;
    label.textContent = "Copied!";
    window.setTimeout(() => {
      label.textContent = original;
    }, RESET_AFTER_MS);
  }

  function hydrate() {
    for (const button of document.querySelectorAll("[data-copy-markdown]")) {
      button.addEventListener("click", () => copy(button));
    }
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", hydrate);
  } else {
    hydrate();
  }
})();
