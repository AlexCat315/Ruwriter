//! Extract readable book text and chapter boundaries from local files.

mod epub;

use std::fs;
use std::io;
use std::ops::Range;
use std::path::Path;

use crate::files::read_import_text;
use crate::headings;

pub(super) const MAX_BOOK_BYTES: u64 = 128 * 1024 * 1024;

pub struct BookText {
    pub title: String,
    pub format: String,
    pub text: String,
    pub chapters: Vec<Chapter>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Chapter {
    pub title: String,
    pub bytes: Range<usize>,
}

pub fn read_book(path: &Path) -> io::Result<BookText> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() > MAX_BOOK_BYTES {
        return Err(invalid("文件不可读取或超过 128 MB。"));
    }
    let fallback = path
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "未命名参考书".into());
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let (title, text, format, chapters) = match extension.as_str() {
        "epub" => {
            let (title, text, chapters) = epub::extract(path)?;
            (title.unwrap_or(fallback), text, "EPUB", chapters)
        }
        "txt" | "md" | "markdown" => {
            let text = read_import_text(path)?;
            let chapters = text_chapters(&text);
            (
                fallback,
                text,
                if extension == "txt" {
                    "TXT"
                } else {
                    "Markdown"
                },
                chapters,
            )
        }
        _ => return Err(invalid("请选择 TXT、Markdown 或 EPUB 参考书。")),
    };
    if text.trim().is_empty() {
        return Err(invalid("书中没有可读取的正文。"));
    }
    Ok(BookText {
        title,
        format: format.into(),
        text,
        chapters,
    })
}

fn numbered_title(title: &str) -> bool {
    let title = title.trim();
    let Some(tail) = title.strip_prefix('第') else {
        let mut words = title.split_whitespace();
        return words
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("chapter"))
            && words.next().is_some_and(|number| {
                let number = number.trim_matches(['.', ':', '-']).to_ascii_lowercase();
                !number.is_empty()
                    && (number.bytes().all(|ch| ch.is_ascii_digit())
                        || number.bytes().all(|ch| b"ivxlcdm".contains(&ch))
                        || [
                            "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
                            "ten",
                        ]
                        .contains(&number.as_str()))
            });
    };
    let tail = tail.trim_start();
    let digits = tail.trim_start_matches(|ch: char| {
        matches!(ch, '0'..='9' | '０'..='９' | '〇' | '一' | '二' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十' | '百' | '千' | '万' | '两' | '零')
            || ch.is_whitespace()
    });
    digits.len() < tail.len() && digits.starts_with(['章', '回', '话', '节'])
}

fn text_chapters(text: &str) -> Vec<Chapter> {
    let mut marks = headings(text);
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let title = line.trim();
        if title.chars().count() <= 80 && numbered_title(title) {
            marks.push(crate::Heading {
                offset,
                level: 2,
                title: title.to_owned(),
            });
        }
        offset += line.chars().count();
    }
    marks.sort_by_key(|item| item.offset);
    marks.dedup_by_key(|item| item.offset);
    let has_numbered = marks.iter().any(|item| numbered_title(&item.title));
    let level = marks.iter().map(|item| item.level).min().unwrap_or(1);
    let next_level =
        if !has_numbered && marks.iter().filter(|item| item.level == level).count() == 1 {
            marks
                .iter()
                .map(|item| item.level)
                .filter(|&item| item > level)
                .min()
                .unwrap_or(level)
        } else {
            level
        };
    let mut offsets = text
        .char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(text.len()));
    let mut scanned = 0;
    let mut boundaries: Vec<_> = marks
        .into_iter()
        .filter(|heading| {
            if has_numbered {
                numbered_title(&heading.title)
                    || ["序", "楔子", "尾声", "后记", "番外"]
                        .iter()
                        .any(|prefix| heading.title.starts_with(prefix))
            } else {
                heading.level == next_level
            }
        })
        .filter_map(|heading| {
            let byte = offsets.nth(heading.offset.checked_sub(scanned)?)?;
            scanned = heading.offset + 1;
            Some((heading.title, byte))
        })
        .collect();
    if boundaries
        .first()
        .is_none_or(|(_, byte)| !text[..*byte].trim().is_empty())
    {
        let title = if boundaries.is_empty() {
            "正文"
        } else {
            "卷首"
        };
        boundaries.insert(0, (title.into(), 0));
    } else if let Some((_, first)) = boundaries.first_mut() {
        *first = 0;
    }
    boundaries
        .iter()
        .enumerate()
        .map(|(index, (title, start))| Chapter {
            title: title.clone(),
            bytes: *start
                ..boundaries
                    .get(index + 1)
                    .map_or(text.len(), |(_, start)| *start),
        })
        .collect()
}

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_the_entire_book_and_ignores_nested_section_headings() {
        let text = "# 书名\n\n## 第一章 雪地\n她关了灯。\n### 中段\n她继续等。\n## 第二章 回声\n门口有人。";
        let sections = text_chapters(text);
        assert_eq!(
            sections
                .iter()
                .map(|chapter| chapter.title.as_str())
                .collect::<Vec<_>>(),
            ["卷首", "第一章 雪地", "第二章 回声"]
        );
        assert_eq!(
            sections
                .iter()
                .map(|chapter| &text[chapter.bytes.clone()])
                .collect::<String>(),
            text
        );
    }

    #[test]
    fn detects_plain_text_chapter_labels_with_colons_or_roman_numerals() {
        let text = "CHAPTER I: Fog\nWind bent the grass.\n第 2章：火光\n他抬起灯。";
        let labels: Vec<_> = text_chapters(text)
            .into_iter()
            .map(|chapter| chapter.title)
            .collect();
        assert_eq!(labels, ["CHAPTER I: Fog", "第 2章：火光"]);
    }
}
