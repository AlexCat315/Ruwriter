//! Author-approved changes to the current work's writing brief.

use std::path::PathBuf;

use cove_agent::{AgentTool, JsonSchema};
use ruwriter_core::WritingBrief;
use serde::Deserialize;
use serde_json::{Value, json};

pub const APPLIED: &str = "已保存到作品资料";
pub const DECLINED: &str = "作者保留原样";

#[derive(Clone)]
pub struct WorkMaterial {
    pub key: (PathBuf, String),
    pub brief: WritingBrief,
}

#[derive(Clone)]
pub struct WorkChange {
    pub base: WorkMaterial,
    pub updated: WritingBrief,
    pub previews: Vec<ChangePreview>,
}

#[derive(Clone)]
pub struct ChangePreview {
    pub label: String,
    pub before: String,
    pub after: String,
}

pub struct OutlineSection<'a> {
    pub title: &'a str,
    pub range: std::ops::Range<usize>,
}

pub fn outline_sections(text: &str, chapters: bool) -> Vec<OutlineSection<'_>> {
    let mut sections: Vec<OutlineSection<'_>> = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let heading = line.trim().trim_start_matches('#').trim_start();
        if let Some(numbered) = heading.strip_prefix('第')
            && let Some((end, kind)) = numbered.char_indices().find(|(_, ch)| {
                !ch.is_whitespace()
                    && !matches!(ch, '0'..='9' | '０'..='９' | '零' | '〇' | '一' | '二' | '三'
                        | '四' | '五' | '六' | '七' | '八' | '九' | '十' | '百' | '千' | '万' | '两')
            })
            && !numbered[..end].trim().is_empty()
            && if chapters { matches!(kind, '章' | '节' | '回' | '话') } else { matches!(kind, '卷' | '部' | '篇') }
        {
            if let Some(previous) = sections.last_mut() {
                previous.range.end = offset;
            }
            let label_end = '第'.len_utf8() + end + kind.len_utf8();
            // Imported outlines often put a title and the plot on the same line.
            let tail = heading[label_end..].trim_start_matches(['：', ':', ' ', '\t']);
            let title_end = if chapters {
                heading.len()
            } else {
                tail.split_whitespace().next().map_or(label_end, |title| {
                    heading.len() - tail.len() + title.len()
                })
            };
            sections.push(OutlineSection { title: &heading[..title_end], range: offset..text.len() });
        }
        offset += line.len();
    }
    sections
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Field {
    Outline,
    DetailedOutline,
    CharacterCards,
    Relationships,
    Memories,
    Feedback,
    Genre,
    Tone,
    Direction,
    Canon,
    Voice,
    Avoid,
    OpenThreads,
    CharacterClaims,
}

impl Field {
    fn key(self) -> &'static str {
        match self {
            Self::Outline => "outline",
            Self::DetailedOutline => "detailed_outline",
            Self::CharacterCards => "character_cards",
            Self::Relationships => "relationships",
            Self::Memories => "memories",
            Self::Feedback => "feedback",
            Self::Genre => "genre",
            Self::Tone => "tone",
            Self::Direction => "direction",
            Self::Canon => "canon",
            Self::Voice => "voice",
            Self::Avoid => "avoid",
            Self::OpenThreads => "open_threads",
            Self::CharacterClaims => "character_claims",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Outline => "全书大纲",
            Self::DetailedOutline => "章节细纲",
            Self::CharacterCards => "角色卡",
            Self::Relationships => "人物关系",
            Self::Memories => "记忆",
            Self::Feedback => "DiffData",
            Self::Genre => "题材",
            Self::Tone => "叙事口味",
            Self::Direction => "发展方向",
            Self::Canon => "作品设定",
            Self::Voice => "文风",
            Self::Avoid => "避用写法",
            Self::OpenThreads => "待续线索",
            Self::CharacterClaims => "角色说法",
        }
    }

    fn in_story(self) -> bool {
        !matches!(
            self,
            Self::Memories | Self::Feedback | Self::Canon | Self::Voice | Self::Avoid
        )
    }

    fn has_optional_field(self, key: &str) -> bool {
        matches!(self, Self::Relationships)
            && matches!(
                key,
                "evidence" | "from_knows" | "to_knows" | "reveal_condition"
            )
    }

    fn record(self) -> bool {
        matches!(
            self,
            Self::CharacterCards | Self::Relationships | Self::Memories | Self::Feedback
        )
    }
}

#[derive(Deserialize)]
struct Read {
    field: Field,
    #[serde(default)]
    start: usize,
}

#[derive(Deserialize)]
struct Proposal {
    edits: Vec<Edit>,
}

#[derive(Deserialize)]
struct Edit {
    field: Field,
    index: Option<usize>,
    before: Option<String>,
    after: Value,
}

pub fn is_edit_request(prompt: &str) -> bool {
    [
        "作品资料",
        "大纲",
        "细纲",
        "人设",
        "角色卡",
        "角色设定",
        "人物关系",
        "记忆",
        "diffdata",
    ]
    .iter()
    .any(|field| prompt.to_lowercase().contains(field))
        && !crate::ai::is_continuation_command(prompt)
        && !["按", "根据", "照着", "参考", "润色"]
            .iter()
            .any(|command| prompt.trim_start().starts_with(command))
        && [
            "修改",
            "改一下",
            "改成",
            "改大纲",
            "改细纲",
            "改人设",
            "改角色",
            "更新",
            "整理",
            "补",
            "调整",
            "新增",
            "添加",
            "删除",
            "记入",
            "记下",
            "记住",
            "学一下",
            "删掉",
            "填到",
            "加到",
            "写进",
            "加入",
            "保存",
        ]
        .iter()
        .any(|verb| prompt.contains(verb))
}

pub fn missing_story(brief: &WritingBrief, chapter: Option<&str>) -> Vec<&'static str> {
    let story = &brief.story;
    let mut missing = Vec::new();
    for (value, label) in [
        (&story.outline, "全书大纲（主线、分卷与关键转折）"),
        (
            &story.detailed_outline,
            "当前章节细纲（场景顺序、冲突、结果与下一场承接）",
        ),
        (&story.genre, "题材"),
        (&story.tone, "叙事口味"),
        (&story.direction, "发展方向"),
    ] {
        let value = value.trim();
        if value.is_empty() || matches!(value, "待定" | "待补" | "暂无" | "TODO") {
            missing.push(label);
        }
    }
    let chapter_number = crate::ai_research::chapter_number;
    if let Some(number) = chapter.and_then(chapter_number) {
        let sections = outline_sections(&story.detailed_outline, true);
        let listed: Vec<_> = sections
            .iter()
            .filter_map(|section| Some((chapter_number(section.title)?, section)))
            .collect();
        if !listed.is_empty() {
            let has_scenes = listed.iter().any(|(chapter, section)| {
                *chapter == number
                    && story.detailed_outline[section.range.clone()]
                        .split_once('\n')
                        .is_some_and(|(_, body)| !body.trim().is_empty())
            });
            if !has_scenes {
                missing.push("当前章节在细纲中的对应场景");
            }
        }
    }
    if !story.has_character_cards()
        || story
            .character_cards
            .iter()
            .any(|card| card.defense.trim().is_empty())
    {
        missing.push("主要人物角色卡与关系（动机、防御和关系边界）");
    } else if story.character_cards.len() > 1 && story.relationships.is_empty() {
        missing.push("主要人物关系（现状、走向和边界）");
    }
    missing
}

pub const INSTRUCTIONS: &str = r#"你可以通过 read_work_material 读取作者工作资料，通过 propose_work_changes 提交真正可保存的更改。当作者要求修改大纲、细纲、人设、关系、记忆、DiffData 或文风时，先读相关字段、核对本轮谈话，必须调用提案工具，不能只在对话里说已经记住。当前草稿修改和作品级约定有不同作用范围；作者没明确哪种、或核心内容不明确时先 ask_user。对原有作品只改所需字句，不覆盖作者已经确定的分卷主线和写法。修改人物关系时，state 写作者认可的已发生行为或结果，evidence 可附具体事件或原句，from_knows 和 to_knows 各写一方知道的内容；有作者给出的限制时用 boundary 写当前不能提前发生的具体事件，用 reveal_condition 写什么条件满足后可发生，不从一个眼神或暧昧对白推断人物已相爱或应怎样发展。章节细纲是作者同意的具体事件、顺序、冲突、结果及承接，不从正文摘要推导为正式细纲。记忆须区分事实、角色说法、原文笔记和写法。DiffData 需要明确的改前改后或作者明确的写作指令；没有证据就先核对。提案确认后以工具结果为准。"#;

pub fn tools() -> [AgentTool; 2] {
    let fields = "outline 全书大纲，detailed_outline 章节细纲，character_cards 角色卡，relationships 关系，memories 记忆，feedback DiffData，genre 题材，tone 叙事口味，direction 走向，canon 设定，voice 文风，avoid 避用写法，open_threads 线索，character_claims 角色说法。";
    let read = JsonSchema::object(
        [
            ("field".into(), JsonSchema::string(None, None)),
            ("start".into(), JsonSchema::integer(Some(0.0), None)),
        ]
        .into_iter()
        .collect(),
        vec!["field".into()],
    );
    let edit = JsonSchema::object(
        [
            ("field".into(), JsonSchema::string(None, None)),
            ("index".into(), JsonSchema::integer(Some(1.0), None)),
            ("before".into(), JsonSchema::string(None, None)),
            ("after".into(), JsonSchema::any()),
        ]
        .into_iter()
        .collect(),
        vec!["field".into(), "after".into()],
    );
    let propose = JsonSchema::object(
        [("edits".into(), JsonSchema::array_of(edit, Some(12)))]
            .into_iter()
            .collect(),
        vec!["edits".into()],
    );
    [
        AgentTool::new(
            "read_work_material",
            &format!(
                "读取作品资料。{fields} start 默认 0。列表一页最多 12 条，返回的 index 为一基序号。长文本按字符分页。"
            ),
            read,
            true,
        ),
        AgentTool::new(
            "propose_work_changes",
            &format!(
                "提交作者确认。{fields} 文本字段用 before 的唯一原句定位，初次填写空字段用空字符串；after 是替换文本。列表用 index 定位，after 是需修改的字段对象；不传 index 表示追加，after 为 null 且指定 index 表示删除。多个删除按序号从大到小。"
            ),
            propose,
            false,
        ),
    ]
}

impl WorkMaterial {
    fn json(&self) -> Value {
        let mut brief = self.brief.clone();
        brief.story = brief.story.public_view();
        serde_json::to_value(brief).expect("serializable writing brief")
    }

    pub fn read(&self, input: Value) -> Result<Value, String> {
        let Read { field, start } =
            serde_json::from_value(input).map_err(|_| "请选择要读取的资料字段。")?;
        let brief = self.json();
        let value = if field.in_story() {
            &brief["story"][field.key()]
        } else {
            &brief[field.key()]
        };
        if field.record() {
            let entries = value.as_array().map(Vec::as_slice).unwrap_or_default();
            let items: Vec<_> = entries
                .iter()
                .enumerate()
                .skip(start)
                .take(12)
                .map(|(index, value)| json!({"index": index + 1, "value": value}))
                .collect();
            return Ok(
                json!({"items": items, "total": entries.len(), "next": (start.saturating_add(12) < entries.len()).then(|| start + 12)}),
            );
        }
        let text = value.as_str().unwrap_or_default();
        let count = text.chars().count();
        let excerpt: String = text.chars().skip(start).take(8_000).collect();
        Ok(
            json!({"text": excerpt, "total": count, "next": (start.saturating_add(8_000) < count).then(|| start + 8_000)}),
        )
    }

    pub fn propose(&self, input: Value) -> Result<WorkChange, String> {
        let Proposal { edits } = serde_json::from_value(input).map_err(|_| "提案格式有误。")?;
        if !(1..=12).contains(&edits.len()) {
            return Err("每份提案需要 1 到 12 项具体改动。".into());
        }
        let mut updated = self.json();
        if !updated["story"].is_object() {
            updated["story"] = json!({});
        }
        let mut previews = Vec::new();
        for edit in edits {
            let container = if edit.field.in_story() {
                &mut updated["story"]
            } else {
                &mut updated
            };
            let key = edit.field.key();
            previews.push(edit.apply(&mut container[key])?);
        }
        let mut updated: WritingBrief =
            serde_json::from_value(updated).map_err(|_| "有一项的字段类型不匹配。")?;
        updated
            .story
            .author_truth
            .clone_from(&self.brief.story.author_truth);
        if updated == self.brief {
            return Err("提案没有实质变化。".into());
        }
        validate(&updated)?;
        Ok(WorkChange {
            base: self.clone(),
            updated,
            previews,
        })
    }
}

impl Edit {
    fn apply(self, value: &mut Value) -> Result<ChangePreview, String> {
        let label = self.field.label();
        if !self.field.record() {
            let current = value.as_str().unwrap_or_default();
            let before = self.before.ok_or("请提供逐字一致的原句。")?;
            let after = self.after.as_str().ok_or("这项资料需要文本。")?;
            let revised = if current.is_empty() && before.is_empty() {
                after.to_owned()
            } else {
                cove_agent::revision::apply_replacements(current, [(before.as_str(), after)])
                    .ok_or("原句定位失败，请重新读取资料再定位。")?
            };
            *value = json!(revised);
            return Ok(ChangePreview {
                label: label.into(),
                before,
                after: after.to_owned(),
            });
        }
        if value.is_null() {
            *value = json!([]);
        }
        let records = value.as_array_mut().ok_or("资料列表格式错误。")?;
        if let Some(index) = self.index {
            let offset = index
                .checked_sub(1)
                .filter(|&i| i < records.len())
                .ok_or("资料序号已经失效。")?;
            let before = describe(&records[offset]);
            if self.after.is_null() {
                records.remove(offset);
                return Ok(ChangePreview {
                    label: format!("{label} · 第 {index} 项"),
                    before,
                    after: String::new(),
                });
            }
            let patch = self.after.as_object().ok_or("请列出要修改的记录字段。")?;
            let record = records[offset].as_object_mut().ok_or("记录格式错误。")?;
            if patch
                .keys()
                .any(|key| !record.contains_key(key) && !self.field.has_optional_field(key))
            {
                return Err("改动包含未知字段，请先读取这项资料。".into());
            }
            record.extend(patch.clone());
            let after = describe(&records[offset]);
            Ok(ChangePreview {
                label: format!("{label} · 第 {index} 项"),
                before,
                after,
            })
        } else {
            let fields = self.after.as_object().ok_or("请提供新增记录的字段。")?;
            let kind = match self.field {
                Field::CharacterCards => json!(ruwriter_core::CharacterCard::default()),
                Field::Relationships => json!(ruwriter_core::RelationshipCard::default()),
                Field::Memories => json!(ruwriter_core::WritingMemory::default()),
                Field::Feedback => json!(ruwriter_core::WritingFeedback::default()),
                _ => unreachable!(),
            };
            if fields
                .keys()
                .any(|key| kind.get(key).is_none() && !self.field.has_optional_field(key))
            {
                return Err("新增记录包含未知字段，请先读取同类资料。".into());
            }
            let after = describe(&self.after);
            records.push(self.after);
            Ok(ChangePreview {
                label: format!("新增{label}"),
                before: String::new(),
                after,
            })
        }
    }
}

fn describe(record: &Value) -> String {
    record
        .as_object()
        .into_iter()
        .flat_map(|fields| fields.iter())
        .filter_map(|(key, value)| {
            let label = match key.as_str() {
                "name" => "姓名",
                "surface" => "表面",
                "motive" => "动机",
                "defense" => "防御",
                "boundary" => "边界",
                "from" => "相关人物",
                "to" => "另一方",
                "state" => "现状",
                "evidence" => "依据的事件或原句",
                "from_knows" => "前者知道",
                "to_knows" => "后者知道",
                "reveal_condition" => "变化或揭晓的条件",
                "direction" => "走向",
                "title" => "标题",
                "subject" => "适用范围",
                "text" => "内容",
                "source" => "出处",
                "kind" => "类型",
                "pinned" => "置顶",
                "held" => "暂停使用",
                "instruction" => "作者要求",
                "before" => "改前",
                "after" => "改后",
                "lesson" => "改法",
                _ => return None,
            };
            let displayed = match value {
                Value::String(value) if !value.trim().is_empty() => match value.as_str() {
                    "note" => "原文摘记",
                    "fact" => "已确认事实",
                    "character_claim" => "角色说过的话",
                    "craft" => "写法经验",
                    _ => value,
                },
                Value::Bool(true) => "是",
                Value::Bool(false) => "否",
                _ => return None,
            };
            Some(format!("{label}　{displayed}"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn validate(brief: &WritingBrief) -> Result<(), String> {
    let names: std::collections::BTreeSet<_> = brief
        .story
        .character_cards
        .iter()
        .map(|card| card.name.trim())
        .collect();
    if names.contains("") || names.len() != brief.story.character_cards.len() {
        return Err("角色卡需要唯一姓名。".into());
    }
    if brief.story.relationships.iter().any(|edge| {
        let from = edge.from.trim();
        let to = edge.to.trim();
        from == to || !names.contains(from) || !names.contains(to)
    }) {
        return Err("人物关系需要连接两张已有的角色卡。".into());
    }
    if brief
        .memories
        .iter()
        .any(|entry| entry.text.trim().is_empty())
        || brief.feedback.iter().any(|entry| {
            entry.instruction.trim().is_empty()
                && entry.lesson.trim().is_empty()
                && entry.before == entry.after
        })
    {
        return Err("请补齐记忆内容与 DiffData 的依据。".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapter_readiness_requires_the_current_chapter_in_a_numbered_outline() {
        let mut brief = WritingBrief::default();
        let story = &mut brief.story;
        story.outline = "第一卷：她第一次出海。".into();
        story.genre = "海洋科幻".into();
        story.tone = "第一人称".into();
        story.direction = "找回船员".into();
        story.character_cards.push(ruwriter_core::CharacterCard {
            name: "阿青".into(),
            motive: "带回失散的人".into(),
            defense: "越担心越不肯开口".into(),
            ..Default::default()
        });
        assert_eq!(missing_story(&brief, Some("第 14章")).len(), 1);
        brief.story.detailed_outline = "第十三章\n她录下灯塔的信号。".into();
        assert_eq!(
            missing_story(&brief, Some("第 14章")),
            ["当前章节在细纲中的对应场景"]
        );
        brief
            .story
            .detailed_outline
            .push_str("\n第十四章\n她在水底听见灯塔的回声，并决定沿缆索往前。");
        assert!(missing_story(&brief, Some("第 14章")).is_empty());
    }

    #[test]
    fn proposal_changes_only_selected_fields_and_preserves_author_truth() {
        let mut brief = WritingBrief::default();
        brief.story.outline = "第一卷：分开。\n\n第二卷：重逢。".into();
        brief.story.author_truth = "她撒过谎。".into();
        brief
            .story
            .character_cards
            .push(ruwriter_core::CharacterCard {
                name: "阿青".into(),
                defense: "沉默".into(),
                motive: "带回失散的人".into(),
                ..Default::default()
            });
        let work = WorkMaterial {
            key: (PathBuf::from("library"), "work".into()),
            brief,
        };
        let proposal = work.propose(json!({"edits":[
            {"field":"outline","before":"第二卷：重逢。","after":"第二卷：回乡后重逢。"},
            {"field":"character_cards","index":1,"after":{"defense":"会岔开话题"}},
            {"field":"memories","after":{"title":"角色说法","text":"她说她忘了","kind":"character_claim"}},
            {"field":"feedback","after":{"instruction":"避免让旁白解释已明白的动作"}}
        ]})).unwrap();
        assert_eq!(proposal.base.brief, work.brief);
        assert_eq!(
            proposal.updated.story.outline,
            "第一卷：分开。\n\n第二卷：回乡后重逢。"
        );
        assert_eq!(
            proposal.updated.story.character_cards[0].motive,
            "带回失散的人"
        );
        assert_eq!(
            proposal.updated.story.character_cards[0].defense,
            "会岔开话题"
        );
        assert_eq!(
            proposal.updated.story.author_truth,
            work.brief.story.author_truth
        );
        assert_eq!(
            proposal.updated.memories[0].kind,
            ruwriter_core::WritingMemoryKind::CharacterClaim
        );
        assert_eq!(proposal.updated.feedback.len(), 1);
        assert_eq!(proposal.previews.len(), 4);
        assert_eq!(
            work.read(json!({"field":"character_cards"})).unwrap()["items"][0]["value"]["defense"],
            "沉默"
        );
        assert!(work.read(json!({"field":"author_truth"})).is_err());
        assert!(
            work.propose(
                json!({"edits":[{"field":"outline","before":"不存在","after":"其他内容"}]})
            )
            .is_err()
        );
    }
}
