//! Local manuscript traversal, chapter tools and evidence for a writing run.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cove_agent::retrieval::SearchIndex;
use cove_agent::{AgentError, AgentTool, AgentToolResult, JsonSchema};
use cove_ui::compose::primitives::text::TextBuffer;
use ruwriter_core::files::{Stamp, stamp};
use serde_json::{Value, json};

const MAX_INDEX_BYTES: usize = 64 * 1024 * 1024;
const MAX_TOOL_BYTES: usize = 48 * 1024;
const CHUNK_BYTES: usize = 2100;
const CHUNK_OVERLAP: usize = 240;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
    text: String,
    lines: Vec<usize>,
}

struct Chapter {
    document: usize,
    title: String,
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
            let mut available = MAX_INDEX_BYTES;
            for (file, fingerprint) in &files {
                check_cancelled(cancelled)?;
                let text = if file.id == self.active_id {
                    if self.active.utf8_length() > available {
                        return Err(index_limit());
                    }
                    self.active.string_value()
                } else {
                    let Some(fingerprint) = fingerprint else {
                        index.skipped += 1;
                        continue;
                    };
                    if fingerprint.size > available as u64 {
                        return Err(index_limit());
                    }
                    let Some(path) = file
                        .path
                        .canonicalize()
                        .ok()
                        .filter(|path| path.starts_with(&root))
                    else {
                        index.skipped += 1;
                        continue;
                    };
                    let Ok(handle) = std::fs::File::open(path) else {
                        index.skipped += 1;
                        continue;
                    };
                    let mut bytes = Vec::new();
                    if handle
                        .take(available as u64 + 1)
                        .read_to_end(&mut bytes)
                        .is_err()
                        || stamp(&file.path) != Some(*fingerprint)
                    {
                        index.skipped += 1;
                        continue;
                    }
                    if bytes.len() > available {
                        return Err(index_limit());
                    }
                    String::from_utf8_lossy(&bytes).into_owned()
                };
                available = available.checked_sub(text.len()).ok_or_else(index_limit)?;
                index.add(file, text, cancelled)?;
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
            used_bytes: 0,
            evidence: Vec::new(),
        })
    }
}

fn index_limit() -> AgentError {
    AgentError::Model("本次本地索引超过 64 MiB，请缩小作品范围。".to_owned())
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
            text,
            lines,
        });
        Ok(())
    }
}

#[derive(Clone)]
pub struct ResearchRun {
    index: Arc<ManuscriptIndex>,
    current: Option<usize>,
    used_bytes: usize,
    evidence: Vec<(Passage, Source)>,
}

impl ResearchRun {
    pub fn coverage(&self) -> String {
        let count = self.index.chapters.len();
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
            context.push_str(&format!(
                " 当前落笔位置在章节 {}（{}）。",
                current + 1,
                chapter.title
            ));
        }
        context
    }

    pub fn sources(&self) -> Vec<Source> {
        self.evidence
            .iter()
            .map(|(_, source)| source.clone())
            .collect()
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
        let total = self.index.chapters.len();
        let first = from.saturating_sub(1);
        let entries: Vec<_> = self
            .index
            .chapters
            .iter()
            .enumerate()
            .skip(first)
            .take(limit)
            .map(|(id, chapter)| {
                let doc = &self.index.documents[chapter.document];
                json!({"chapter": id + 1, "title": chapter.title, "file": doc.id,
                    "line": doc.lines.partition_point(|&byte| byte <= chapter.start)})
            })
            .collect();
        let next = first + entries.len() + 1;
        json!({"total": total, "chapters": entries, "next": (next <= total).then_some(next)})
    }

    fn search(&mut self, query: &str, limit: usize) -> Value {
        let mut by_chapter = HashMap::<usize, usize>::new();
        let hits = self.index.search.search(query, usize::MAX);
        let mut results = Vec::new();
        for hit in hits {
            let passage = self.index.passages[hit];
            let count = by_chapter.entry(passage.chapter).or_default();
            if *count == 2 {
                continue;
            }
            *count += 1;
            results.push(self.reference(passage));
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
        if offset >= section.end - section.start {
            return json!({"error": "偏移超出本章。"});
        }
        let text = &self.index.documents[section.document].text;
        let start = text.ceil_char_boundary(section.start + offset);
        if start == section.end {
            return json!({"error": "偏移超出本章。"});
        }
        let end = text.floor_char_boundary((start + max).min(section.end));
        let next = (end < section.end).then_some(end - section.start);
        json!({
            "passage": self.reference(Passage { chapter: id, start, end }),
            "next_offset_bytes": next,
        })
    }

    fn reference(&mut self, passage: Passage) -> Value {
        let chapter = &self.index.chapters[passage.chapter];
        let doc = &self.index.documents[chapter.document];
        let source =
            if let Some((_, source)) = self.evidence.iter().find(|(known, _)| *known == passage) {
                source
            } else {
                let source = Source {
                    id: format!("R{}", self.evidence.len() + 1),
                    title: chapter.title.clone(),
                    file: doc.id.clone(),
                    first_line: doc.lines.partition_point(|&byte| byte <= passage.start),
                    last_line: doc.lines.partition_point(|&byte| byte < passage.end).max(1),
                    text: doc.text[passage.start..passage.end].to_owned(),
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
                    ("offset_bytes", integer(0.0, MAX_INDEX_BYTES as f64)),
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
