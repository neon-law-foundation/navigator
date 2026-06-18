// Initialize highlight.js on the server-rendered code blocks (the
// `<pre><code class="language-…">` the talk slides and the /design gallery
// emit). Kept as a first-party EXTERNAL file rather than an inline
// `<script>` so it satisfies the strict `script-src 'self'` CSP — an inline
// init call is blocked by the browser and the highlighter never runs.
// Loaded right after highlight.min.js, which defines the global `hljs`.
hljs.highlightAll();
