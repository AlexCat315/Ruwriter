//! Heading extraction for the outline pane and the command palette.
//!
//! Only ATX headings (`#` … `######`) count. Lines inside fenced code blocks,
//! indented code blocks and escaped hashes are left alone: a `#` in a shell
//! snippet is not a chapter title.

use serde::{Deserialize, Serialize};

/// One heading, with its offset in characters from the start of the text
/// (what the editor's caret requests use, not bytes).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heading {
    pub offset: usize,
    pub level: u8,
    pub title: String,
}

/// Reads the heading tree of a Markdown document.
pub fn headings(text: &str) -> Vec<Heading> {
    let mut out = Vec::new();
    let mut offset = 0_usize;
    // Marker character and run length of the open fence, if any.
    let mut fence: Option<(char, usize)> = None;

    for line in text.split('\n') {
        let (indent, rest) = split_indent(line);

        if let Some((marker, length)) = fence {
            // A closing fence uses the same marker, at least as long, and
            // carries nothing but whitespace after it.
            let run = rest.chars().take_while(|&c| c == marker).count();
            let after = rest.chars().nth(run);
            if indent <= 3 && run >= length && matches!(after, None | Some(' ' | '\t')) {
                fence = None;
            }
            offset += line.chars().count() + 1;
            continue;
        }

        // A fence opener: three or more backticks or tildes.
        if indent <= 3 {
            let marker = rest.chars().next();
            if let Some(marker @ ('`' | '~')) = marker {
                let run = rest.chars().take_while(|&c| c == marker).count();
                if run >= 3 {
                    fence = Some((marker, run));
                    offset += line.chars().count() + 1;
                    continue;
                }
            }
        }

        // Four spaces indent the line as code; a heading cannot start there.
        if indent <= 3 {
            let hashes = rest.chars().take_while(|&c| c == '#').count();
            if (1..=6).contains(&hashes) {
                let after = rest.chars().nth(hashes);
                if matches!(after, Some(' ') | Some('\t')) {
                    let raw: String = rest.chars().skip(hashes + 1).collect();
                    let title = strip_closing_hashes(raw.trim());
                    if !title.is_empty() {
                        out.push(Heading {
                            offset,
                            level: hashes as u8,
                            title: title.to_owned(),
                        });
                    }
                }
            }
        }

        offset += line.chars().count() + 1;
    }
    out
}

/// The heading whose section contains `caret` (character offset).
pub fn heading_at(headings: &[Heading], caret: usize) -> Option<usize> {
    headings.iter().rposition(|heading| heading.offset <= caret)
}

/// Leading spaces (tabs count as four) and the text after them.
fn split_indent(line: &str) -> (usize, &str) {
    let mut indent = 0;
    let mut byte = 0;
    for (index, character) in line.char_indices() {
        match character {
            ' ' => indent += 1,
            '\t' => indent += 4,
            _ => return (indent, &line[index..]),
        }
        byte = index + character.len_utf8();
    }
    (indent, &line[byte..])
}

/// Removes a closing sequence such as `## Title ##`.
fn strip_closing_hashes(title: &str) -> &str {
    let trimmed = title.trim_end();
    let without = trimmed.trim_end_matches('#').trim_end();
    if without.is_empty() { trimmed } else { without }
}
