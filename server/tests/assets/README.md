# Web test assets

This directory contains third-party files used only by browser tests. Its vendored `axe.min.js` is axe-core 4.10.2,
injected by the accessibility end-to-end suite and never served by the application or included in the production image.

It serves maintainers who need reproducible accessibility checks without downloading test code at runtime. axe-core is
kept unmodified under MPL-2.0, preserving a clear license and production boundary for the vendored file.
