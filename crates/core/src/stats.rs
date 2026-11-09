//! Writing statistics. Latin scripts count word runs, CJK/Kana/Hangul count
//! single characters, so Chinese drafts do not report one word per paragraph.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WritingStats {
    pub characters: usize,
    pub words: usize,
    pub lines: usize,
    pub reading_minutes: usize,
}

pub fn count(text: &str) -> WritingStats {
    let mut characters = 0_usize;
    let mut words = 0_usize;
    let mut in_word = false;

    for character in text.chars() {
        if character.is_whitespace() {
            if in_word {
                words += 1;
                in_word = false;
            }
            continue;
        }
        characters += 1;
        if is_ideographic(character) {
            if in_word {
                words += 1;
                in_word = false;
            }
            words += 1;
        } else if character.is_alphanumeric() || (in_word && character == '\'') {
            in_word = true;
        } else if in_word {
            words += 1;
            in_word = false;
        }
    }
    if in_word {
        words += 1;
    }

    WritingStats {
        characters,
        words,
        lines: text.split('\n').count(),
        reading_minutes: if words == 0 { 0 } else { words.div_ceil(300) },
    }
}

fn is_ideographic(character: char) -> bool {
    matches!(character,
        '\u{3400}'..='\u{4dbf}'
        | '\u{4e00}'..='\u{9fff}'
        | '\u{3040}'..='\u{30ff}'
        | '\u{ac00}'..='\u{d7af}'
        | '\u{20000}'..='\u{2fa1f}'
        | '\u{f900}'..='\u{faff}'
    )
}
