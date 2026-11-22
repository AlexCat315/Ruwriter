//! Document cleanup works on physical lines and preserves their line endings.

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag};
use ruwriter_core::WritingMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Indent {
    Keep,
    TwoFullWidth,
    Remove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlankLines {
    Keep,
    Collapse,
    Remove,
    BetweenParagraphs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatOptions {
    pub indent: Indent,
    pub blank_lines: BlankLines,
    pub trim_line_ends: bool,
    pub trim_outer_blank_lines: bool,
}

impl FormatOptions {
    pub const CLEAN: Self = Self {
        indent: Indent::Keep,
        blank_lines: BlankLines::Collapse,
        trim_line_ends: true,
        trim_outer_blank_lines: true,
    };

    pub const NOVEL: Self = Self {
        indent: Indent::TwoFullWidth,
        ..Self::CLEAN
    };
}

struct Change {
    range: Range<usize>,
    replacement: &'static str,
}

pub struct FormatPlan {
    pub text: String,
    pub changed_lines: usize,
    pub before_preview: String,
    pub after_preview: String,
    pub kept_markdown_boundary: bool,
    changes: Vec<Change>,
}

impl FormatPlan {
    /// Map a caret or selection endpoint from source bytes to the formatted text.
    pub fn map_offset(&self, offset: usize) -> usize {
        let mut removed = 0;
        let mut inserted = 0;
        for change in &self.changes {
            if offset < change.range.start {
                break;
            }
            if offset < change.range.end {
                return change.range.start - removed + inserted + change.replacement.len();
            }
            removed += change.range.len();
            inserted += change.replacement.len();
        }
        offset - removed + inserted
    }
}

struct Line<'a> {
    offset: usize,
    body: &'a str,
    ending: &'a str,
}

impl Line<'_> {
    fn end(&self) -> usize {
        self.offset + self.body.len() + self.ending.len()
    }
}

fn lines(source: &str) -> impl Iterator<Item = Line<'_>> {
    let mut offset = 0;
    std::iter::from_fn(move || {
        let rest = source.get(offset..).filter(|rest| !rest.is_empty())?;
        let body_len = rest.find(['\r', '\n']).unwrap_or(rest.len());
        let suffix = &rest[body_len..];
        let ending_len = if suffix.starts_with("\r\n") {
            2
        } else {
            usize::from(!suffix.is_empty())
        };
        let line = Line {
            offset,
            body: &rest[..body_len],
            ending: &suffix[..ending_len],
        };
        offset = line.end();
        Some(line)
    })
}

fn markdown_parser(source: &str) -> Parser<'_> {
    Parser::new_ext(
        source,
        Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_DEFINITION_LIST
            | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
            | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS
            | Options::ENABLE_MATH,
    )
}

/// Keep authored Markdown blocks and inline spans with significant whitespace
/// byte-for-byte. Scan the entire document even if formatting only a selection.
fn markdown_protected(source: &str) -> Vec<Range<usize>> {
    let mut depth = 0;
    let mut protected = Vec::new();
    for (event, range) in markdown_parser(source).into_offset_iter() {
        let protect = match event {
            Event::Start(tag) => {
                let block = depth == 0 && !matches!(tag, Tag::Paragraph);
                depth += 1;
                block
            }
            Event::End(_) => {
                depth -= 1;
                false
            }
            Event::Code(_)
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::HardBreak => true,
            _ => false,
        };
        if protect {
            protected.push(range);
        }
    }
    protected.sort_unstable_by_key(|range| range.start);
    protected
}

pub(crate) fn horizontal_space(ch: char) -> bool {
    matches!(
        ch,
        ' ' | '\t' | '\u{00a0}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
    )
}

fn is_blank(line: &str, markdown: bool) -> bool {
    if markdown {
        line.trim_matches([' ', '\t']).is_empty()
    } else {
        line.trim_matches(horizontal_space).is_empty()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct MarkdownStructureChanged;

pub fn format(
    source: &str,
    selection: Option<Range<usize>>,
    mode: WritingMode,
    options: FormatOptions,
) -> Result<FormatPlan, MarkdownStructureChanged> {
    let markdown = mode == WritingMode::Markdown;
    let blank_lines = if markdown {
        match options.blank_lines {
            BlankLines::Collapse => BlankLines::Collapse,
            _ => BlankLines::Keep,
        }
    } else {
        options.blank_lines
    };
    let scope = selection
        .clone()
        .filter(|range| !range.is_empty())
        .unwrap_or(0..source.len());
    let bom = source
        .strip_prefix('\u{feff}')
        .map_or(0, |body| source.len() - body.len());
    let document_lines = || {
        lines(&source[bom..]).map(move |line| Line {
            offset: line.offset + bom,
            ..line
        })
    };
    let content = options.trim_outer_blank_lines.then(|| {
        let mut nonblank = document_lines().filter(|line| !is_blank(line.body, markdown));
        nonblank.next().map_or(0..0, |first| {
            let last_end = nonblank
                .last()
                .map_or_else(|| first.end(), |line| line.end());
            first.offset..last_end
        })
    });
    // Some Markdown tools recognize front matter only at the first byte,
    // whereas the local parser also recognizes it after a blank line.
    let keep_leading_blank = markdown
        && content.as_ref().is_some_and(|range| {
            range.start > scope.start.max(bom)
                && lines(&source[range.start..]).next().is_some_and(|line| {
                    matches!(line.body.trim_end_matches([' ', '\t']), "---" | "+++")
                })
        });
    let protected = if markdown {
        markdown_protected(source)
    } else {
        Vec::new()
    };
    let mut protected = protected.iter().peekable();
    let mut changes = Vec::new();
    let mut changed_lines = 0;
    let mut blank_run = false;
    let mut previous_ending = None;

    for line in document_lines() {
        let end = line.end();
        while protected
            .peek()
            .is_some_and(|range| range.end <= line.offset)
        {
            protected.next();
        }
        let protected_line = protected.peek().is_some_and(|range| range.start < end);
        let blank = is_blank(line.body, markdown);

        if line.offset >= scope.end {
            break;
        }
        if end <= scope.start || protected_line {
            blank_run = blank;
            previous_ending = None;
            continue;
        }

        let prior = changes.len();
        let outer = content.as_ref().is_some_and(|range| {
            !range.contains(&line.offset) && !(keep_leading_blank && line.offset < range.start)
        });
        if blank
            && (outer
                || blank_lines == BlankLines::Remove
                || (blank_lines != BlankLines::Keep && blank_run))
        {
            changes.push(Change {
                range: line.offset..end,
                replacement: "",
            });
        } else {
            if !blank {
                if let Some(ending) = previous_ending
                    && blank_lines == BlankLines::BetweenParagraphs
                {
                    changes.push(Change {
                        range: line.offset..line.offset,
                        replacement: ending,
                    });
                }
                if !markdown && options.indent != Indent::Keep {
                    let body = line.body.trim_start_matches(horizontal_space);
                    let prefix = &line.body[..line.body.len() - body.len()];
                    let replacement = match options.indent {
                        Indent::TwoFullWidth if ruwriter_core::is_chapter_line(body.trim_end()) => {
                            ""
                        }
                        Indent::TwoFullWidth => "　　",
                        Indent::Remove => "",
                        Indent::Keep => unreachable!(),
                    };
                    if prefix != replacement {
                        changes.push(Change {
                            range: line.offset..line.offset + prefix.len(),
                            replacement,
                        });
                    }
                }
            }
            if options.trim_line_ends || (!markdown && blank && options.indent == Indent::Remove) {
                let trimmed = if markdown {
                    line.body.trim_end_matches([' ', '\t'])
                } else {
                    line.body.trim_end_matches(horizontal_space)
                };
                if trimmed.len() < line.body.len() {
                    changes.push(Change {
                        range: line.offset + trimmed.len()..line.offset + line.body.len(),
                        replacement: "",
                    });
                }
            }
        }
        changed_lines += usize::from(changes.len() != prior);
        blank_run = blank;
        previous_ending = if blank {
            None
        } else {
            match line.ending {
                "\r\n" => Some("\r\n"),
                "\r" => Some("\r"),
                "\n" => Some("\n"),
                _ => None,
            }
        };
    }
    let mut text = String::with_capacity(source.len());
    let mut offset = 0;
    for change in &changes {
        text.push_str(&source[offset..change.range.start]);
        text.push_str(change.replacement);
        offset = change.range.end;
    }
    text.push_str(&source[offset..]);

    if markdown && !changes.is_empty() && !markdown_parser(source).eq(markdown_parser(&text)) {
        if options.trim_outer_blank_lines {
            let mut plan = format(
                source,
                selection,
                mode,
                FormatOptions {
                    trim_outer_blank_lines: false,
                    ..options
                },
            )?;
            plan.kept_markdown_boundary = true;
            return Ok(plan);
        }
        return Err(MarkdownStructureChanged);
    }

    let mut plan = FormatPlan {
        text,
        changed_lines,
        before_preview: String::new(),
        after_preview: String::new(),
        kept_markdown_boundary: keep_leading_blank,
        changes,
    };
    let at = plan
        .changes
        .first()
        .map_or(scope.start, |first| first.range.start);
    let current = source[..at].rfind(['\r', '\n']).map_or(0, |pos| pos + 1);
    let start = if current > 0 && plan.changed_lines > 0 {
        let ending = if source[..current].ends_with("\r\n") {
            2
        } else {
            1
        };
        let prior = source[..current - ending]
            .rfind(['\r', '\n'])
            .map_or(0, |pos| pos + 1);
        if current - prior <= 160 {
            prior
        } else {
            current
        }
    } else {
        current
    };
    plan.before_preview = preview(&source[start..]);
    plan.after_preview = preview(&plan.text[start..]);
    Ok(plan)
}

fn preview(text: &str) -> String {
    const MAX_CHARS: usize = 900;
    let mut result: String = lines(text)
        .take(7)
        .flat_map(|line| line.body.chars().chain(line.ending.chars()))
        .take(MAX_CHARS)
        .collect();
    if result.len() < text.len() {
        result.push_str("\n…");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn novel_preset_formats_titles_indents_and_extra_blank_lines_in_mixed_endings() {
        let source = "  第一章 雨夜  \r\n\t第一段。　\r\n \r\n\r\n　　第二段。\r尾声";
        let plan = format(source, None, WritingMode::Novel, FormatOptions::NOVEL).unwrap();
        assert_eq!(
            plan.text,
            "第一章 雨夜\r\n　　第一段。\r\n\r\n　　第二段。\r尾声"
        );
        let second = format(&plan.text, None, WritingMode::Novel, FormatOptions::NOVEL).unwrap();
        assert_eq!(second.text, plan.text);
        assert_eq!(second.changed_lines, 0);
    }

    #[test]
    fn selection_uses_touched_lines_and_maps_grapheme_boundaries() {
        let source = "  前文  \r\n  👩‍💻甲  \r\n\t乙　\r\n  后文  ";
        let start = source.find("👩").unwrap();
        let end = source.find("  后文").unwrap();
        let options = FormatOptions {
            indent: Indent::Remove,
            ..FormatOptions::CLEAN
        };
        let plan = format(source, Some(start..end), WritingMode::Novel, options).unwrap();
        assert_eq!(plan.text, "  前文  \r\n👩‍💻甲\r\n乙\r\n  后文  ");
        assert_eq!(plan.map_offset(start), plan.text.find("👩").unwrap());
        let after_emoji = start + "👩‍💻".len();
        assert_eq!(plan.map_offset(after_emoji), plan.text.find('甲').unwrap());
        assert_eq!(plan.map_offset(end), plan.text.find("  后文").unwrap());
        assert_eq!(plan.changed_lines, 2);
    }

    #[test]
    fn paragraph_spacing_and_blank_line_removal_do_not_reflow_text() {
        let source = "甲\r\n乙\n\n\n丙";
        let options = FormatOptions {
            blank_lines: BlankLines::BetweenParagraphs,
            ..FormatOptions::CLEAN
        };
        let plan = format(source, None, WritingMode::Novel, options).unwrap();
        assert_eq!(plan.text, "甲\r\n\r\n乙\n\n丙");
        let plan = format(
            &plan.text,
            None,
            WritingMode::Novel,
            FormatOptions {
                blank_lines: BlankLines::Remove,
                ..options
            },
        )
        .unwrap();
        assert_eq!(plan.text, "甲\r\n乙\n丙");
    }

    #[test]
    fn markdown_preserves_syntax_and_only_cleans_safe_whitespace() {
        let metadata = "---\ntitle: 夜雨  \nnotes: |\n  甲  \n  \n  乙\n---";
        let code = "```txt\n  代码  \n\n\n\t缩进\n```";
        let list = "- 任务  \n  - 子项  \n    续行";
        let table = "| A | B |\n| - | - |\n| a | b |  ";
        let hard_break = "第一行  \n下一行";
        let quote = "> 甲  \n> 乙";
        let source = format!(
            "{metadata}\n\n普通一行 \n\n\n{code}\n\n{list}\n\n{table}\n\n{quote}\n\n{hard_break}"
        );
        let plan = format(&source, None, WritingMode::Markdown, FormatOptions::NOVEL).unwrap();
        for protected in [metadata, code, list, table, hard_break, quote] {
            assert!(plan.text.contains(protected), "lost {protected:?}");
        }
        assert!(plan.text.contains("普通一行\n\n```txt"));
        assert!(!plan.text.contains('　'));
    }

    #[test]
    fn markdown_protects_a_selection_inside_a_fenced_block() {
        let source = "开头 \n\n```\n\t甲  \n\n\n乙  \n```\n\n结尾 ";
        let start = source.find('甲').unwrap();
        let end = source.find('乙').unwrap() + '乙'.len_utf8();
        let plan = format(
            source,
            Some(start..end),
            WritingMode::Markdown,
            FormatOptions::CLEAN,
        )
        .unwrap();
        assert_eq!(plan.text, source);
        assert_eq!(plan.changed_lines, 0);
    }

    #[test]
    fn trims_document_boundary_blanks_independently_of_internal_spacing() {
        let source = "\r\n　\n\u{00a0}\r\n甲\n\n\n乙\r\n\t\n";
        let options = FormatOptions {
            blank_lines: BlankLines::Keep,
            trim_line_ends: false,
            ..FormatOptions::CLEAN
        };
        let plan = format(source, None, WritingMode::Novel, options).unwrap();
        assert_eq!(plan.text, "甲\n\n\n乙\r\n");
        assert_eq!(plan.map_offset(source.find('甲').unwrap()), 0);
        let plan = format("　\n\t", None, WritingMode::Novel, options).unwrap();
        assert!(plan.text.is_empty());
    }

    #[test]
    fn formatting_keeps_the_utf8_bom_at_the_start() {
        let source = "\u{feff}\r\n  第一章 开始\r\n正文";
        let plan = format(source, None, WritingMode::Novel, FormatOptions::NOVEL).unwrap();
        assert_eq!(plan.text, "\u{feff}第一章 开始\r\n　　正文");
        let plan = format(
            "\u{feff}正文",
            None,
            WritingMode::Novel,
            FormatOptions::NOVEL,
        )
        .unwrap();
        assert_eq!(plan.text, "\u{feff}　　正文");
    }

    #[test]
    fn markdown_does_not_turn_a_thematic_break_into_front_matter() {
        let source = "\n---\ntitle: Heading\n---\n\n普通段落 \n\n\n下一段";
        let plan = format(source, None, WritingMode::Markdown, FormatOptions::CLEAN).unwrap();
        assert!(plan.kept_markdown_boundary);
        assert_eq!(
            plan.text,
            "\n---\ntitle: Heading\n---\n\n普通段落\n\n下一段"
        );
    }
}
