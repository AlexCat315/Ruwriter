//! Markdown syntax coloring for the editor. The text stays plain; marks are
//! dimmed and structure is tinted, so the draft reads like prose while its
//! Markdown remains visible.
//!
//! Spans are rebuilt only when the editor hands over a different buffer, and
//! each glyph is resolved with a binary search.

use std::cell::RefCell;
use std::rc::Rc;

use cove_ui::compose::primitives::text::TextBuffer;
use cove_ui::core::Color;

use crate::theme::SyntaxColors;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Heading,
    Mark,
    Code,
    Link,
    Quote,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

/// Classifies `text` into sorted, non-overlapping byte spans.
pub fn spans(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    let mut in_fence = false;
    let mut offset = 0;
    for raw in text.split_inclusive('\n') {
        let line = raw.trim_end_matches(['\n', '\r']);
        let start = offset;
        offset += raw.len();
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            push(&mut out, start, start + line.len(), Kind::Mark);
            continue;
        }
        if in_fence {
            push(&mut out, start, start + line.len(), Kind::Code);
            continue;
        }
        if is_rule(trimmed) {
            push(&mut out, start, start + line.len(), Kind::Mark);
            continue;
        }
        let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            let mark_end = start + indent + hashes + 1;
            push(&mut out, start + indent, mark_end, Kind::Mark);
            push(&mut out, mark_end, start + line.len(), Kind::Heading);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('>') {
            let mark = 1 + usize::from(rest.starts_with(' '));
            push(&mut out, start + indent, start + indent + mark, Kind::Mark);
            let body = start + indent + mark;
            let before = out.len();
            inline(&line[indent + mark..], body, &mut out);
            fill_gaps(&mut out, before, body, start + line.len(), Kind::Quote);
            continue;
        }
        let marker = list_marker(trimmed);
        if marker > 0 {
            push(
                &mut out,
                start + indent,
                start + indent + marker,
                Kind::Mark,
            );
        }
        inline(&line[indent + marker..], start + indent + marker, &mut out);
    }
    out
}

fn push(out: &mut Vec<Span>, start: usize, end: usize, kind: Kind) {
    if end > start {
        out.push(Span { start, end, kind });
    }
}

/// Tints the uncovered parts of `start..end` with `kind`, keeping the inline
/// spans pushed since `from` and the overall order sorted.
fn fill_gaps(out: &mut Vec<Span>, from: usize, start: usize, end: usize, kind: Kind) {
    let inner: Vec<Span> = out.drain(from..).collect();
    let mut cursor = start;
    for span in inner {
        push(out, cursor, span.start, kind);
        cursor = span.end;
        out.push(span);
    }
    push(out, cursor, end, kind);
}

fn is_rule(line: &str) -> bool {
    let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    compact.len() >= 3
        && (compact.chars().all(|c| c == '-')
            || compact.chars().all(|c| c == '*')
            || compact.chars().all(|c| c == '_'))
}

/// Byte length of a list or task marker including its trailing space.
fn list_marker(line: &str) -> usize {
    let bytes = line.as_bytes();
    let bullet = match bytes.first() {
        Some(b'-' | b'*' | b'+') if bytes.get(1) == Some(&b' ') => 2,
        Some(b'0'..=b'9') => {
            let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
            match (bytes.get(digits), bytes.get(digits + 1)) {
                (Some(b'.' | b')'), Some(b' ')) if digits <= 9 => digits + 2,
                _ => return 0,
            }
        }
        _ => return 0,
    };
    let rest = &line[bullet..];
    if rest.starts_with("[ ] ") || rest.starts_with("[x] ") || rest.starts_with("[X] ") {
        bullet + 4
    } else {
        bullet
    }
}

/// Inline marks: code spans, emphasis markers and links.
fn inline(text: &str, base: usize, out: &mut Vec<Span>) {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'`' => {
                let ticks = bytes[index..].iter().take_while(|b| **b == b'`').count();
                let fence = &text[index..index + ticks];
                if let Some(close) = text[index + ticks..].find(fence) {
                    let content = index + ticks;
                    let end = content + close;
                    push(out, base + index, base + content, Kind::Mark);
                    push(out, base + content, base + end, Kind::Code);
                    push(out, base + end, base + end + ticks, Kind::Mark);
                    index = end + ticks;
                } else {
                    index += ticks;
                }
            }
            b'*' | b'_' | b'~' => {
                let run = bytes[index..]
                    .iter()
                    .take_while(|b| **b == bytes[index])
                    .count();
                // Underscores inside words are text (snake_case), not emphasis.
                let inside_word = bytes[index] == b'_'
                    && index > 0
                    && bytes[index - 1].is_ascii_alphanumeric()
                    && bytes
                        .get(index + run)
                        .is_some_and(|b| b.is_ascii_alphanumeric());
                let lone_tilde = bytes[index] == b'~' && run < 2;
                if !inside_word && !lone_tilde && has_partner(bytes, index, run) {
                    push(out, base + index, base + index + run, Kind::Mark);
                }
                index += run;
            }
            b'[' => {
                if let Some((label_end, url_end)) = link_at(text, index) {
                    let image = index > 0 && bytes[index - 1] == b'!';
                    let open = if image { index - 1 } else { index };
                    push(out, base + open, base + index + 1, Kind::Mark);
                    push(out, base + index + 1, base + label_end, Kind::Link);
                    push(out, base + label_end, base + url_end, Kind::Mark);
                    index = url_end;
                } else {
                    index += 1;
                }
            }
            _ => index += 1,
        }
    }
}

/// Whether a run of emphasis characters has a partner run on the same line:
/// an earlier run when it can close, a later run when it can open. CJK text
/// has no spaces, so a run may be able to do both.
fn has_partner(bytes: &[u8], index: usize, run: usize) -> bool {
    let character = bytes[index];
    let same = |window: &[u8]| window.iter().all(|b| *b == character);
    let can_open = bytes
        .get(index + run)
        .is_some_and(|b| !b.is_ascii_whitespace());
    let can_close = index > 0 && !bytes[index - 1].is_ascii_whitespace();
    let earlier = || bytes[..index].windows(run).filter(|w| same(w)).count() % 2 == 1;
    let later = || bytes[index + run..].windows(run).any(same);
    (can_close && earlier()) || (can_open && later())
}

/// `[label](target)` starting at `open`; returns the byte offsets of `]` and
/// one past `)`.
fn link_at(text: &str, open: usize) -> Option<(usize, usize)> {
    let close = open + text[open..].find(']')?;
    let rest = text.get(close + 1..)?;
    if !rest.starts_with('(') {
        return None;
    }
    let end = close + 1 + rest.find(')')? + 1;
    Some((close, end))
}

/// Cached spans for the buffer the editor last rendered.
#[derive(Default)]
struct Cache {
    identity: Option<usize>,
    length: usize,
    spans: Vec<Span>,
}

/// The per-glyph color lookup Cove's text field calls while drawing.
pub type Highlighter = Rc<dyn Fn(&TextBuffer, usize) -> Option<Color>>;

pub fn highlighter(colors: SyntaxColors) -> Highlighter {
    let cache = RefCell::new(Cache::default());
    Rc::new(move |buffer: &TextBuffer, offset: usize| {
        let mut cache = cache.borrow_mut();
        let identity = buffer.identity();
        if cache.identity != Some(identity) || cache.length != buffer.utf8_length() {
            cache.spans = spans(&buffer.string_value());
            cache.identity = Some(identity);
            cache.length = buffer.utf8_length();
        }
        let spans = &cache.spans;
        let index = spans.partition_point(|span| span.end <= offset);
        let span = spans.get(index).filter(|span| span.start <= offset)?;
        Some(match span.kind {
            Kind::Heading => colors.heading,
            Kind::Mark => colors.mark,
            Kind::Code => colors.code,
            Kind::Link => colors.link,
            Kind::Quote => colors.quote,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(&str, Kind)> {
        spans(text)
            .into_iter()
            .map(|span| (&text[span.start..span.end], span.kind))
            .collect()
    }

    #[test]
    fn headings_split_mark_and_text() {
        assert_eq!(
            kinds("## 标题\n正文"),
            vec![("## ", Kind::Mark), ("标题", Kind::Heading)]
        );
    }

    #[test]
    fn inline_code_emphasis_and_links() {
        assert_eq!(
            kinds("a `x` **b** [c](d)"),
            vec![
                ("`", Kind::Mark),
                ("x", Kind::Code),
                ("`", Kind::Mark),
                ("**", Kind::Mark),
                ("**", Kind::Mark),
                ("[", Kind::Mark),
                ("c", Kind::Link),
                ("](d)", Kind::Mark),
            ]
        );
    }

    #[test]
    fn emphasis_between_cjk_characters() {
        assert_eq!(
            kinds("沙地。**脚印**很快"),
            vec![("**", Kind::Mark), ("**", Kind::Mark)]
        );
        assert_eq!(
            kinds("大约 *四十*"),
            vec![("*", Kind::Mark), ("*", Kind::Mark)]
        );
    }

    #[test]
    fn snake_case_is_not_emphasis() {
        assert!(kinds("snake_case_name").is_empty());
    }

    #[test]
    fn fences_and_lists() {
        assert_eq!(
            kinds("- [ ] 任务\n```\nlet x\n```"),
            vec![
                ("- [ ] ", Kind::Mark),
                ("```", Kind::Mark),
                ("let x", Kind::Code),
                ("```", Kind::Mark),
            ]
        );
    }

    #[test]
    fn quotes_tint_text_around_inline_marks() {
        assert_eq!(
            kinds("> a `b` c"),
            vec![
                ("> ", Kind::Mark),
                ("a ", Kind::Quote),
                ("`", Kind::Mark),
                ("b", Kind::Code),
                ("`", Kind::Mark),
                (" c", Kind::Quote),
            ]
        );
    }

    #[test]
    fn spans_are_sorted() {
        let text = "# T\n> q *e* [l](u)\n1. `c`\n---\n";
        let spans = spans(text);
        assert!(spans.windows(2).all(|w| w[0].end <= w[1].start));
    }
}
