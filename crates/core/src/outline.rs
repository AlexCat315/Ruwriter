//! Heading extraction for the outline pane and the command palette.
//!
//! Two kinds of lines count: ATX headings (`#` … `######`) and the chapter
//! lines Chinese prose writers type without any markup (`第一章 雨夜`,
//! `楔子`, `尾声`). Lines inside fenced code blocks, indented code blocks
//! and escaped hashes are left alone: a `#` in a shell snippet is not a
//! chapter title.

use serde::{Deserialize, Serialize};

/// One heading, with its offset in Unicode scalars from the start of the text.
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

        if let Some((level, title)) = chapter_line(rest.trim()) {
            out.push(Heading {
                offset,
                level,
                title: title.to_owned(),
            });
            offset += line.chars().count() + 1;
            continue;
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

/// A chapter line as novelists write it: `第3章 雨夜`, `第十二回 重逢`,
/// `楔子`, `尾声`, `番外 其一`. Prose that merely starts with 第… is
/// rejected by length and by sentence punctuation.
/// True when the line is a Chinese chapter marker such as「第一章 雨夜」.
/// The editor keeps these flush instead of indenting them like a paragraph.
pub fn is_chapter_line(line: &str) -> bool {
    chapter_line(line).is_some()
}

fn chapter_line(line: &str) -> Option<(u8, &str)> {
    let count = line.chars().count();
    if count == 0 || count > 30 {
        return None;
    }
    if line.ends_with(['。', '！', '？', '；', '，', '：']) {
        return None;
    }
    const SPECIAL: [&str; 7] = ["序章", "楔子", "引子", "尾声", "后记", "番外", "序言"];
    if SPECIAL.iter().any(|prefix| line.starts_with(prefix)) {
        return Some((1, line));
    }
    let numbered = line.strip_prefix('第')?.trim_start();
    let end = numbered.find(|character| {
        !matches!(character, '0'..='9' | '０'..='９'
            | '零'
            | '〇'
            | '一'
            | '二'
            | '三'
            | '四'
            | '五'
            | '六'
            | '七'
            | '八'
            | '九'
            | '十'
            | '百'
            | '千'
            | '万'
            | '两')
    })?;
    if end == 0 {
        return None;
    }
    let level = match numbered[end..].trim_start().chars().next()? {
        '卷' | '部' | '篇' => 1,
        '章' | '节' | '回' | '话' => 2,
        _ => return None,
    };
    Some((level, line))
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
