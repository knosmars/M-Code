//! Map between 0-based character columns (tool API) and 0-based UTF-16
//! code-unit columns (LSP). ASCII is identical; non-BMP chars differ.

/// UTF-16 code-unit offset of the first `char_col` characters of `line`.
pub fn char_to_utf16(line: &str, char_col: usize) -> u32 {
    line.chars().take(char_col).map(|c| c.len_utf16() as u32).sum()
}

/// Character index corresponding to a UTF-16 code-unit offset; clamps to the
/// character count if the offset lands past the end (or inside a surrogate
/// pair, returning the char that starts at/after the offset).
pub fn utf16_to_char(line: &str, utf16_col: u32) -> usize {
    let mut units = 0u32;
    for (i, c) in line.chars().enumerate() {
        if units >= utf16_col {
            return i;
        }
        units += c.len_utf16() as u32;
    }
    line.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_is_identity() {
        assert_eq!(char_to_utf16("hello", 3), 3);
        assert_eq!(utf16_to_char("hello", 3), 3);
    }

    #[test]
    fn cjk_counts_one_utf16_unit() {
        // each CJK char is 1 UTF-16 unit
        let line = "你好x";
        assert_eq!(char_to_utf16(line, 2), 2);
        assert_eq!(char_to_utf16(line, 3), 3);
        assert_eq!(utf16_to_char(line, 2), 2);
    }

    #[test]
    fn emoji_is_surrogate_pair() {
        // 😀 is 2 UTF-16 units, 1 char
        let line = "a😀b";
        assert_eq!(char_to_utf16(line, 1), 1); // before emoji
        assert_eq!(char_to_utf16(line, 2), 3); // after emoji (1 + 2)
        assert_eq!(char_to_utf16(line, 3), 4);
        assert_eq!(utf16_to_char(line, 3), 2); // utf16 col 3 -> char 2
        assert_eq!(utf16_to_char(line, 1), 1);
    }

    #[test]
    fn out_of_range_clamps_to_end() {
        assert_eq!(char_to_utf16("ab", 9), 2);
        assert_eq!(utf16_to_char("ab", 9), 2);
    }
}
