//! Versioned, local writing conversations. Archive entries never authorize edits.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cove_ui::compose::primitives::text::TextBuffer;
use cove_ui::system::files::{Access, FileLock, atomic_write};
use serde::{Deserialize, Serialize};

use crate::ai::{ChatTurn, WritingTask};
use crate::ai_scene::LocalTurn;

const VERSION: u32 = 1;
const MAX_IMPORT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Archive {
    version: u32,
    turns: Vec<Turn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timeline: Option<Timeline>,
}

#[derive(Default, Serialize, Deserialize)]
struct Timeline {
    tip: Option<String>,
    parents: BTreeMap<String, Option<String>>,
}

struct Conversation {
    turns: Vec<ChatTurn>,
    timeline: Timeline,
}

#[derive(Clone)]
pub struct Branch {
    pub id: String,
    pub title: String,
}

#[derive(Serialize, Deserialize)]
struct Turn {
    id: String,
    #[serde(default)]
    markdown: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    task: Option<WritingTask>,
    author: String,
    answer: Option<String>,
    adopted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    local: Option<LocalTurn>,
}

fn path(root: &Path, key: &str, extension: &str) -> PathBuf {
    let id = ruwriter_core::sync::content_hash(key);
    root.join(".ruwriter")
        .join("ai-chats")
        .join(format!("{id}.{extension}"))
}

pub fn encode(turns: &[ChatTurn]) -> io::Result<Vec<u8>> {
    encode_archive(turns, None)
}

fn encode_archive(turns: &[ChatTurn], timeline: Option<&Timeline>) -> io::Result<Vec<u8>> {
    #[derive(Serialize)]
    struct Out<'a> {
        version: u32,
        turns: Vec<Turn>,
        #[serde(skip_serializing_if = "Option::is_none")]
        timeline: Option<&'a Timeline>,
    }
    let retain_local = timeline.is_some();
    let archive = Out {
        version: VERSION,
        turns: turns
            .iter()
            .map(|turn| Turn {
                id: turn.id.clone(),
                markdown: turn.markdown,
                task: turn.task,
                author: turn.author.string_value(),
                answer: turn.answer.as_ref().map(TextBuffer::string_value),
                adopted: turn.adopted,
                local: if retain_local {
                    turn.local.clone()
                } else {
                    None
                },
            })
            .collect(),
        timeline,
    };
    serde_json::to_vec_pretty(&archive).map_err(io::Error::other)
}

pub fn decode(path: &Path) -> io::Result<Vec<ChatTurn>> {
    Ok(decode_archive(path, false)?.turns)
}

fn decode_archive(path: &Path, retain_local: bool) -> io::Result<Conversation> {
    let mut file = std::fs::File::open(path)?.take(MAX_IMPORT_BYTES + 1);
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    if data.len() as u64 > MAX_IMPORT_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "对话存档过大"));
    }
    let archive: Archive = serde_json::from_slice(&data).map_err(io::Error::other)?;
    if archive.version != VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "不支持此对话存档版本",
        ));
    }
    let mut ids = std::collections::BTreeSet::new();
    if archive.turns.iter().any(|turn| {
        turn.id.is_empty()
            || turn.id.len() > 128
            || !turn
                .id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'-')
            || !ids.insert(turn.id.as_str())
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "对话轮次 ID 无效或重复",
        ));
    }
    let turns: Vec<_> = archive
        .turns
        .into_iter()
        .map(|turn| ChatTurn {
            id: turn.id,
            markdown: turn.markdown,
            task: turn.task,
            author: TextBuffer::new(&turn.author),
            answer: turn.answer.map(|answer| TextBuffer::new(&answer)),
            adopted: turn.adopted,
            local: if retain_local { turn.local } else { None },
        })
        .collect();
    let timeline = if retain_local { archive.timeline } else { None }
        .unwrap_or_else(|| Timeline::linear(&turns));
    timeline.validate(&turns)?;
    Ok(Conversation { turns, timeline })
}

pub fn load(root: &Path, key: &str) -> io::Result<Vec<ChatTurn>> {
    Ok(load_all(root, key)?.visible())
}

fn load_all(root: &Path, key: &str) -> io::Result<Conversation> {
    match decode_archive(&path(root, key, "json"), true) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Conversation {
            turns: Vec::new(),
            timeline: Timeline::default(),
        }),
        result => result,
    }
}

pub fn save(
    root: &Path,
    key: &str,
    turns: &[ChatTurn],
    updated_turn: Option<&str>,
) -> io::Result<()> {
    update(root, key, |conversation| {
        conversation.merge(turns, updated_turn);
        Ok(())
    })
}

pub fn branches(root: &Path, key: &str) -> io::Result<Vec<Branch>> {
    let conversation = load_all(root, key)?;
    let parents: BTreeSet<_> = conversation.timeline.parents.values().flatten().collect();
    Ok(conversation
        .turns
        .iter()
        .rev()
        .filter(|turn| {
            !parents.contains(&turn.id) && conversation.timeline.tip.as_ref() != Some(&turn.id)
        })
        .map(|turn| Branch {
            id: turn.id.clone(),
            title: turn_title(turn),
        })
        .collect())
}

pub fn turn_title(turn: &ChatTurn) -> String {
    let author = turn.author.string_value();
    let title: String = author
        .trim()
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(60)
        .collect();
    if title.is_empty() {
        "写作请求".to_owned()
    } else {
        title
    }
}

pub fn move_tip(root: &Path, key: &str, tip: Option<&str>) -> io::Result<Vec<ChatTurn>> {
    update(root, key, |conversation| {
        if tip.is_some_and(|id| !conversation.timeline.parents.contains_key(id)) {
            return Err(io::Error::new(io::ErrorKind::NotFound, "找不到这轮对话。"));
        }
        conversation.timeline.tip = tip.map(str::to_owned);
        Ok(conversation.visible())
    })
}

fn update<R>(
    root: &Path,
    key: &str,
    change: impl FnOnce(&mut Conversation) -> io::Result<R>,
) -> io::Result<R> {
    let record = path(root, key, "json");
    std::fs::create_dir_all(record.parent().expect("archive directory"))?;
    let _lock = FileLock::acquire(&path(root, key, "lock"))?;
    let mut conversation = load_all(root, key)?;
    let result = change(&mut conversation)?;
    let bytes = encode_archive(&conversation.turns, Some(&conversation.timeline))?;
    if bytes.len() as u64 > MAX_IMPORT_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "对话存档过大"));
    }
    atomic_write(&record, &bytes, Access::OwnerOnly)?;
    Ok(result)
}

impl Timeline {
    fn linear(turns: &[ChatTurn]) -> Self {
        let mut timeline = Self::default();
        for turn in turns {
            timeline
                .parents
                .insert(turn.id.clone(), timeline.tip.replace(turn.id.clone()));
        }
        timeline
    }

    fn validate(&self, turns: &[ChatTurn]) -> io::Result<()> {
        let mut seen = BTreeSet::new();
        if self.parents.len() != turns.len()
            || turns.iter().any(|turn| {
                let found = self.parents.get(&turn.id).is_some_and(|parent| {
                    parent.as_ref().is_none_or(|parent| seen.contains(parent))
                });
                seen.insert(&turn.id);
                !found
            })
            || self.tip.as_ref().is_some_and(|tip| !seen.contains(tip))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "对话分支存档无效。",
            ));
        }
        Ok(())
    }
}

impl Conversation {
    fn visible(&self) -> Vec<ChatTurn> {
        let mut wanted = BTreeSet::new();
        let mut cursor = self.timeline.tip.as_ref();
        while let Some(id) = cursor {
            wanted.insert(id);
            cursor = self.timeline.parents[id].as_ref();
        }
        self.turns
            .iter()
            .filter(|turn| wanted.contains(&turn.id))
            .cloned()
            .collect()
    }

    fn merge(&mut self, turns: &[ChatTurn], updated_turn: Option<&str>) {
        let mut parent = if updated_turn.is_some() {
            None
        } else {
            self.timeline.tip.clone()
        };
        let mut indices: BTreeMap<_, _> = self
            .turns
            .iter()
            .enumerate()
            .map(|(index, turn)| (turn.id.clone(), index))
            .collect();
        for turn in turns {
            if let Some(&index) = indices.get(&turn.id) {
                if updated_turn == Some(turn.id.as_str()) {
                    self.turns[index] = turn.clone();
                }
            } else {
                indices.insert(turn.id.clone(), self.turns.len());
                self.turns.push(turn.clone());
                self.timeline
                    .parents
                    .insert(turn.id.clone(), parent.clone());
                if self.timeline.tip == parent {
                    self.timeline.tip = Some(turn.id.clone());
                }
            }
            if updated_turn.is_some() || self.timeline.tip.as_ref() == Some(&turn.id) {
                parent = Some(turn.id.clone());
            }
        }
    }
}

pub fn new_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!(
        "{}-{}-{}",
        at.as_nanos(),
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

pub fn load_story(root: &Path, key: &str) -> io::Result<Option<crate::ai_story::StoryDraft>> {
    let record = path(root, key, "story.json");
    let data = match std::fs::read(record) {
        Ok(data) => data,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    serde_json::from_slice(&data)
        .map(Some)
        .map_err(io::Error::other)
}

pub fn save_story(root: &Path, key: &str, draft: &crate::ai_story::StoryDraft) -> io::Result<()> {
    let record = path(root, key, "story.json");
    std::fs::create_dir_all(record.parent().expect("archive directory"))?;
    atomic_write(
        &record,
        &serde_json::to_vec(draft).map_err(io::Error::other)?,
        Access::OwnerOnly,
    )
}

pub fn clear_story(root: &Path, key: &str) -> io::Result<()> {
    match std::fs::remove_file(path(root, key, "story.json")) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::WritingTarget;
    use crate::ai_continuation::SceneQuestion;
    use crate::ai_research::ResearchScope;
    use crate::ai_scene::{PendingScene, SceneAnchor, SceneAnswer, append_answers, pending};

    #[test]
    fn rolling_back_forks_reversibly_and_late_saves_preserve_the_selected_branch() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let key = "uid:1";
        let turn = |id: &str, title: &str| ChatTurn {
            id: id.into(),
            markdown: true,
            task: None,
            author: TextBuffer::new(title),
            answer: Some(TextBuffer::new("一轮答复")),
            adopted: false,
            local: None,
        };
        let first = turn("1", "第一轮");
        let previous = turn("2", "旧方向");
        let branch = turn("3", "新方向");
        let legacy = path(root, key, "json");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, encode(&[first.clone(), previous.clone()]).unwrap()).unwrap();
        assert_eq!(load(root, key).unwrap().len(), 2);

        assert_eq!(move_tip(root, key, Some("1")).unwrap().len(), 1);
        save(root, key, &[first.clone(), branch], Some("3")).unwrap();
        assert_eq!(load(root, key).unwrap().last().unwrap().id, "3");
        assert_eq!(branches(root, key).unwrap()[0].id, "2");

        let updated = ChatTurn {
            adopted: true,
            ..previous
        };
        save(root, key, &[first, updated], Some("2")).unwrap();
        assert_eq!(load(root, key).unwrap().last().unwrap().id, "3");
        assert!(
            move_tip(root, key, Some("2"))
                .unwrap()
                .last()
                .unwrap()
                .adopted
        );
        assert_eq!(branches(root, key).unwrap()[0].id, "3");
        assert!(move_tip(root, key, None).unwrap().is_empty());
        assert!(load(root, key).unwrap().is_empty());
        assert_eq!(branches(root, key).unwrap().len(), 2);
    }

    #[test]
    fn restores_pending_questions_and_reuses_only_locally_confirmed_answers_at_the_same_anchor() {
        let root = tempfile::tempdir().unwrap();
        let target = WritingTarget {
            document: (root.path().to_owned(), "chapter.txt".into(), 1),
            buffer: TextBuffer::new("她合上了平板。\n接着把杯子放到桌上。"),
            selection: Some(1..7),
            insertion: 7,
        };
        let anchor = SceneAnchor::capture(&target).unwrap();
        let question = SceneQuestion {
            quote: "合上了平板".into(),
            question: "她是在犹豫，还是想结束话题？".into(),
        };
        let waiting = ChatTurn {
            id: new_id(),
            markdown: false,
            task: None,
            author: TextBuffer::new("只接一个动作"),
            answer: Some(TextBuffer::new(&question.question)),
            adopted: false,
            local: Some(LocalTurn {
                pending: Some(PendingScene {
                    anchor: anchor.clone(),
                    instruction: "只接一个动作".into(),
                    question: question.clone(),
                    scope: ResearchScope::Project,
                }),
                ..Default::default()
            }),
        };
        save(root.path(), "uid:1", std::slice::from_ref(&waiting), None).unwrap();
        let loaded = load(root.path(), "uid:1").unwrap();
        let (_, saved) = pending(&loaded).unwrap();
        assert_eq!(saved.scope, ResearchScope::Project);
        assert_eq!(saved.instruction, "只接一个动作");
        let reopened = WritingTarget {
            document: (root.path().to_owned(), "chapter.txt".into(), 8),
            selection: None,
            insertion: 0,
            ..target.clone()
        };
        let restored = saved.anchor.restore(&reopened).unwrap();
        assert_eq!(restored.selection, Some(1..7));
        let edited = WritingTarget {
            buffer: TextBuffer::new("她压住了平板。\n接着把杯子放到桌上。"),
            ..reopened.clone()
        };
        assert!(saved.anchor.restore(&edited).is_none());
        let answer = ChatTurn {
            id: new_id(),
            author: TextBuffer::new("是犹豫，她还想继续聊。"),
            answer: None,
            local: Some(LocalTurn {
                scene_answer: Some(SceneAnswer { anchor, question }),
                ..Default::default()
            }),
            ..waiting
        };
        save(root.path(), "uid:1", std::slice::from_ref(&answer), None).unwrap();
        let loaded = load(root.path(), "uid:1").unwrap();
        assert!(pending(&loaded).is_none());
        assert!(append_answers("再试一版", Some(&restored), &loaded).contains("她还想继续聊"));
        assert_eq!(
            append_answers("再试一版", Some(&edited), &loaded),
            "再试一版"
        );
        assert_eq!(
            append_answers("再试一版", Some(&reopened), &loaded),
            "再试一版"
        );

        let exported = root.path().join("export.json");
        let bytes = encode(&loaded).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("\"local\""));
        std::fs::write(&exported, bytes).unwrap();
        assert!(
            decode(&exported)
                .unwrap()
                .iter()
                .all(|turn| turn.local.is_none())
        );
        // Opening even a copied local archive via Import must strip its authority.
        let imported = decode(&path(root.path(), "uid:1", "json")).unwrap();
        assert_eq!(
            append_answers("再试一版", Some(&restored), &imported),
            "再试一版"
        );
    }

    #[test]
    fn archived_turns_reopen_and_import_without_overwriting_or_duplicating() {
        let root = tempfile::tempdir().unwrap();
        let original = ChatTurn {
            id: new_id(),
            markdown: true,
            task: None,
            author: TextBuffer::new("再写一段"),
            answer: Some(TextBuffer::new("她问我什么时候回来。")),
            adopted: false,
            local: Some(LocalTurn::default()),
        };
        let updated = ChatTurn {
            adopted: true,
            ..original.clone()
        };
        save(root.path(), "uid:1", std::slice::from_ref(&original), None).unwrap();
        save(
            root.path(),
            "uid:1",
            std::slice::from_ref(&updated),
            Some(&updated.id),
        )
        .unwrap();
        save(root.path(), "uid:1", std::slice::from_ref(&original), None).unwrap();
        let reopened = load(root.path(), "uid:1").unwrap();
        assert!(reopened[0].adopted);
        assert!(reopened[0].markdown);
        assert_eq!(
            reopened[0].answer.as_ref().unwrap().string_value(),
            "她问我什么时候回来。"
        );
        save(root.path(), "uid:1", &[original], None).unwrap();
        assert_eq!(load(root.path(), "uid:1").unwrap().len(), 1);
        assert!(load(root.path(), "uid:2").unwrap().is_empty());

        let path = root.path().join("unknown.json");
        std::fs::write(&path, r#"{"version":8,"turns":[]}"#).unwrap();
        assert_eq!(
            decode(&path).err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
