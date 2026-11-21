//! Versioned, local writing conversations. Archive entries never authorize edits.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cove_ui::compose::primitives::text::TextBuffer;
use cove_ui::system::files::{Access, FileLock, atomic_write};
use serde::{Deserialize, Serialize};

use crate::ai::ChatTurn;

const VERSION: u32 = 1;
const MAX_IMPORT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Archive {
    version: u32,
    turns: Vec<Turn>,
}

#[derive(Serialize, Deserialize)]
struct Turn {
    id: String,
    author: String,
    answer: Option<String>,
    adopted: bool,
}

fn path(root: &Path, key: &str, extension: &str) -> PathBuf {
    let id = ruwriter_core::sync::content_hash(key);
    root.join(".ruwriter")
        .join("ai-chats")
        .join(format!("{id}.{extension}"))
}

pub fn encode(turns: &[ChatTurn]) -> io::Result<Vec<u8>> {
    let archive = Archive {
        version: VERSION,
        turns: turns
            .iter()
            .map(|turn| Turn {
                id: turn.id.clone(),
                author: turn.author.string_value(),
                answer: turn.answer.as_ref().map(TextBuffer::string_value),
                adopted: turn.adopted,
            })
            .collect(),
    };
    serde_json::to_vec_pretty(&archive).map_err(io::Error::other)
}

pub fn decode(path: &Path) -> io::Result<Vec<ChatTurn>> {
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
    Ok(archive
        .turns
        .into_iter()
        .map(|turn| ChatTurn {
            id: turn.id,
            author: TextBuffer::new(&turn.author),
            answer: turn.answer.map(|answer| TextBuffer::new(&answer)),
            adopted: turn.adopted,
        })
        .collect())
}

pub fn load(root: &Path, key: &str) -> io::Result<Vec<ChatTurn>> {
    match decode(&path(root, key, "json")) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        result => result,
    }
}

pub fn save(
    root: &Path,
    key: &str,
    turns: &[ChatTurn],
    updated_turn: Option<&str>,
) -> io::Result<()> {
    let record = path(root, key, "json");
    std::fs::create_dir_all(record.parent().expect("archive directory"))?;
    let _lock = FileLock::acquire(&path(root, key, "lock"))?;
    let mut merged = load(root, key)?;
    merge(&mut merged, turns.iter().cloned(), updated_turn);
    let bytes = encode(&merged)?;
    if bytes.len() as u64 > MAX_IMPORT_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "对话存档过大"));
    }
    atomic_write(&record, &bytes, Access::OwnerOnly)
}

pub fn merge_new(current: &mut Vec<ChatTurn>, imported: Vec<ChatTurn>) {
    merge(current, imported, None);
}

fn merge(
    current: &mut Vec<ChatTurn>,
    turns: impl IntoIterator<Item = ChatTurn>,
    updated_turn: Option<&str>,
) {
    let mut indices: std::collections::BTreeMap<_, _> = current
        .iter()
        .enumerate()
        .map(|(index, turn)| (turn.id.clone(), index))
        .collect();
    for turn in turns {
        if let Some(&index) = indices.get(&turn.id) {
            if updated_turn == Some(turn.id.as_str()) {
                current[index] = turn;
            }
        } else {
            indices.insert(turn.id.clone(), current.len());
            current.push(turn);
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

    #[test]
    fn archived_turns_reopen_and_import_without_overwriting_or_duplicating() {
        let root = tempfile::tempdir().unwrap();
        let original = ChatTurn {
            id: new_id(),
            author: TextBuffer::new("再写一段"),
            answer: Some(TextBuffer::new("她问我什么时候回来。")),
            adopted: false,
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
        let mut reopened = load(root.path(), "uid:1").unwrap();
        assert!(reopened[0].adopted);
        assert_eq!(
            reopened[0].answer.as_ref().unwrap().string_value(),
            "她问我什么时候回来。"
        );
        merge_new(&mut reopened, vec![original]);
        assert_eq!(reopened.len(), 1);
        assert!(load(root.path(), "uid:2").unwrap().is_empty());

        let path = root.path().join("unknown.json");
        std::fs::write(&path, r#"{"version":8,"turns":[]}"#).unwrap();
        assert_eq!(
            decode(&path).err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
