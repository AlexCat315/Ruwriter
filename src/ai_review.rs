//! Manuscript-aware review and targeted revision of a candidate.

use cove_agent::{AgentError, AgentModel};
use ruwriter_core::WritingBrief;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::ai::WritingTask;
use crate::ai_continuation::{
    ScenePlan, check_prose, continuation_body, prose_reply, quality_check,
};
use crate::ai_research::Source;

const REVIEW: &str = r#"你是这部小说的编辑。先判断 candidate 是否真的交付了正文。只有流程解释、提纲、拒绝或索取材料时，usable 为 false。人物对白或实际接续的半句可以是正文。
先核对 character_cards、relationships 和 character_beats 的因果。检查候选稿的动作是否由卡片中的内在价值和防御方式驱动，对话是否会把关系推向作者不允许的方向。比较表面话、真正动机、可见反应三者。人物的调侃或防御不是价值观。明显反转核心动机、违背触发后的反应或跨过关系边界时，用 character 类型并写 character 字段，值必须为角色卡原名，quote 为产生偏离的实际字句。核对 genre、tone 和 direction 的创作意图。author_truth_for_review_only 是只供审稿核对的作者私设，包括真实动机、谁已知、何时可以揭示。用它判断越过视角、不当揭露或把人物说法坐实，审稿意见只引用候选稿及公开原文，不复述私设。检查人名、角色已知事实、物品与时间是否和原稿相符，已经发生的事有没有被重写。不要用其他题材或你的文字偏好评判这部作品。比较 manuscript_excerpt 和 author_examples 中叙述者说话的方法、长短句、对白切换、情绪和场景收束。作者原有的术语、调侃、抒情、议论或感官细节应保留。只有与作者稿实际不同、会造成跳脱的地方才报告。举原稿中支持判断的短句，诸如「文风偏 AI」的抽象判断无效。author_revisions 按时间排列，最新的要求优先。
只输出 JSON 对象，格式为 {"usable":true,"issues":[{"kind":"continuity","quote":"候选稿原句","reason":"问题和改法","source":null,"basis":"用来核对的原稿或作者样段中的逐字短句"}]}。kind 为 continuity、voice、intent、disclosure 或 character。只有候选稿确实提前揭露私设、把未获证实的角色说法坐实，或超出当前视角时才用 disclosure。作者本轮明确要求揭晓且视角允许时，按本轮要求审稿。缺乏确切原句时 basis 填 null，并说明待核对。只有实际引用了检索来源才填 source 编号。最多五项，没发现问题时 issues 为空。不要凭空添加新设定。"#;

const REVISE: &str = r#"按原文和作者要求修订候选稿。candidate 为空时，直接使用 manuscript_excerpt 起草。revision 是当前要做的事；author_revisions 中仍适用的作者要求继续遵守。accepted_revisions 是作者明示保留的改法，仅参考发生变化的写法。author_examples 只供揣摩写法，原稿写法优先，不能把样段事件带进剧情。定位被指出的句子，按角色卡的动机、触发和保护动作重写，保留其他已成立的事实、视角和节奏。有话没说完就从原稿末字后接，不抄原稿，不重新叙述已发生的事。只输出完整的修订后候选正文。"#;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    Continuity,
    Voice,
    Intent,
    Disclosure,
    Character,
}

impl IssueKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Continuity => "连续性",
            Self::Voice => "文风",
            Self::Intent => "作者要求",
            Self::Disclosure => "知情边界",
            Self::Character => "角色或关系",
        }
    }
}

#[derive(Clone, Deserialize)]
pub struct DraftIssue {
    pub kind: IssueKind,
    pub quote: String,
    pub reason: String,
    pub source: Option<String>,
    pub basis: Option<String>,
    pub character: Option<String>,
}

#[derive(Clone)]
pub enum Review {
    NotRequested,
    Incomplete,
    Checked(Vec<DraftIssue>),
    Rejected,
}

impl Review {
    pub fn has_disclosure(&self) -> bool {
        matches!(self, Self::Checked(issues) if issues.iter().any(|issue| matches!(issue.kind, IssueKind::Disclosure)))
    }

    pub fn has_character_violation(&self) -> bool {
        matches!(self, Self::Checked(issues) if issues.iter().any(|issue| matches!(issue.kind, IssueKind::Character)))
    }

    pub fn revision_direction(&self, protect_truth: bool) -> Option<String> {
        let Self::Checked(issues) = self else {
            return None;
        };
        let edits: Vec<_> = issues
            .iter()
            .filter_map(|issue| {
                if matches!(issue.kind, IssueKind::Disclosure) {
                    return Some(format!(
                        "删除「{}」中超出当前视角的定论，保留可观察的言行和不确定性。",
                        issue.quote
                    ));
                }
                if matches!(issue.kind, IssueKind::Character) {
                    let character = issue.character.as_deref()?;
                    return Some(format!(
                        "按「{character}」的角色卡和关系边界重写「{}」。保留角色在当前触发下可观察的反应。",
                        issue.quote,
                    ));
                }
                let basis = issue.basis.as_ref()?;
                if protect_truth {
                    return Some(format!(
                        "参照公开原文「{basis}」检查「{}」的{}，不要补写角色尚不知道的内容。",
                        issue.quote,
                        issue.kind.label()
                    ));
                }
                Some(format!(
                    "「{}」{}。对照原稿「{basis}」。",
                    issue.quote, issue.reason
                ))
            })
            .collect();
        if edits.is_empty() {
            return None;
        }
        Some(format!(
            "修复以下有原文依据的问题。保留没有问题的写法和剧情。\n{}",
            edits.join("\n")
        ))
    }

    pub fn is_improved_by(&self, candidate: &Self) -> bool {
        let (Self::Checked(before), Self::Checked(after)) = (self, candidate) else {
            return false;
        };
        let counts = |issues: &[DraftIssue]| {
            let mut counts = [0usize; 5];
            for issue in issues {
                counts[issue.kind as usize] += 1;
            }
            counts
        };
        let before_counts = counts(before);
        let after_counts = counts(after);
        let actionable = |issues: &[DraftIssue]| {
            issues
                .iter()
                .filter(|issue| {
                    issue.basis.is_some()
                        || matches!(issue.kind, IssueKind::Disclosure | IssueKind::Character)
                })
                .count()
        };
        before_counts
            .iter()
            .zip(after_counts)
            .all(|(prior, later)| later <= *prior)
            && actionable(after) < actionable(before)
    }
}

pub struct DraftContext {
    pub task: WritingTask,
    pub request: String,
    pub excerpt: String,
    pub plan: Option<ScenePlan>,
    pub brief: WritingBrief,
    pub voice_samples: Vec<String>,
    pub local_reference: String,
}

impl DraftContext {
    fn payload(&self, draft: &str, sources: &[Source], revisions: &[String]) -> Value {
        let evidence: Vec<_> = sources
            .iter()
            .map(|source| {
                json!({
                    "id": source.id, "chapter": source.title, "excerpt": source.text,
                })
            })
            .collect();
        json!({
            "task": if self.task == WritingTask::Continue { "续写" } else { "润色" },
            "request": self.request,
            "author_revisions": revisions,
            "manuscript_excerpt": self.excerpt,
            "continuity": self.plan,
            "story": self.brief.story.public_view(),
            "canon": self.brief.canon,
            "voice": self.brief.voice,
            "author_examples": self.voice_samples,
            "accepted_revisions": self.brief.feedback,
            "confirmed_memories": self.brief.memories,
            "avoid": self.brief.avoid,
            "evidence": evidence,
            "candidate": draft,
        })
    }

    pub fn review(
        &self,
        model: &AgentModel,
        draft: &str,
        sources: &[Source],
        revisions: &[String],
    ) -> Review {
        if self.task == WritingTask::Request {
            return Review::NotRequested;
        }
        let mut input = self.payload(draft, sources, revisions);
        input["author_truth_for_review_only"] = json!(self.brief.story.author_truth);
        prose_reply(model, REVIEW, &input)
            .ok()
            .and_then(|reply| {
                let mut review = parse_review(&reply, draft, sources, |quote| {
                    self.excerpt.contains(quote)
                        || self
                            .voice_samples
                            .iter()
                            .any(|sample| sample.contains(quote))
                        || sources.iter().any(|source| source.text.contains(quote))
                })?;
                if let Review::Checked(issues) = &mut review {
                    for issue in issues {
                        if matches!(issue.kind, IssueKind::Character)
                            && !issue.character.as_deref().is_some_and(|name| {
                                self.brief
                                    .story
                                    .character_cards
                                    .iter()
                                    .any(|card| card.name.trim() == name.trim())
                            })
                        {
                            return Some(Review::Incomplete);
                        }
                    }
                }
                Some(review)
            })
            .unwrap_or(Review::Incomplete)
    }

    pub fn revise(
        &self,
        model: &AgentModel,
        draft: &str,
        sources: &[Source],
        revisions: &[String],
        revision: &str,
    ) -> Result<String, AgentError> {
        let mut input = self.payload(draft, sources, revisions);
        input["revision"] = json!(revision);
        let answer = prose_reply(model, REVISE, &input)?;
        let prose = if self.task == WritingTask::Continue {
            continuation_body(&self.excerpt, &answer)
        } else {
            &answer
        };
        check_prose(prose)?;
        Ok(prose.to_owned())
    }

    pub fn local_warning(&self, draft: &str, sources: &[Source]) -> Option<String> {
        let references: Vec<_> = std::iter::once(self.local_reference.as_str())
            .chain(sources.iter().map(|source| source.text.as_str()))
            .collect();
        let references = if self.task == WritingTask::Continue {
            references.as_slice()
        } else {
            &[]
        };
        quality_check(draft, references, &self.brief)
    }
}

fn parse_review(
    reply: &str,
    draft: &str,
    sources: &[Source],
    grounds: impl Fn(&str) -> bool,
) -> Option<Review> {
    #[derive(Deserialize)]
    struct Report {
        usable: Option<bool>,
        issues: Vec<DraftIssue>,
    }

    let reply = reply.trim();
    let reply = reply
        .strip_prefix("```json")
        .and_then(|text| text.strip_suffix("```"))
        .unwrap_or(reply);
    let mut report: Report = serde_json::from_str(reply).ok()?;
    if report.usable == Some(false) {
        return Some(Review::Rejected);
    }
    if report.issues.len() > 5 {
        return None;
    }

    for issue in &mut report.issues {
        let quote = issue.quote.trim();
        if quote.is_empty()
            || quote.chars().count() > 120
            || !draft.contains(quote)
            || issue.reason.trim().is_empty()
            || issue.reason.chars().count() > 280
        {
            return None;
        }
        if let Some(id) = &issue.source {
            let id = id.trim().trim_matches(['[', ']']);
            if !sources.iter().any(|source| source.id == id) {
                return None;
            }
            issue.source = Some(id.to_owned());
        }
        issue.basis = issue.basis.take().and_then(|basis| {
            let basis = basis.trim();
            ((3..=120).contains(&basis.chars().count()) && grounds(basis)).then(|| basis.to_owned())
        });
    }
    Some(Review::Checked(report.issues))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_that_review_quotes_and_references_exist() {
        let source = Source {
            id: "R1".into(),
            title: "十二章".into(),
            file: "卷一".into(),
            first_line: 1,
            last_line: 4,
            text: "钥匙还在周明手上。".into(),
        };
        let draft = "她从口袋里掏出了钥匙。";
        let reply = r#"{"issues":[{"kind":"continuity","quote":"她从口袋里掏出了钥匙","reason":"先交代钥匙如何到她手里","source":"[R1]"}]}"#;
        assert!(parse_review(reply, draft, &[], |_| false).is_none());
        assert!(
            parse_review(
                reply,
                "她开了门。",
                std::slice::from_ref(&source),
                |_| false
            )
            .is_none()
        );
        let Some(Review::Checked(report)) = parse_review(reply, draft, &[source], |_| false) else {
            panic!("expected a grounded review");
        };
        assert_eq!(report[0].source.as_deref(), Some("R1"));
        assert_eq!(report[0].kind.label(), "连续性");
    }

    #[test]
    fn automatic_revisions_require_grounded_notes_and_cannot_trade_voice_for_story_errors() {
        let earlier = "他把匣子交给阿霜。";
        let draft = "那匣子像夜幕里的命运。";
        let report = r#"{"usable":true,"issues":[
            {"kind":"voice","quote":"命运","reason":"改用原稿里的客观动作","basis":"他把匣子交给阿霜。"}
        ]}"#;
        let unsupported = parse_review(report, draft, &[], |_| false).unwrap();
        assert!(unsupported.revision_direction(false).is_none());
        let original = parse_review(report, draft, &[], |quote| earlier.contains(quote)).unwrap();
        assert!(
            original
                .revision_direction(false)
                .unwrap()
                .contains(earlier)
        );
        let revision = "他把匣子交给阿禾。";
        let worse = parse_review(
            r#"{"usable":true,"issues":[
            {"kind":"continuity","quote":"阿禾","reason":"原稿是阿霜","basis":"他把匣子交给阿霜。"}
        ]}"#,
            revision,
            &[],
            |quote| earlier.contains(quote),
        )
        .unwrap();
        assert!(!original.is_improved_by(&worse));
        assert!(original.is_improved_by(&Review::Checked(Vec::new())));
        assert!(!original.is_improved_by(&Review::Incomplete));
    }
}
