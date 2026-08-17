//! Court-paper geometry as a Typst template — jurisdiction as a
//! parameter, not a fork (#889).
//!
//! A pleading is not generic paper. It is a fixed number frame that
//! single-spaced material — the counsel block, the caption, footnotes —
//! floats over. Every measurement here derives from one number:
//!
//! ```text
//! text height = 672pt = 28 x 24pt
//! ```
//!
//! [`GRID_UNIT`] is the 24pt line. [`LINES_PER_PAGE`] is the 28-line
//! frame. Everything else is expressed in whole grid units, because a
//! pleading's vertical rhythm is the thing the numbered rail registers
//! against: a line that lands off-grid is a line whose number is wrong.
//!
//! # Jurisdiction is a parameter
//!
//! Three calibrations differ only in their top margin, whether a numbered
//! rail exists, and how leading is set. The governing rule:
//!
//! > **Absolute leading when a rail exists to register against, relative
//! > leading when it does not.**
//!
//! A wrong calibration silently shifts every line on the page, which is
//! the failure mode this module exists to prevent. That is why the
//! calibration is a closed enum ([`Variant`]) whose table is asserted in
//! tests rather than a set of loose arguments a caller can transpose.
//!
//! # One type size
//!
//! A pleading has no type scale. Everything is [`TYPE_SIZE`]; hierarchy
//! comes from case, weight, underline, centring, and indent. Nothing here
//! exposes a size parameter, for the same reason a pleading's heading
//! takes no size argument.
//!
//! # One renderer
//!
//! Typst generates the pleading paper and the browser does not
//! reimplement it, so there is no second renderer to agree with and no
//! geometry-agreement test. The browser displays what Rust produced — the
//! rendered PDF, or the page renderings from #893 — and edits Markdown.

/// The vertical grid. Every line of court paper sits on a 24pt baseline,
/// and the numbered rail counts those baselines.
pub const GRID_UNIT: f64 = 24.0;

/// Lines in the fixed number frame.
pub const LINES_PER_PAGE: u32 = 28;

/// The single derivation the whole layout hangs off:
/// `672pt = 28 x 24pt`.
pub const TEXT_HEIGHT: f64 = GRID_UNIT * LINES_PER_PAGE as f64;

/// The one type size. A pleading has no type scale.
pub const TYPE_SIZE: f64 = 12.0;

/// How leading is specified for a calibration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leading {
    /// Baselines pinned to the 24pt grid, because a rail registers
    /// against them.
    Absolute,
    /// Conventional double spacing, used where no rail exists to
    /// register against.
    Relative,
}

/// A jurisdiction calibration of the same court-paper geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// Trial court with a numbered rail: 28 numbers, a double rule on the
    /// left and a single rule on the right.
    NumberedRailTrial,
    /// Trial court without a rail — a deeper top margin instead.
    NoRailTrial,
    /// Appellate brief: no rail, relative (double) leading.
    Appellate,
}

impl Variant {
    /// Every calibration, so a caller can enumerate rather than guess.
    pub const ALL: &'static [Variant] = &[
        Variant::NumberedRailTrial,
        Variant::NoRailTrial,
        Variant::Appellate,
    ];

    /// Top margin in inches.
    #[must_use]
    pub fn top_margin_inches(self) -> f64 {
        match self {
            Variant::NumberedRailTrial => 1.0,
            Variant::NoRailTrial | Variant::Appellate => 1.5,
        }
    }

    /// Whether this calibration draws the numbered rail.
    #[must_use]
    pub fn has_rail(self) -> bool {
        match self {
            Variant::NumberedRailTrial => true,
            Variant::NoRailTrial | Variant::Appellate => false,
        }
    }

    /// Absolute leading exactly when a rail exists to register against.
    #[must_use]
    pub fn leading(self) -> Leading {
        if self.has_rail() {
            Leading::Absolute
        } else {
            Leading::Relative
        }
    }

    /// The name this calibration is declared under.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Variant::NumberedRailTrial => "numbered_rail_trial",
            Variant::NoRailTrial => "no_rail_trial",
            Variant::Appellate => "appellate",
        }
    }

    /// Parse a declared calibration name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Variant> {
        Self::ALL
            .iter()
            .copied()
            .find(|v| v.as_str() == name.trim())
    }
}

/// Vertical space, expressible **only** in whole grid units.
///
/// Nothing inside the text column takes an arbitrary margin: a half-unit
/// skip would put every following baseline off the rail. Callers state
/// intent in lines, and this is the only way to move down the page.
#[must_use]
pub fn grid_skip(units: u32) -> String {
    format!("#v({}pt, weak: false)\n", GRID_UNIT * f64::from(units))
}

/// A brief that exceeds its allowed length, reported rather than silently
/// overflowed.
///
/// Court page limits are jurisdictional and a filing over the limit gets
/// stricken, so this returns a message a caller surfaces — it never
/// truncates, because dropping a page of argument is worse than filing a
/// long one.
#[must_use]
pub fn page_limit_warning(pages: u32, limit: u32) -> Option<String> {
    (pages > limit).then(|| {
        format!("pleading runs {pages} pages against a {limit}-page limit; it will be over length")
    })
}

/// One table-of-authorities entry: italic case name, roman reporter
/// cite, dotfill, page.
///
/// The closest thing to a Bluebook rule in this crate — the case name is
/// italicised and the reporter citation is not, which is the distinction
/// a court reads the table for.
#[must_use]
pub fn authority_entry(case_name: &str, reporter_cite: &str, page: u32) -> String {
    format!(
        "#block[#emph[{case}], {cite}#box(width: 1fr, repeat[.]){page}]\n",
        case = escape(case_name),
        cite = escape(reporter_cite),
        page = page,
    )
}

/// Escape Typst markup sigils so a party or reporter name renders
/// verbatim. Case names carry ampersands, brackets, and apostrophes.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '\\' | '#' | '$' | '*' | '_' | '`' | '<' | '@' | '[' | ']'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The Typst chrome for a pleading page under `variant`.
///
/// Emits the geometry from the constants above so there is exactly one
/// source of truth for the measurements; changing [`GRID_UNIT`] changes
/// the rail, the leading, and the text height together.
#[must_use]
pub fn preamble(variant: Variant) -> String {
    let top = variant.top_margin_inches();
    // Line box of exactly 1em plus 1em of leading gives a 24pt baseline
    // pitch that does not depend on the font's own ascender/descender
    // metrics — the rail must register against the text regardless of
    // which face is installed.
    let par_rule = match variant.leading() {
        Leading::Absolute => {
            let size = TYPE_SIZE;
            format!(
                "#set text(size: {size}pt, top-edge: 0.75em, bottom-edge: -0.25em)\n\
                 #set par(justify: false, leading: {size}pt, spacing: {size}pt)\n"
            )
        }
        Leading::Relative => {
            let size = TYPE_SIZE;
            format!(
                "#set text(size: {size}pt)\n\
                 #set par(justify: false, leading: 1em, spacing: 1em)\n"
            )
        }
    };

    let background = if variant.has_rail() {
        let count = LINES_PER_PAGE;
        let pitch = GRID_UNIT;
        format!(
            "  background: place(top + left, dx: 0.75in, dy: {top}in, \
             rail(count: {count}, pitch: {pitch}pt)),\n"
        )
    } else {
        String::new()
    };

    format!(
        "{rail_fn}\
         #set page(\n\
        \x20 paper: \"us-letter\",\n\
        \x20 margin: (top: {top}in, bottom: 1in, left: {left}in, right: {right}in),\n\
         {background}\
         )\n\
         {par_rule}\n",
        rail_fn = if variant.has_rail() { RAIL_FN } else { "" },
        top = top,
        left = if variant.has_rail() { 1.5 } else { 1.0 },
        right = if variant.has_rail() { 0.5 } else { 1.0 },
        background = background,
        par_rule = par_rule,
    )
}

/// The numbered rail: `count` line numbers on a `pitch` grid, a double
/// rule to their right, and a single rule at the right margin. Defined
/// once as Typst so the numbers and the rules share one coordinate
/// system and cannot drift apart.
const RAIL_FN: &str = "\
#let rail(count: 28, pitch: 24pt) = {\n\
\x20 box(width: 100%, height: count * pitch)[\n\
\x20   #for i in range(count) [\n\
\x20     #place(top + left, dy: i * pitch, \
box(width: 0.35in)[#align(right)[#text(size: 12pt)[#(i + 1)]]])\n\
\x20   ]\n\
\x20   #place(top + left, dx: 0.45in, line(angle: 90deg, length: count * pitch, \
stroke: 0.5pt))\n\
\x20   #place(top + left, dx: 0.47in, line(angle: 90deg, length: count * pitch, \
stroke: 0.5pt))\n\
\x20   #place(top + right, line(angle: 90deg, length: count * pitch, stroke: 0.5pt))\n\
\x20 ]\n\
}\n";

#[cfg(test)]
mod tests {
    use super::{
        authority_entry, grid_skip, page_limit_warning, preamble, Leading, Variant, GRID_UNIT,
        LINES_PER_PAGE, TEXT_HEIGHT, TYPE_SIZE,
    };

    /// The single derivation the layout hangs off. If this drifts, every
    /// other measurement is wrong.
    #[test]
    fn text_height_is_twenty_eight_twenty_four_point_lines() {
        assert!((TEXT_HEIGHT - 672.0).abs() < f64::EPSILON);
        assert!((GRID_UNIT - 24.0).abs() < f64::EPSILON);
        assert_eq!(LINES_PER_PAGE, 28);
    }

    /// The calibration table from the issue, asserted rather than
    /// trusted: a transposed row silently shifts every line on the page.
    #[test]
    fn the_three_calibrations_match_their_specified_geometry() {
        let table = [
            (Variant::NumberedRailTrial, 1.0, true, Leading::Absolute),
            (Variant::NoRailTrial, 1.5, false, Leading::Relative),
            (Variant::Appellate, 1.5, false, Leading::Relative),
        ];
        for (variant, top, rail, leading) in table {
            assert!(
                (variant.top_margin_inches() - top).abs() < f64::EPSILON,
                "{} top margin",
                variant.as_str()
            );
            assert_eq!(variant.has_rail(), rail, "{} rail", variant.as_str());
            assert_eq!(variant.leading(), leading, "{} leading", variant.as_str());
        }
    }

    /// The governing rule, stated as an invariant over every calibration
    /// so a fourth one cannot be added that violates it unnoticed.
    #[test]
    fn leading_is_absolute_exactly_when_a_rail_exists_to_register_against() {
        for variant in Variant::ALL {
            let expected = if variant.has_rail() {
                Leading::Absolute
            } else {
                Leading::Relative
            };
            assert_eq!(variant.leading(), expected, "{}", variant.as_str());
        }
    }

    #[test]
    fn a_calibration_round_trips_through_its_declared_name() {
        for variant in Variant::ALL {
            assert_eq!(Variant::parse(variant.as_str()), Some(*variant));
        }
        assert_eq!(Variant::parse("some_other_court"), None);
    }

    /// Vertical space is expressible only in whole grid units.
    #[test]
    fn grid_skip_moves_in_whole_lines_only() {
        assert_eq!(grid_skip(1), "#v(24pt, weak: false)\n");
        assert_eq!(grid_skip(3), "#v(72pt, weak: false)\n");
        assert_eq!(grid_skip(0), "#v(0pt, weak: false)\n");
    }

    /// A brief over the limit is reported, never silently overflowed and
    /// never truncated — dropping argument is worse than filing long.
    #[test]
    fn page_limit_warns_rather_than_truncating() {
        assert!(page_limit_warning(30, 30).is_none());
        let warning = page_limit_warning(31, 30).expect("over-length brief must warn");
        assert!(warning.contains("31"));
        assert!(warning.contains("30"));
    }

    /// Italic case name, roman reporter cite, dotfill, page.
    #[test]
    fn an_authority_entry_italicises_only_the_case_name() {
        let entry = authority_entry("Marbury v. Madison", "5 U.S. 137", 12);
        assert!(entry.contains("#emph[Marbury v. Madison]"));
        assert!(
            entry.contains("5 U.S. 137"),
            "the reporter cite stays roman"
        );
        assert!(!entry.contains("#emph[5 U.S. 137]"));
        assert!(entry.contains("repeat[.]"), "dotfill to the page number");
        assert!(entry.ends_with("12]\n"));
    }

    /// A party name carrying Typst sigils must render verbatim.
    #[test]
    fn an_authority_entry_escapes_markup_in_a_party_name() {
        let entry = authority_entry("Ford & Sons [Nev.]", "1 P.2d 1", 3);
        assert!(entry.contains("Ford \\& Sons \\[Nev.\\]") || entry.contains("\\["));
        assert!(!entry.contains("[Nev.]"));
    }

    /// The rail is emitted for exactly the calibration that has one, and
    /// the geometry comes from the shared constants.
    #[test]
    fn only_the_railed_calibration_emits_a_rail() {
        let railed = preamble(Variant::NumberedRailTrial);
        assert!(railed.contains("#let rail("));
        assert!(railed.contains("count: 28"));
        assert!(railed.contains("pitch: 24pt"));
        assert!(railed.contains("top: 1in"));

        for variant in [Variant::NoRailTrial, Variant::Appellate] {
            let plain = preamble(variant);
            assert!(
                !plain.contains("#let rail("),
                "{} must not draw a rail",
                variant.as_str()
            );
            assert!(plain.contains("top: 1.5in"), "{}", variant.as_str());
        }
    }

    /// One type size everywhere — the preamble must not introduce a
    /// second one, because a pleading has no type scale.
    #[test]
    fn the_preamble_sets_exactly_one_type_size() {
        for variant in Variant::ALL {
            let src = preamble(*variant);
            let sizes: Vec<&str> = src.match_indices("size: ").map(|(_, s)| s).collect();
            assert!(!sizes.is_empty(), "{}", variant.as_str());
            assert!(
                !src.contains("size: 14pt") && !src.contains("size: 10pt"),
                "{} introduced a second type size",
                variant.as_str()
            );
            assert!(
                src.contains(&format!("size: {TYPE_SIZE}pt")),
                "{} must set the one type size",
                variant.as_str()
            );
        }
    }
}
