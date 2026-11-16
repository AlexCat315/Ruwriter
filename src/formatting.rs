//! Markdown formatting commands. Pure text transforms over character
//! indices (the units Cove's caret and selection report), so they can be
//! tested without a UI and shared by the toolbar, shortcuts and palette.
//!
//! Wrapping commands toggle: marked text loses its marks, unmarked text
//! gains them. Line commands apply to every line the selection touches.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Bold,
    Italic,
    Strikethrough,
    Code,
    Link,
    /// Cycles the touched lines through H1 → H2 → H3 → plain.
    Heading,
    BulletList,
    NumberedList,
    Quote,
}

/// The replacement text and where the caret (and optional selection) lands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub text: String,
    pub caret: usize,
    pub selection: Option<(usize, usize)>,
}

pub fn apply(
    text: &str,
    caret: usize,
    selection: Option<(usize, usize)>,
    command: Command,
) -> Edit {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let caret = caret.min(len);
    let (low, high) = match selection {
        Some((a, b)) => {
            let (a, b) = (a.min(len), b.min(len));
            if a <= b { (a, b) } else { (b, a) }
        }
        None => (caret, caret),
    };
    match command {
        Command::Bold => wrap(&chars, low, high, "**"),
        Command::Italic => wrap(&chars, low, high, "*"),
        Command::Strikethrough => wrap(&chars, low, high, "~~"),
        Command::Code => wrap(&chars, low, high, "`"),
        Command::Link => link(&chars, low, high),
        Command::Heading => lines(&chars, low, high, caret, LineStyle::Heading),
        Command::BulletList => lines(&chars, low, high, caret, LineStyle::Bullet),
        Command::NumberedList => lines(&chars, low, high, caret, LineStyle::Numbered),
        Command::Quote => lines(&chars, low, high, caret, LineStyle::Quote),
    }
}

fn string(chars: &[char]) -> String {
    chars.iter().collect()
}

fn matches_at(chars: &[char], at: usize, mark: &[char]) -> bool {
    at + mark.len() <= chars.len() && chars[at..at + mark.len()] == *mark
}

/// The nearest `mark` before `at` with no other occurrence in between.
fn mark_before(chars: &[char], at: usize, mark: &[char]) -> Option<usize> {
    let found = (0..at).rev().find(|&i| matches_at(chars, i, mark))?;
    let m = mark.len();
    (!(found + m..at).any(|i| matches_at(chars, i, mark))).then_some(found)
}

/// The nearest `mark` at or after `at`.
fn mark_after(chars: &[char], at: usize, mark: &[char]) -> Option<usize> {
    (at..chars.len()).find(|&i| matches_at(chars, i, mark))
}

/// Toggles `marker` around the selection, or inserts a pair at the caret.
fn wrap(chars: &[char], low: usize, high: usize, marker: &str) -> Edit {
    let mark: Vec<char> = marker.chars().collect();
    let m = mark.len();
    let mut out = chars.to_vec();
    if low == high {
        // An empty pair around the caret collapses instead of nesting.
        if low >= m && matches_at(chars, low - m, &mark) && matches_at(chars, low, &mark) {
            out.drain(low..low + m);
            out.drain(low - m..low);
            return Edit {
                text: string(&out),
                caret: low - m,
                selection: None,
            };
        }
        // Caret at the inner end of a wrapped span: the closing mark follows
        // it, so the same command unwraps instead of nesting another pair.
        if matches_at(chars, low, &mark)
            && let Some(open) = mark_before(chars, low, &mark)
            && open + m <= low
        {
            out.drain(low..low + m);
            out.drain(open..open + m);
            let end = low - m;
            return Edit {
                text: string(&out),
                caret: end,
                selection: Some((open, end)),
            };
        }
        // Caret at the inner start: the opening mark precedes it.
        if low >= m
            && matches_at(chars, low - m, &mark)
            && let Some(close) = mark_after(chars, low, &mark)
            && close >= low
        {
            out.drain(close..close + m);
            out.drain(low - m..low);
            let start = low - m;
            return Edit {
                text: string(&out),
                caret: start,
                selection: Some((start, close - m)),
            };
        }
        out.splice(low..low, mark.iter().copied().chain(mark.iter().copied()));
        return Edit {
            text: string(&out),
            caret: low + m,
            selection: None,
        };
    }
    // The selection includes its own marks.
    if high - low >= 2 * m && matches_at(chars, low, &mark) && matches_at(chars, high - m, &mark) {
        out.drain(high - m..high);
        out.drain(low..low + m);
        let end = high - 2 * m;
        return Edit {
            text: string(&out),
            caret: end,
            selection: Some((low, end)),
        };
    }
    // The marks sit just outside the selection.
    if low >= m && matches_at(chars, low - m, &mark) && matches_at(chars, high, &mark) {
        out.drain(high..high + m);
        out.drain(low - m..low);
        let end = high - m;
        return Edit {
            text: string(&out),
            caret: end,
            selection: Some((low - m, end)),
        };
    }
    out.splice(high..high, mark.iter().copied());
    out.splice(low..low, mark.iter().copied());
    Edit {
        text: string(&out),
        caret: high + m,
        selection: Some((low + m, high + m)),
    }
}

fn link(chars: &[char], low: usize, high: usize) -> Edit {
    let selected = &chars[low..high];
    // A complete link in the selection unwraps to its text.
    if let Some(inner) = link_text(selected) {
        let mut out = chars.to_vec();
        out.splice(low..high, inner.iter().copied());
        let end = low + inner.len();
        return Edit {
            text: string(&out),
            caret: end,
            selection: Some((low, end)),
        };
    }
    let mut out = chars.to_vec();
    if low == high {
        out.splice(low..low, "[]()".chars());
        return Edit {
            text: string(&out),
            caret: low + 1,
            selection: None,
        };
    }
    let text = string(selected);
    if is_url(&text) {
        out.splice(low..high, format!("[]({text})").chars());
        return Edit {
            text: string(&out),
            caret: low + 1,
            selection: None,
        };
    }
    out.splice(low..high, format!("[{text}]()").chars());
    Edit {
        text: string(&out),
        caret: low + text.chars().count() + 3,
        selection: None,
    }
}

/// `[text](url)` → `text`, when the whole slice is one link.
fn link_text(chars: &[char]) -> Option<Vec<char>> {
    if chars.first() != Some(&'[') || chars.last() != Some(&')') {
        return None;
    }
    let close =
        (1..chars.len() - 1).find(|&i| chars[i] == ']' && chars.get(i + 1) == Some(&'('))?;
    Some(chars[1..close].to_vec())
}

fn is_url(text: &str) -> bool {
    text.starts_with("http://") || text.starts_with("https://") || text.starts_with("www.")
}

#[derive(Clone, Copy)]
enum LineStyle {
    Heading,
    Bullet,
    Numbered,
    Quote,
}

fn line_start(chars: &[char], mut index: usize) -> usize {
    while index > 0 && chars[index - 1] != '\n' {
        index -= 1;
    }
    index
}

fn line_end(chars: &[char], mut index: usize) -> usize {
    while index < chars.len() && chars[index] != '\n' {
        index += 1;
    }
    index
}

/// Length of the line's existing prefix for `style`, if any.
fn prefix_len(line: &[char], style: LineStyle) -> Option<usize> {
    match style {
        LineStyle::Heading => {
            let hashes = line.iter().take_while(|c| **c == '#').count();
            if (1..=6).contains(&hashes) && line.get(hashes) == Some(&' ') {
                Some(hashes + 1)
            } else {
                None
            }
        }
        LineStyle::Bullet => {
            if matches!(line.first(), Some('-' | '*' | '+')) && line.get(1) == Some(&' ') {
                Some(2)
            } else {
                None
            }
        }
        LineStyle::Numbered => {
            let digits = line.iter().take_while(|c| c.is_ascii_digit()).count();
            if digits > 0 && line.get(digits) == Some(&'.') && line.get(digits + 1) == Some(&' ') {
                Some(digits + 2)
            } else {
                None
            }
        }
        LineStyle::Quote => {
            if line.first() == Some(&'>') {
                Some(if line.get(1) == Some(&' ') { 2 } else { 1 })
            } else {
                None
            }
        }
    }
}

/// Rewrites the prefix of every line from `low` to `high`, keeping the
/// caret on its line at the equivalent column.
fn lines(chars: &[char], low: usize, high: usize, caret: usize, style: LineStyle) -> Edit {
    let start = line_start(chars, low);
    let end = line_end(chars, if high > low { high - 1 } else { low });
    let mut segments = Vec::new();
    let mut cursor = start;
    loop {
        let stop = line_end(chars, cursor);
        segments.push((cursor, stop));
        if stop >= end {
            break;
        }
        cursor = stop + 1;
    }

    let first = &chars[segments[0].0..segments[0].1];
    let heading_target = match style {
        LineStyle::Heading => {
            let level = prefix_len(first, LineStyle::Heading).map_or(0, |n| n - 1);
            if level >= 3 { 0 } else { level + 1 }
        }
        _ => 0,
    };
    let removing = match style {
        LineStyle::Heading => false,
        _ => segments
            .iter()
            .all(|&(s, e)| prefix_len(&chars[s..e], style).is_some()),
    };

    let caret_line = segments
        .iter()
        .position(|&(s, e)| caret >= s && caret <= e)
        .unwrap_or(segments.len() - 1);
    let mut out: Vec<char> = Vec::with_capacity(chars.len() + segments.len() * 3);
    out.extend_from_slice(&chars[..start]);
    let mut new_caret = caret;
    for (index, &(s, e)) in segments.iter().enumerate() {
        let line = &chars[s..e];
        let old = match style {
            LineStyle::Heading => prefix_len(line, LineStyle::Heading).unwrap_or(0),
            LineStyle::Bullet | LineStyle::Numbered => prefix_len(line, LineStyle::Bullet)
                .or_else(|| prefix_len(line, LineStyle::Numbered))
                .unwrap_or(0),
            LineStyle::Quote => prefix_len(line, LineStyle::Quote).unwrap_or(0),
        };
        let prefix: Vec<char> = match style {
            LineStyle::Heading => {
                if heading_target == 0 {
                    Vec::new()
                } else {
                    format!("{} ", "#".repeat(heading_target)).chars().collect()
                }
            }
            LineStyle::Bullet => {
                if removing || prefix_len(line, LineStyle::Bullet).is_some() {
                    Vec::new()
                } else {
                    "- ".chars().collect()
                }
            }
            LineStyle::Numbered => {
                if removing {
                    Vec::new()
                } else {
                    format!("{}. ", index + 1).chars().collect()
                }
            }
            LineStyle::Quote => {
                if removing {
                    Vec::new()
                } else {
                    "> ".chars().collect()
                }
            }
        };
        if index == caret_line {
            let column = caret - s;
            let mapped = if column <= old {
                prefix.len()
            } else {
                (column + prefix.len()).saturating_sub(old)
            };
            new_caret = out.len() + mapped.min(prefix.len() + line.len() - old);
        }
        out.extend_from_slice(&prefix);
        out.extend_from_slice(&line[old..]);
        if index + 1 < segments.len() {
            out.push('\n');
        }
    }
    out.extend_from_slice(&chars[end..]);
    Edit {
        text: string(&out),
        caret: new_caret.min(out.len()),
        selection: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(text: &str, caret: usize, selection: Option<(usize, usize)>, command: Command) -> Edit {
        apply(text, caret, selection, command)
    }

    #[test]
    fn bold_wraps_selection_and_keeps_it_selected() {
        let result = edit("你好世界", 0, Some((0, 2)), Command::Bold);
        assert_eq!(result.text, "**你好**世界");
        assert_eq!(result.selection, Some((2, 4)));
        assert_eq!(result.caret, 4);
    }

    #[test]
    fn bold_toggles_off_marks_outside_selection() {
        let result = edit("**你好**世界", 2, Some((2, 4)), Command::Bold);
        assert_eq!(result.text, "你好世界");
        assert_eq!(result.selection, Some((0, 2)));
    }

    #[test]
    fn bold_toggles_off_marks_inside_selection() {
        let result = edit("**你好**", 0, Some((0, 6)), Command::Bold);
        assert_eq!(result.text, "你好");
        assert_eq!(result.selection, Some((0, 2)));
    }

    #[test]
    fn bold_at_caret_inserts_pair_and_toggles_it_off() {
        let inserted = edit("ab", 1, None, Command::Bold);
        assert_eq!(inserted.text, "a****b");
        assert_eq!(inserted.caret, 3);
        let removed = edit(&inserted.text, inserted.caret, None, Command::Bold);
        assert_eq!(removed.text, "ab");
        assert_eq!(removed.caret, 1);
    }

    #[test]
    fn bold_toggles_off_at_wrapped_span_boundaries() {
        let end = edit("**你好**", 4, None, Command::Bold);
        assert_eq!(end.text, "你好");
        assert_eq!(end.caret, 2);
        assert_eq!(end.selection, Some((0, 2)));
        let start = edit("**你好**", 2, None, Command::Bold);
        assert_eq!(start.text, "你好");
        assert_eq!(start.caret, 0);
        assert_eq!(start.selection, Some((0, 2)));
    }

    #[test]
    fn italic_at_bold_boundary_peels_one_layer() {
        let result = edit("**a**", 2, None, Command::Italic);
        assert_eq!(result.text, "*a*");
        assert_eq!(result.selection, Some((1, 2)));
    }

    #[test]
    fn italic_and_code_and_strike_wrap() {
        assert_eq!(
            edit("word", 0, Some((0, 4)), Command::Italic).text,
            "*word*"
        );
        assert_eq!(edit("word", 0, Some((0, 4)), Command::Code).text, "`word`");
        assert_eq!(
            edit("word", 0, Some((0, 4)), Command::Strikethrough).text,
            "~~word~~"
        );
    }

    #[test]
    fn link_wraps_text_with_caret_in_url() {
        let result = edit("看看这个", 0, Some((0, 4)), Command::Link);
        assert_eq!(result.text, "[看看这个]()");
        assert_eq!(result.caret, 7);
    }

    #[test]
    fn link_wraps_url_with_caret_in_text() {
        let result = edit("https://a.b", 0, Some((0, 11)), Command::Link);
        assert_eq!(result.text, "[](https://a.b)");
        assert_eq!(result.caret, 1);
    }

    #[test]
    fn link_unwraps_complete_link() {
        let result = edit("[文字](https://a.b)", 0, Some((0, 17)), Command::Link);
        assert_eq!(result.text, "文字");
        assert_eq!(result.selection, Some((0, 2)));
    }

    #[test]
    fn link_at_caret_inserts_template() {
        let result = edit("", 0, None, Command::Link);
        assert_eq!(result.text, "[]()");
        assert_eq!(result.caret, 1);
    }

    #[test]
    fn heading_cycles_levels_on_caret_line() {
        let one = edit("标题\n正文", 1, None, Command::Heading);
        assert_eq!(one.text, "# 标题\n正文");
        assert_eq!(one.caret, 3);
        let two = edit(&one.text, one.caret, None, Command::Heading);
        assert_eq!(two.text, "## 标题\n正文");
        let three = edit(&two.text, two.caret, None, Command::Heading);
        assert_eq!(three.text, "### 标题\n正文");
        let plain = edit(&three.text, three.caret, None, Command::Heading);
        assert_eq!(plain.text, "标题\n正文");
        assert_eq!(plain.caret, 1);
    }

    #[test]
    fn heading_applies_to_every_selected_line() {
        let result = edit("甲\n乙\n丙", 0, Some((0, 5)), Command::Heading);
        assert_eq!(result.text, "# 甲\n# 乙\n# 丙");
    }

    #[test]
    fn selection_ending_at_line_start_leaves_that_line_alone() {
        let result = edit("甲\n乙", 0, Some((0, 2)), Command::BulletList);
        assert_eq!(result.text, "- 甲\n乙");
    }

    #[test]
    fn bullet_list_toggles() {
        let added = edit("甲\n乙", 0, Some((0, 3)), Command::BulletList);
        assert_eq!(added.text, "- 甲\n- 乙");
        let removed = edit(
            &added.text,
            0,
            Some((0, added.text.chars().count())),
            Command::BulletList,
        );
        assert_eq!(removed.text, "甲\n乙");
    }

    #[test]
    fn numbered_list_counts_and_converts_bullets() {
        let result = edit("- 甲\n乙\n丙", 0, Some((0, 8)), Command::NumberedList);
        assert_eq!(result.text, "1. 甲\n2. 乙\n3. 丙");
        let removed = edit(
            &result.text,
            0,
            Some((0, result.text.chars().count())),
            Command::NumberedList,
        );
        assert_eq!(removed.text, "甲\n乙\n丙");
    }

    #[test]
    fn quote_toggles_and_keeps_caret_column() {
        let added = edit("引用文字", 2, None, Command::Quote);
        assert_eq!(added.text, "> 引用文字");
        assert_eq!(added.caret, 4);
        let removed = edit(&added.text, added.caret, None, Command::Quote);
        assert_eq!(removed.text, "引用文字");
        assert_eq!(removed.caret, 2);
    }

    #[test]
    fn line_command_on_empty_document() {
        assert_eq!(edit("", 0, None, Command::BulletList).text, "- ");
        assert_eq!(edit("", 0, None, Command::Heading).text, "# ");
    }

    #[test]
    fn caret_inside_removed_prefix_lands_after_new_prefix() {
        let result = edit("## 标题", 1, None, Command::Heading);
        assert_eq!(result.text, "### 标题");
        assert_eq!(result.caret, 4);
    }

    #[test]
    fn indices_are_characters_not_bytes() {
        let result = edit("中文 abc", 3, Some((3, 6)), Command::Bold);
        assert_eq!(result.text, "中文 **abc**");
        assert_eq!(result.selection, Some((5, 8)));
    }

    #[test]
    fn out_of_range_indices_are_clamped() {
        let result = edit("ab", 99, Some((99, 100)), Command::Bold);
        assert_eq!(result.text, "ab****");
        assert_eq!(result.caret, 4);
    }
}
