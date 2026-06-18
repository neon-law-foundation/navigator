//! Translation between byte offsets (what the rules engine uses)
//! and LSP positions (UTF-16 code units within a line). Done at the
//! protocol boundary so `rules` stays UTF-8-byte-native.

use std::ops::Range;

use lsp_types::{Position, Range as LspRange};

/// UTF-16 code-unit position for a byte offset in `text`. Returns
/// `Position { line: 0, character: 0 }` for an out-of-range offset.
#[must_use]
pub fn byte_to_position(text: &str, byte_offset: usize) -> Position {
    let mut line: u32 = 0;
    let mut line_start = 0usize;
    let byte_offset = byte_offset.min(text.len());
    for (i, ch) in text.char_indices() {
        if i >= byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    let mut character: u32 = 0;
    let mut byte = line_start;
    while byte < byte_offset {
        let ch = match text[byte..].chars().next() {
            Some(c) => c,
            None => break,
        };
        character += u32::try_from(ch.len_utf16()).unwrap_or(0);
        byte += ch.len_utf8();
    }
    Position { line, character }
}

/// Convert a byte-offset `Range` to an LSP `Range` with both ends
/// translated to UTF-16 positions.
#[must_use]
pub fn range_to_lsp_range(text: &str, range: &Range<usize>) -> LspRange {
    LspRange {
        start: byte_to_position(text, range.start),
        end: byte_to_position(text, range.end),
    }
}

#[cfg(test)]
mod tests {
    use super::{byte_to_position, range_to_lsp_range};
    use lsp_types::Position;

    #[test]
    fn byte_to_position_on_first_line_counts_utf16_units() {
        let text = "hello\nworld\n";
        assert_eq!(
            byte_to_position(text, 0),
            Position {
                line: 0,
                character: 0
            }
        );
        assert_eq!(
            byte_to_position(text, 5),
            Position {
                line: 0,
                character: 5
            }
        );
    }

    #[test]
    fn byte_to_position_advances_line_after_newline() {
        let text = "ab\ncd\n";
        assert_eq!(
            byte_to_position(text, 3),
            Position {
                line: 1,
                character: 0
            }
        );
        assert_eq!(
            byte_to_position(text, 5),
            Position {
                line: 1,
                character: 2
            }
        );
    }

    #[test]
    fn byte_to_position_treats_astral_char_as_two_utf16_units() {
        // U+1F600 is 4 bytes in UTF-8 and 2 code units in UTF-16
        // (surrogate pair). Position character must be 2 after the
        // smiley.
        let text = "😀x";
        assert_eq!(
            byte_to_position(text, 4),
            Position {
                line: 0,
                character: 2
            }
        );
    }

    #[test]
    fn range_to_lsp_range_converts_both_ends() {
        let text = "first\nsecond\n";
        let r = range_to_lsp_range(text, &(6..12));
        assert_eq!(r.start.line, 1);
        assert_eq!(r.start.character, 0);
        assert_eq!(r.end.line, 1);
        assert_eq!(r.end.character, 6);
    }
}
