# Noto Serif font

**Noto Serif** is the firm's typeface. `pdf/src/lib.rs` embeds it via `include_bytes!` and sets it as the default family
for every rendered PDF. It's a sturdy serif that stays legible from headline down to fine print, and its broad Unicode
coverage (Latin + all European accents, Cyrillic, Greek, Vietnamese) keeps client names and addresses rendering
correctly without missing-glyph boxes.

- Upstream: <https://fonts.google.com/noto/specimen/Noto+Serif> (source: `google/fonts`, `ofl/notoserif/`).
- License: SIL Open Font License 1.1 — full text in [`OFL.txt`](OFL.txt).

## Files checked in

The two Google Fonts variable masters (`wght` + `wdth` axes). Typst instantiates Regular and Bold off the weight axis,
so two files cover regular / bold / italic:

- `NotoSerif-VF.ttf` — upright
- `NotoSerif-Italic-VF.ttf` — italic

## Provenance

Downloaded verbatim from `google/fonts` (`ofl/notoserif/NotoSerif[wdth,wght].ttf` and the italic master); only the
filenames were changed to drop the axis brackets. The web side serves the same typeface self-hosted as Fontsource subset
woff2 (`web/public/fonts/noto-serif/`, SHA-pinned in `web/public/VENDOR.toml`) — no CDN on either surface. To refresh,
pull the new masters from `google/fonts` and re-pin the web woff2.
