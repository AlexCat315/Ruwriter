//! Local manuscript traversal, chapter tools and evidence for a writing run.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cove_agent::retrieval::SearchIndex;
use cove_agent::{AgentError, AgentTool, AgentToolResult, JsonSchema};
use cove_ui::compose::primitives::text::TextBuffer;
use ruwriter_core::files::{Stamp, stamp};
use serde_json::{Value, json};

const MAX_TOOL_BYTES: usize = 48 * 1024;
const CHUNK_BYTES: usize = 2100;
const CHUNK_OVERLAP: usize = 240;
const STORY_BATCH_BYTES: usize = 42 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum ResearchScope {
    #[default]
    Excerpt,
    Document,
    Project,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManuscriptFile {
    pub id: String,
    pub title: String,
    pub path: PathBuf,
}

pub struct ResearchRequest {
    pub root: PathBuf,
    pub files: Vec<ManuscriptFile>,
    pub active_id: String,
    pub active: TextBuffer,
    pub caret: usize,
}

#[derive(Clone)]
pub struct Source {
    pub id: String,
    pub title: String,
    pub file: String,
    pub first_line: usize,
    pub last_line: usize,
    pub text: String,
}

#[derive(serde::Serialize)]
pub struct StoryExcerpt {
    file: String,
    chapter: usize,
    title: String,
    offset: usize,
    text: String,
}

pub type ResearchCache = Arc<Mutex<Option<CachedIndex>>>;

pub struct CachedIndex {
    root: PathBuf,
    files: Vec<(ManuscriptFile, Option<Stamp>)>,
    active_id: String,
    active: TextBuffer,
    index: Arc<ManuscriptIndex>,
}

struct ManuscriptIndex {
    search: SearchIndex,
    documents: Vec<IndexedDocument>,
    chapters: Vec<Chapter>,
    passages: Vec<Passage>,
    skipped: usize,
}

struct IndexedDocument {
    id: String,
    text: IndexedText,
    lines: Vec<usize>,
}

enum IndexedText {
    Memory(String),
    File { path: PathBuf, fingerprint: Stamp },
}

impl IndexedText {
    fn read(&self, start: usize, end: usize) -> Option<Vec<u8>> {
        match self {
            Self::Memory(text) => Some(text.as_bytes().get(start..end)?.to_vec()),
            Self::File { path, fingerprint } => {
                if path.canonicalize().ok().as_ref() != Some(path)
                    || stamp(path) != Some(*fingerprint)
                {
                    return None;
                }
                let mut handle = std::fs::File::open(path).ok()?;
                handle.seek(SeekFrom::Start(start as u64)).ok()?;
                let mut bytes = vec![0; end - start];
                handle.read_exact(&mut bytes).ok()?;
                (stamp(path) == Some(*fingerprint)).then_some(bytes)
            }
        }
    }
}

struct Chapter {
    document: usize,
    title: String,
    number: Option<u32>,
    start: usize,
    end: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Passage {
    chapter: usize,
    start: usize,
    end: usize,
}

impl ResearchRequest {
    pub fn open(
        mut self,
        cache: &ResearchCache,
        cancelled: &AtomicBool,
    ) -> Result<ResearchRun, AgentError> {
        check_cancelled(cancelled)?;
        self.files.sort_by(|a, b| a.id.cmp(&b.id));
        let root = self
            .root
            .canonicalize()
            .map_err(|error| AgentError::Model(format!("无法读取资料库：{error}")))?;
        let files: Vec<_> = self
            .files
            .into_iter()
            .map(|file| {
                let fingerprint = (file.id != self.active_id)
                    .then(|| stamp(&file.path))
                    .flatten();
                (file, fingerprint)
            })
            .collect();
        let existing = cache.lock().ok().and_then(|cached| {
            let entry = cached.as_ref()?;
            (entry.root == root
                && entry.files == files
                && entry.active_id == self.active_id
                && entry.active == self.active)
                .then(|| entry.index.clone())
        });
        let index = if let Some(index) = existing {
            index
        } else {
            let mut index = ManuscriptIndex {
                search: SearchIndex::default(),
                documents: Vec::new(),
                chapters: Vec::new(),
                passages: Vec::new(),
                skipped: 0,
            };
            for (file, fingerprint) in &files {
                check_cancelled(cancelled)?;
                let (text, backing) = if file.id == self.active_id {
                    (self.active.string_value(), None)
                } else {
                    let Some(fingerprint) = *fingerprint else {
                        index.skipped += 1;
                        continue;
                    };
                    let Some(path) = file
                        .path
                        .canonicalize()
                        .ok()
                        .filter(|path| path.starts_with(&root))
                    else {
                        index.skipped += 1;
                        continue;
                    };
                    let Ok(bytes) = std::fs::read(&path) else {
                        index.skipped += 1;
                        continue;
                    };
                    if stamp(&path) != Some(fingerprint) {
                        index.skipped += 1;
                        continue;
                    }
                    match String::from_utf8(bytes) {
                        Ok(text) => (text, Some(IndexedText::File { path, fingerprint })),
                        Err(error) => {
                            (String::from_utf8_lossy(error.as_bytes()).into_owned(), None)
                        }
                    }
                };
                index.add(file, text, backing, cancelled)?;
            }
            let index = Arc::new(index);
            if index.skipped == 0
                && let Ok(mut cache) = cache.lock()
            {
                *cache = Some(CachedIndex {
                    root,
                    files,
                    active_id: self.active_id.clone(),
                    active: self.active.clone(),
                    index: index.clone(),
                });
            }
            index
        };
        if index.chapters.is_empty() {
            return Err(AgentError::Model(
                "选定范围内没有可供检索的文稿。".to_owned(),
            ));
        }
        let caret = self.active.utf8_offset_for_character_index(self.caret);
        let active_document = index
            .documents
            .iter()
            .position(|doc| doc.id == self.active_id);
        let current = index
            .chapters
            .iter()
            .position(|chapter| {
                index.documents[chapter.document].id == self.active_id
                    && chapter.start <= caret
                    && caret < chapter.end
            })
            .or_else(|| {
                index
                    .chapters
                    .iter()
                    .rposition(|chapter| index.documents[chapter.document].id == self.active_id)
            });
        Ok(ResearchRun {
            index,
            current,
            active_document,
            caret,
            before_caret: false,
            continuation_only: false,
            current_number: None,
            used_bytes: 0,
            evidence: Vec::new(),
        })
    }
}

/// Only explicit chapter labels establish ordering across files. Volume and
/// section labels are kept out because their numbering often restarts.
fn chapter_number(title: &str) -> Option<u32> {
    let (number, _) = title.trim().strip_prefix('第')?.split_once('章')?;
    if let Ok(number) = number.parse::<u32>() {
        return (number > 0).then_some(number);
    }
    let digit = |character| match character {
        '零' | '〇' => Some(0),
        '一' => Some(1),
        '二' | '两' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    };
    if number.chars().all(|character| digit(character).is_some()) {
        let value = number.chars().try_fold(0_u32, |sum, character| {
            sum.checked_mul(10)?.checked_add(digit(character)?)
        })?;
        return (value != 0).then_some(value);
    }
    let mut sum = 0;
    let mut pending = None;
    let mut last_unit = 10_000;
    for (index, character) in number.chars().enumerate() {
        if let Some(value) = digit(character) {
            if value == 0 && pending.is_none() {
                continue;
            }
            if value == 0 || pending.replace(value).is_some() {
                return None;
            }
            continue;
        }
        let unit = match character {
            '十' => 10,
            '百' => 100,
            '千' => 1000,
            _ => return None,
        };
        if unit >= last_unit {
            return None;
        }
        let value = pending.take().or((index == 0 && unit == 10).then_some(1))?;
        sum += value * unit;
        last_unit = unit;
    }
    let sum = sum + pending.unwrap_or(0);
    (sum > 0).then_some(sum)
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), AgentError> {
    if cancelled.load(Ordering::Relaxed) {
        Err(AgentError::Model("请求已取消。".to_owned()))
    } else {
        Ok(())
    }
}

impl ManuscriptIndex {
    fn add(
        &mut self,
        file: &ManuscriptFile,
        text: String,
        backing: Option<IndexedText>,
        cancelled: &AtomicBool,
    ) -> Result<(), AgentError> {
        if text.trim().is_empty() {
            return Ok(());
        }
        let document = self.documents.len();
        let lines: Vec<_> = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(byte, _)| byte + 1))
            .collect();
        let headings = ruwriter_core::headings(&text);
        let mut marks = Vec::with_capacity(headings.len() + 1);
        let mut offset = 0;
        let mut heading = headings.into_iter().peekable();
        for (part, &byte) in text.split_inclusive('\n').zip(&lines) {
            if heading
                .peek()
                .is_some_and(|heading| heading.offset == offset)
            {
                marks.push((byte, heading.next().unwrap().title));
            }
            offset += part.chars().count();
        }
        if marks
            .first()
            .is_none_or(|(byte, _)| !text[..*byte].trim().is_empty())
        {
            marks.insert(0, (0, file.title.clone()));
        }
        for (i, (start, title)) in marks.iter().enumerate() {
            check_cancelled(cancelled)?;
            let title: String = title.chars().take(128).collect();
            let end = marks.get(i + 1).map_or(text.len(), |(byte, _)| *byte);
            let chapter = self.chapters.len();
            self.chapters.push(Chapter {
                document,
                number: chapter_number(&title),
                title: title.clone(),
                start: *start,
                end,
            });
            let mut from = *start;
            while from < end {
                let to = text.floor_char_boundary((from + CHUNK_BYTES).min(end));
                let to = if to < end {
                    let midpoint = text.ceil_char_boundary((from + to) / 2);
                    text[midpoint..to]
                        .char_indices()
                        .rev()
                        .find(|(_, character)| {
                            matches!(character, '\n' | '。' | '！' | '？' | '；')
                        })
                        .map_or(to, |(byte, character)| {
                            midpoint + byte + character.len_utf8()
                        })
                } else {
                    to
                };
                self.search.add(&title, &text[from..to]);
                self.passages.push(Passage {
                    chapter,
                    start: from,
                    end: to,
                });
                if to == end {
                    break;
                }
                check_cancelled(cancelled)?;
                from = text.floor_char_boundary(to - CHUNK_OVERLAP);
            }
        }
        self.documents.push(IndexedDocument {
            id: file.id.clone(),
            text: backing.unwrap_or(IndexedText::Memory(text)),
            lines,
        });
        Ok(())
    }
}

#[derive(Clone)]
pub struct ResearchRun {
    index: Arc<ManuscriptIndex>,
    current: Option<usize>,
    active_document: Option<usize>,
    caret: usize,
    before_caret: bool,
    continuation_only: bool,
    current_number: Option<u32>,
    used_bytes: usize,
    evidence: Vec<(Passage, Source)>,
}

impl ResearchRun {
    pub fn restrict_to_preceding_text(&mut self) {
        self.before_caret = true;
    }

    pub fn restrict_for_continuation(&mut self) {
        self.before_caret = true;
        self.continuation_only = true;
        let mut owners = HashMap::new();
        if self
            .index
            .chapters
            .iter()
            .filter_map(|chapter| chapter.number.map(|number| (number, chapter.document)))
            .any(|(number, document)| {
                owners
                    .insert(number, document)
                    .is_some_and(|owner| owner != document)
            })
        {
            return;
        }
        self.current_number = self.current.and_then(|id| {
            let chapter = &self.index.chapters[id];
            chapter_number(self.readable_title(chapter))
        });
    }

    fn readable_end(&self, chapter: &Chapter) -> usize {
        if self.before_caret && self.active_document == Some(chapter.document) {
            chapter.end.min(self.caret)
        } else if self.continuation_only
            && !chapter
                .number
                .zip(self.current_number)
                .is_some_and(|(number, current)| number < current)
        {
            chapter.start
        } else {
            chapter.end
        }
    }

    fn readable_chapters(&self) -> usize {
        self.index
            .chapters
            .iter()
            .filter(|chapter| chapter.start < self.readable_end(chapter))
            .count()
    }

    fn readable_title<'a>(&self, chapter: &'a Chapter) -> &'a str {
        let stop = self.readable_end(chapter);
        if stop < chapter.end {
            let doc = &self.index.documents[chapter.document];
            let end_of_line = doc
                .lines
                .get(doc.lines.partition_point(|&byte| byte <= chapter.start))
                .copied()
                .unwrap_or(chapter.end);
            if stop < end_of_line {
                return "当前段落";
            }
        }
        &chapter.title
    }

    pub fn coverage(&self) -> String {
        let count = self.readable_chapters();
        let files = self.index.documents.len();
        let skipped = self.index.skipped;
        if skipped == 0 {
            format!("已索引 {files} 篇文稿、{count} 个章节。")
        } else {
            format!(
                "已索引 {files} 篇文稿、{count} 个章节，{skipped} 篇因读取失败或超出资料库边界而跳过。"
            )
        }
    }

    pub fn context(&self) -> String {
        let mut context = self.coverage();
        if let Some(current) = self.current {
            let chapter = &self.index.chapters[current];
            if chapter.start < self.readable_end(chapter) {
                context.push_str(&format!(
                    " 当前落笔位置在章节 {}（{}）。",
                    current + 1,
                    self.readable_title(chapter)
                ));
            }
        }
        context
    }

    /// Walk every readable chapter segment in order. The batch budget limits
    /// a single model call; no chapter is sampled away at this boundary.
    pub fn visit_story_batches(
        &self,
        cancelled: &AtomicBool,
        mut visit: impl FnMut(&[StoryExcerpt]) -> Result<(), AgentError>,
    ) -> Result<(), AgentError> {
        let mut batch = Vec::new();
        let mut used = 0;
        for (id, chapter) in self.index.chapters.iter().enumerate() {
            let document = &self.index.documents[chapter.document];
            let stop = self.readable_end(chapter);
            let mut start = chapter.start;
            while start < stop {
                check_cancelled(cancelled)?;
                if STORY_BATCH_BYTES - used < 4 {
                    visit(&batch)?;
                    batch.clear();
                    used = 0;
                }
                let end = (start + STORY_BATCH_BYTES - used).min(stop);
                let bytes = document.text.read(start, end).ok_or_else(|| {
                    AgentError::Model("读稿时资料发生变化，请重新梳理。".to_owned())
                })?;
                let text = match std::str::from_utf8(&bytes) {
                    Ok(text) => text,
                    Err(error) if error.error_len().is_none() => {
                        std::str::from_utf8(&bytes[..error.valid_up_to()])
                            .map_err(|_| AgentError::InvalidResult)?
                    }
                    Err(_) => return Err(AgentError::InvalidResult),
                };
                if text.is_empty() {
                    return Err(AgentError::InvalidResult);
                }
                batch.push(StoryExcerpt {
                    file: document.id.clone(),
                    chapter: id + 1,
                    title: self.readable_title(chapter).to_owned(),
                    offset: start - chapter.start,
                    text: text.to_owned(),
                });
                used += text.len();
                start += text.len();
            }
        }
        if !batch.is_empty() {
            visit(&batch)?;
        }
        Ok(())
    }

    pub fn sources(&self) -> Vec<Source> {
        self.evidence
            .iter()
            .map(|(_, source)| source.clone())
            .collect()
    }

    /// Supply a small evidence set even when the model cannot call tools.
    /// For continuation the opening evidence should come from earlier
    /// passages, beyond the immediate context already sent with the request.
    pub fn initial_evidence(&mut self, query: &str, recent_bytes: usize) -> Value {
        let previous = self.evidence.len();
        let data = self.search_passages(query, 3, recent_bytes);
        let result = self.admit(data.clone(), previous);
        if result.is_error {
            json!({"matches":[]})
        } else {
            data
        }
    }

    pub fn answer_tool(
        &mut self,
        name: &str,
        input: &Value,
    ) -> Result<AgentToolResult, AgentError> {
        let previous = self.evidence.len();
        let data = match name {
            "list_chapters" => self.list(number(input, "from", 1), number(input, "limit", 12)),
            "search_manuscript" => self.search(
                input["query"].as_str().unwrap_or_default(),
                number(input, "limit", 4),
            ),
            "read_chapter" => self.read(
                number(input, "chapter", 0),
                number(input, "offset_bytes", 0),
                number(input, "max_bytes", 3200),
            ),
            _ => return Err(AgentError::UnknownTool(name.to_owned())),
        };
        Ok(self.admit(data, previous))
    }

    pub fn answer_excerpt(&mut self, data: Value) -> AgentToolResult {
        self.admit(data, self.evidence.len())
    }

    fn admit(&mut self, data: Value, previous: usize) -> AgentToolResult {
        let result = AgentToolResult::encode(&data, data.get("error").is_some());
        if self.used_bytes + result.json.len() > MAX_TOOL_BYTES {
            self.evidence.truncate(previous);
            return AgentToolResult::encode(
                &json!({
                    "error": "本轮检索字节预算不足。可缩小读取范围；若现有证据不足，请明确说出缺口。",
                    "remaining_bytes": MAX_TOOL_BYTES - self.used_bytes,
                }),
                true,
            );
        }
        self.used_bytes += result.json.len();
        result
    }

    fn list(&self, from: usize, limit: usize) -> Value {
        let total = self.readable_chapters();
        let first = from.saturating_sub(1);
        let mut available = self
            .index
            .chapters
            .iter()
            .enumerate()
            .skip(first)
            .filter(|(_, chapter)| chapter.start < self.readable_end(chapter));
        let entries: Vec<_> = available
            .by_ref()
            .take(limit)
            .map(|(id, chapter)| {
                let doc = &self.index.documents[chapter.document];
                json!({"chapter": id + 1, "title": self.readable_title(chapter), "file": doc.id,
                    "line": doc.lines.partition_point(|&byte| byte <= chapter.start)})
            })
            .collect();
        let next = available.next().map(|(id, _)| id + 1);
        json!({"total": total, "chapters": entries, "next": next})
    }

    fn search(&mut self, query: &str, limit: usize) -> Value {
        self.search_passages(query, limit, 0)
    }

    fn search_passages(&mut self, query: &str, limit: usize, recent_bytes: usize) -> Value {
        let mut by_chapter = HashMap::<usize, usize>::new();
        let hits = self.index.search.search(query, usize::MAX);
        let mut results = Vec::new();
        for hit in hits {
            let mut passage = self.index.passages[hit];
            let chapter = &self.index.chapters[passage.chapter];
            if recent_bytes > 0
                && self.active_document == Some(chapter.document)
                && passage.end > self.caret.saturating_sub(recent_bytes)
            {
                continue;
            }
            let stop = self.readable_end(chapter);
            if passage.start >= stop {
                continue;
            }
            let clipped = passage.end > stop;
            passage.end = passage.end.min(stop);
            let count = by_chapter.entry(passage.chapter).or_default();
            if *count == 2 {
                continue;
            }
            let Some(text) = self.index.documents[chapter.document]
                .text
                .read(passage.start, passage.end)
                .and_then(|bytes| String::from_utf8(bytes).ok())
            else {
                continue;
            };
            if clipped {
                // A hit can come from the hidden half of a passage.
                let mut visible = SearchIndex::default();
                visible.add(self.readable_title(chapter), &text);
                if visible.search(query, 1).is_empty() {
                    continue;
                }
            }
            *count += 1;
            results.push(self.reference(passage, text));
            if results.len() >= limit {
                break;
            }
        }
        json!({"matches": results})
    }

    fn read(&mut self, chapter: usize, offset: usize, max: usize) -> Value {
        let Some(id) = chapter.checked_sub(1) else {
            return json!({"error": "请使用目录或检索结果中的 chapter。"});
        };
        let Some(section) = self.index.chapters.get(id) else {
            return json!({"error": "章节不存在。"});
        };
        let stop = self.readable_end(section);
        if offset >= stop.saturating_sub(section.start) {
            return json!({"error": "偏移超出本章。"});
        }
        let start = section.start + offset;
        let end_with_boundary = start.saturating_add(max + 4).min(stop);
        let backing = &self.index.documents[section.document].text;
        let Some(bytes) = backing.read(start, end_with_boundary) else {
            return json!({"error":"文稿在查阅期间发生变化，请重新生成以刷新索引。"});
        };
        let skip = bytes
            .iter()
            .take_while(|&&byte| byte & 0xc0 == 0x80)
            .count();
        let mut length = (skip + max).min(bytes.len());
        while length < bytes.len() && bytes[length] & 0xc0 == 0x80 {
            length -= 1;
        }
        let Some(text) = bytes
            .get(skip..length)
            .and_then(|part| std::str::from_utf8(part).ok())
            .filter(|text| !text.is_empty())
        else {
            return json!({"error": "偏移超出本章。"});
        };
        let next = (start + length < stop).then_some(start + length - section.start);
        let passage = Passage {
            chapter: id,
            start: start + skip,
            end: start + length,
        };
        json!({
            "passage": self.reference(passage, text.to_owned()),
            "next_offset_bytes": next,
        })
    }

    fn reference(&mut self, passage: Passage, text: String) -> Value {
        let chapter = &self.index.chapters[passage.chapter];
        let doc = &self.index.documents[chapter.document];
        let source =
            if let Some((_, source)) = self.evidence.iter().find(|(known, _)| *known == passage) {
                source
            } else {
                let source = Source {
                    id: format!("R{}", self.evidence.len() + 1),
                    title: self.readable_title(chapter).to_owned(),
                    file: doc.id.clone(),
                    first_line: doc.lines.partition_point(|&byte| byte <= passage.start),
                    last_line: doc.lines.partition_point(|&byte| byte < passage.end).max(1),
                    text,
                };
                self.evidence.push((passage, source));
                &self.evidence.last().unwrap().1
            };
        json!({
            "source": source.id, "chapter": passage.chapter + 1, "title": source.title,
            "file": source.file, "first_line": source.first_line, "last_line": source.last_line,
            "offset_bytes": passage.start - chapter.start, "text": source.text,
        })
    }
}

fn number(input: &Value, key: &str, fallback: usize) -> usize {
    input[key]
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(fallback)
}

pub fn tools() -> Vec<AgentTool> {
    let integer = |min, max| JsonSchema::integer(Some(min), Some(max));
    let schema = |fields: Vec<(&str, JsonSchema)>, required: &[&str]| {
        JsonSchema::object(
            fields
                .into_iter()
                .map(|(key, schema)| (key.to_owned(), schema))
                .collect(),
            required.iter().map(|name| (*name).to_owned()).collect(),
        )
    };
    vec![
        AgentTool::new(
            "list_chapters",
            "分页查看授权文稿的章节目录。from 从 1 起算，返回 next 时可继续。文件按库内路径排序，文件内按正文顺序。",
            schema(
                vec![
                    ("from", integer(1.0, 1_000_000.0)),
                    ("limit", integer(1.0, 20.0)),
                ],
                &[],
            ),
            true,
        ),
        AgentTool::new(
            "search_manuscript",
            "在授权文稿中按关键词查找相关原文。优先用人名、地名、物品或独特短语，可换词分次查。用返回的 source 引用，或用 chapter、offset_bytes 回读。",
            schema(
                vec![
                    ("query", JsonSchema::string(Some(1), Some(160))),
                    ("limit", integer(1.0, 6.0)),
                ],
                &["query"],
            ),
            true,
        ),
        AgentTool::new(
            "read_chapter",
            "按目录或检索返回的 chapter 读取原文。默认从章首读取一小段，也可传检索给出的 offset_bytes，或用 next_offset_bytes 继续。",
            schema(
                vec![
                    ("chapter", integer(1.0, 1_000_000.0)),
                    ("offset_bytes", integer(0.0, 9_007_199_254_740_991.0)),
                    ("max_bytes", integer(256.0, 6500.0)),
                ],
                &["chapter"],
            ),
            true,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(root: &std::path::Path, id: &str) -> ManuscriptFile {
        ManuscriptFile {
            id: id.to_owned(),
            title: id.trim_end_matches(".txt").to_owned(),
            path: root.join(id),
        }
    }

    #[test]
    fn project_continuation_uses_chapter_numbers_and_hides_future_or_unordered_files() {
        let library = tempfile::tempdir().unwrap();
        let root = library.path();
        let active = file(root, "10-当前.txt");
        let older = file(root, "99-较早.txt");
        let later = file(root, "01-较晚.txt");
        let note = file(root, "零散设定.txt");
        std::fs::write(&older.path, "第九十九章\n钥匙交给了十七。").unwrap();
        std::fs::write(&later.path, "第一百零一章\n钥匙被卖到了黑市。").unwrap();
        std::fs::write(&note.path, "钥匙将在月台失踪。").unwrap();
        let prefix = "第一百章\n天亮了。";
        let mut run = ResearchRequest {
            root: root.to_path_buf(),
            files: vec![older, later, note, active.clone()],
            active_id: active.id,
            active: TextBuffer::new(&format!("{prefix}钥匙已换了锁。")),
            caret: TextBuffer::new(prefix).character_count(),
        }
        .open(&ResearchCache::default(), &AtomicBool::new(false))
        .unwrap();
        run.restrict_for_continuation();
        assert_eq!(run.list(1, 12)["total"], 2);
        assert!(run.read(1, 0, 2048).get("error").is_some());
        let hits = run.search("钥匙", 8).to_string();
        assert!(hits.contains("钥匙交给了十七"));
        assert!(!hits.contains("黑市") && !hits.contains("月台") && !hits.contains("换了锁"));
        assert_eq!(chapter_number("第一千零二十六章 夜航"), Some(1026));
    }

    #[test]
    fn story_batches_read_the_whole_authorized_prefix_with_no_utf8_gaps() {
        let library = tempfile::tempdir().unwrap();
        let root = library.path();
        let book = file(root, "大纲.txt");
        let text = format!("第一章\n{}\n第二章\n阿雪关门，", "故事。".repeat(7_000));
        let caret = TextBuffer::new(&text).character_count();
        let active = TextBuffer::new(&format!("{text}光标之后不能发。"));
        let cancelled = AtomicBool::new(false);
        let mut run = ResearchRequest {
            root: root.to_path_buf(),
            files: vec![book.clone()],
            active_id: book.id,
            active,
            caret,
        }
        .open(&ResearchCache::default(), &cancelled)
        .unwrap();
        run.restrict_to_preceding_text();
        let mut reconstructed = String::new();
        let mut visits = 0;
        run.visit_story_batches(&cancelled, |parts| {
            visits += 1;
            for part in parts {
                reconstructed.push_str(&part.text);
            }
            Ok(())
        })
        .unwrap();
        assert!(visits > 1);
        assert_eq!(reconstructed, text);
    }

    #[test]
    fn reads_distant_evidence_in_a_thousand_chapter_book() {
        let library = tempfile::tempdir().unwrap();
        let root = library.path();
        let book = file(root, "长篇.txt");
        let mut text = String::new();
        for chapter in 1..=1000 {
            text.push_str(&format!("第{chapter}章\n"));
            text.push_str(&"沈舟走过街道，风吹动街边的灯笼。月光落在窗台。\n".repeat(52));
            if chapter == 27 {
                text.push_str("沈舟把鹰纹银匣交给晏澄。\n");
            }
        }
        let active = TextBuffer::new(&text);
        let cancelled = AtomicBool::new(false);
        let cache = ResearchCache::default();
        let request = || ResearchRequest {
            root: root.to_path_buf(),
            files: vec![book.clone()],
            active_id: book.id.clone(),
            active: active.clone(),
            caret: active.character_count(),
        };
        let mut run = request().open(&cache, &cancelled).unwrap();
        assert!(run.context().contains("章节 1000（第1000章）"));
        let page = run.list(997, 2);
        assert_eq!(page["total"], 1000);
        assert_eq!(page["chapters"][0]["title"], "第997章");
        assert_eq!(page["next"], 999);

        let initial = run.initial_evidence("鹰纹银匣", 5400);
        assert_eq!(initial["matches"][0]["chapter"], 27);
        assert_eq!(initial["matches"][0]["source"], "R1");

        let results = run.search("鹰纹银匣", 2);
        let hit = &results["matches"][0];
        assert_eq!(hit["chapter"], 27);
        assert_eq!(hit["source"], "R1");
        assert!(hit["text"].as_str().unwrap().contains("交给晏澄"));
        let line = text
            .lines()
            .position(|line| line.contains("鹰纹银匣"))
            .unwrap()
            + 1;
        assert_eq!(hit["last_line"], line);
        let read = run.read(27, hit["offset_bytes"].as_u64().unwrap() as usize, 6500);
        assert_eq!(read["passage"]["title"], "第27章");
        assert_eq!(read["passage"]["file"], book.id);
        assert_eq!(read["passage"]["last_line"], line);
        assert!(
            read["passage"]["text"]
                .as_str()
                .unwrap()
                .contains("交给晏澄")
        );

        let cached = request().open(&cache, &cancelled).unwrap();
        assert!(Arc::ptr_eq(&run.index, &cached.index));
        cancelled.store(true, Ordering::Relaxed);
        assert!(request().open(&cache, &cancelled).is_err());
    }

    #[test]
    fn refreshes_notes_and_keeps_unadmitted_evidence_private() {
        let library = tempfile::tempdir().unwrap();
        let root = library.path();
        let active_file = file(root, "正文.txt");
        let note = file(root, "设定.txt");
        std::fs::write(&note.path, "白塔只在月圆时开放。").unwrap();
        let request = |active: &TextBuffer| ResearchRequest {
            root: root.to_path_buf(),
            files: vec![note.clone(), active_file.clone()],
            active_id: active_file.id.clone(),
            active: active.clone(),
            caret: 0,
        };
        let cancelled = AtomicBool::new(false);
        let cache = ResearchCache::default();
        let active = TextBuffer::new("第一章\n旧稿");
        let original = request(&active).open(&cache, &cancelled).unwrap();
        std::fs::write(&note.path, "白塔十年才开放一次。青铜铃一直挂在塔门外。").unwrap();
        let refreshed = request(&active).open(&cache, &cancelled).unwrap();
        assert!(!Arc::ptr_eq(&original.index, &refreshed.index));
        let mut edited = request(&TextBuffer::new("第一章\n当前未保存的改动"))
            .open(&cache, &cancelled)
            .unwrap();
        assert!(!Arc::ptr_eq(&refreshed.index, &edited.index));
        assert!(
            edited.search("青铜铃", 1)["matches"][0]["text"]
                .as_str()
                .unwrap()
                .contains("白塔十年")
        );
        let count = edited.sources().len();
        edited.used_bytes = MAX_TOOL_BYTES - 1;
        let result = edited
            .answer_tool("search_manuscript", &json!({"query": "未保存"}))
            .unwrap();
        assert!(result.is_error);
        assert_eq!(result.value().unwrap()["remaining_bytes"], 1);
        assert_eq!(edited.sources().len(), count);
        edited.used_bytes = 0;
        let result = edited
            .answer_tool("search_manuscript", &json!({"query": "未保存"}))
            .unwrap();
        assert_eq!(result.value().unwrap()["matches"][0]["source"], "R2");
    }

    #[test]
    fn continuation_never_reads_past_the_active_caret_and_other_files_remain_searchable() {
        let library = tempfile::tempdir().unwrap();
        let root = library.path();
        let active = file(root, "1-正文.txt");
        let note = file(root, "2-设定.txt");
        let past = "第一章 夜\n青铜铃刚响过。";
        std::fs::write(&note.path, "白塔的铜锁始终由晏澄保管。").unwrap();
        let request = ResearchRequest {
            root: root.to_path_buf(),
            files: vec![active.clone(), note.clone()],
            active_id: active.id,
            active: TextBuffer::new(&format!(
                "{past}乌木船随后靠岸。\n第二章 雪豹密令\n未来原文。"
            )),
            caret: TextBuffer::new(past).character_count(),
        };
        let mut run = request
            .open(&ResearchCache::default(), &AtomicBool::new(false))
            .unwrap();
        run.restrict_to_preceding_text();
        assert_eq!(run.list(1, 1)["next"], 3);
        assert_eq!(run.list(1, 3)["total"], 2);
        assert!(run.list(2, 3).to_string().contains("设定"));
        assert!(!run.list(1, 3).to_string().contains("雪豹"));
        assert!(run.read(2, 0, 2000).get("error").is_some());
        assert_eq!(run.read(1, 0, 2000)["passage"]["text"], past);
        assert_eq!(run.search("乌木船", 3)["matches"], json!([]));
        assert_eq!(run.search("青铜铃", 1)["matches"][0]["text"], past);
        assert!(
            run.search("铜锁", 1)["matches"][0]["text"]
                .as_str()
                .unwrap()
                .contains("晏澄")
        );
        std::fs::write(&note.path, "此时改成了另一份设定。").unwrap();
        assert!(run.read(3, 0, 2000).get("error").is_some());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_an_indexed_path_that_resolves_outside_the_library() {
        let library = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let unrelated = other.path().join("私人笔记.txt");
        std::fs::write(&unrelated, "不可见的玉龙印").unwrap();
        let root = library.path();
        let alias = file(root, "别名.txt");
        std::os::unix::fs::symlink(&unrelated, &alias.path).unwrap();
        let active = file(root, "当前.txt");
        let request = ResearchRequest {
            root: root.to_path_buf(),
            files: vec![active.clone(), alias],
            active_id: active.id,
            active: TextBuffer::new("第一章\n留在库内。"),
            caret: 0,
        };
        let mut run = request
            .open(&ResearchCache::default(), &AtomicBool::new(false))
            .unwrap();
        assert!(
            run.coverage()
                .contains("1 篇因读取失败或超出资料库边界而跳过")
        );
        assert_eq!(run.search("玉龙印", 4)["matches"], json!([]));
    }
}
