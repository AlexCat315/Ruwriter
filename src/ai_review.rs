//! Manuscript-aware review and targeted revision of a candidate.

use cove_agent::{
    AgentError, AgentMessage, AgentModel, AgentRole, AgentSession, AgentSkillLibrary, AgentToolbox,
};
use ruwriter_core::WritingBrief;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::ai::{AskUser, WritingTask, ask_questions, ask_user_tool};
use crate::ai_continuation::{
    ScenePlan, check_prose, continuation_body, prose_reply, quality_check,
};
use crate::ai_research::Source;

const REVIEW: &str = r#"你是这部小说的编辑。先判断 candidate 是否真的交付了正文。只有流程解释、提纲、拒绝或索取材料时，usable 为 false。人物对白或实际接续的半句可以是正文。
优先核对作者的大纲、细纲和本场意图；核对是否把未来卷的事件提前兑现，或让人物采取与设定相反的决定。用 intent 报告，basis 逐字引用这项作者约束。核对本段新增的过去经历。若候选稿把此前没有证据、作者也未授权的具体职业、旧事、收入、物件来历等作为人物反应的依据，即使写在对白里，也用 unsupported_history 报告。quote 引用候选稿中明确新增的经历，reason 指出缺少哪段铺垫及如何用当前可见行动替代，basis 可以为 null。作者允许新增的场景事实、原文已有证据的往事、角色明确在试探或撒谎时的说法，不按此项报错。证据检索覆盖不足时只指出候选稿擅自坐实的具体细节。
核对 continuity.state_change 和 advance_to。只有当稿中出现改变知识、决定、行动或关系的具体结果，才算完成下一拍。换词重谈、继续描述同一物件、重说已经明白的感受都不算。没发生变化或走错方向时用 progress，quote 引用问题句，basis 逐字引用 advance_to。author_revisions 和 accepted_revisions 里作者明确否掉的写法，即使换成近义词、类似比喻或同一叙述结构，也报 intent，basis 引用作者的原话，reason 给出能替代该句叙述功能的具体做法。
confirmed_memories 的 kind 为 character_claim 时只确认角色说过这些话，需保留说话人和听者的认知；fact 为作者核定的事实，craft 为写法经验，note 需要回到原文确认。先核对 character_cards、relationships 和 character_beats 的因果。检查候选稿的动作是否由卡片中的内在价值和防御方式驱动，对话是否会把关系推向作者不允许的方向。比较表面话、真正动机、可见反应三者。两人各有自己的认知：严格核对关系卡 from_knows 和 to_knows，只有 reveal_condition 成立后才允许对应变化。越界必须指出候选哪句具体触发了哪个作者禁止的行为或给谁增加了未获得的知识，不能只说「保持距离」或「感情升温过快」。人物的调侃或防御不是价值观。明显反转核心动机、违背触发后的反应或跨过关系边界时，用 character 类型并写 character 字段，值必须为角色卡原名，quote 为产生偏离的实际字句。
先接住 continuity.carry_from：如果其中的最后一个可见动作或物件在候选稿里没有落地，候选稿却另起无关场景，报 continuity，quote 填候选稿里替代它的句子。写法是否偏离原稿，按 manuscript_excerpt、author_examples 和作品资料判断，不套用固定桥段或通用写作教程。核对 genre、tone 和 direction 的创作意图。author_truth_for_review_only 是只供审稿核对的作者私设，包括真实动机、谁已知、何时可以揭示。用它判断越过视角、不当揭露或把人物说法坐实，审稿意见只引用候选稿及公开原文，不复述私设。检查人名、角色已知事实、物品与时间是否和原稿相符，已经发生的事有没有被重写。不要用其他题材或你的文字偏好评判这部作品。把候选句和原稿中承担同一作用的句段并排比较，检查视角、句式、对白人物、段落切换、情绪怎么呈现、话说到哪里收束。重点识别换说法后仍在重复同一套动作、自动增加的比喻、对已看懂动作的重复解释和调平语言节奏。author_sample_shapes 是样段轮廓，不能因数字不同判文风错误。basis 引用对应的真实样段或作者认可的改法，reason 写清两个句段做法有什么不同，以及要改哪一处。作者原有的术语、调侃、抒情、议论或感官细节应保留。只有与作者稿实际不同、会造成跳脱的地方才报告。举原稿中支持判断的短句，诸如「文风偏 AI」的抽象判断无效。author_revisions 按时间排列，最新的要求优先。
只输出 JSON 对象，格式为 {"usable":true,"issues":[{"kind":"continuity","quote":"候选稿原句","reason":"问题和具体改法","source":null,"basis":"原稿、作者要求或 advance_to 中的逐字短句"}]}。kind 为 continuity、voice、intent、disclosure、character、progress 或 unsupported_history。只有候选稿确实提前揭露私设、把未获证实的角色说法坐实，或超出当前视角时才用 disclosure。作者本轮明确要求揭晓且视角允许时，按本轮要求审稿。只报告可指到 candidate 原句的具体问题。除 unsupported_history 外，basis 找不到逐字原句就不报这一条。reason 必须说清把哪一句改成什么，不写泛泛的文学评论。只有实际引用了检索来源才填 source 编号。先列剧情、角色和知情边界问题，再列文风，最多五项。没发现问题时 issues 为空。"#;

const REVISE: &str = r#"按原文和作者要求修订候选稿。revision 是本轮要求，优先于旧的接续计划和旧的改法。story.outline 约束全书走向和揭示时机，story.detailed_outline 是作者安排的场景顺序、具体事件和结果，以与落笔处相符的条目为准。两者中的未来事件尚未发生。character_cards 和 relationships 约束人物为何这么做、如何防御、关系可以走到哪一步。发言可能是掩饰、试探或隐喻。confirmed_memories 的 fact 才可当作者确认的事实，character_claim 是角色说法。不要给人补履历、收入或关系史来解释反应。用 author_examples 和 manuscript_excerpt 判断句长、语气、动作与对白怎样衔接。对照 accepted_revisions 的改前改后，改掉作者否掉的叙述功能，不只换词。保留仍适用的作者要求、事实、视角和节奏。有半句待接时，保留接在原稿最后一字之后的方式。"#;

const PATCH_REVISION: &str = r#"只改落实本轮要求所需的最少部分，保留其余字句和换行。输出 JSON {"edits":[{"before":"candidate 中唯一出现且逐字一致的原句或片段","after":"替换文字"}]}。删除时 after 为空；插入时用邻接原句作为 before，并在 after 中保留它。重名或重复用语要带足邻接字句以唯一定位。各个 before 不能重叠。必要时可替换连续几段，但不能为统一风格改动无关段落。候选是半句时也可直接定位其中的字句。不要输出整份草稿或解释。"#;

const REFLECT: &str = r#"对照模型稿 before 和作者亲改的 after，抓住对这一场结果影响最大的一处变化。prior_context 只用于认清场景和说话人。辨别这次是改了人物动机、剧情结果、知情边界还是写法。说明原句在场景中做了什么、作者改成怎样推进，只从作者改后版本提炼一条下次能执行的做法，不把改掉的写法列为备选。两版用了同一主题词时，要区分话题在两版中的作用和指向。只总结改后字面能支持的变化。没有明确的台词归属就描述问句和反应本身，不能擅自指定说话人，尤其不要把紧接着点头的人当成问话者。规则用「提问者」「叙述者」等中性称谓，不把原文人物名字写进可复用规则，不猜动机、不扩写。只返回 JSON { "before_quote":"改前逐字短句，纯插入可空", "after_quote":"改后逐字短句，纯删除可空", "lesson":"具体差异及下一次的执行方式，最多两句" }。引用落在真正改动的地方。"#;

#[derive(Clone, serde::Serialize, Deserialize)]
pub struct DraftReflection {
    pub before_quote: String,
    pub after_quote: String,
    pub lesson: String,
    #[serde(default)]
    pub after_hash: String,
    #[serde(default)]
    pub imported: bool,
}

impl DraftReflection {
    pub fn applies_to(&self, text: &str) -> bool {
        self.after_hash == ruwriter_core::sync::content_hash(text)
    }
}

pub fn reflect_author_edit(
    model: &AgentModel,
    before: &str,
    after: &str,
    context: &DraftContext,
) -> Result<DraftReflection, AgentError> {
    let invalid = || AgentError::Model("反思没有给出可核对的改稿依据。".into());
    let change = cove_agent::revision::change_span(before, after).ok_or_else(invalid)?;
    let excerpt = |text: &str, span: std::ops::Range<usize>| {
        let start = text.floor_char_boundary(span.start.saturating_sub(360));
        let end = text.floor_char_boundary(
            span.end
                .min(span.start + 8_000)
                .saturating_add(360)
                .min(text.len()),
        );
        text[start..end].to_owned()
    };
    let reply = prose_reply(
        model,
        REFLECT,
        &json!({
            "before": excerpt(before, change.before.clone()),
            "after": excerpt(after, change.after.clone()),
            "prior_context": {
                "request": context.request,
                "scene": &context.excerpt[context.excerpt.ceil_char_boundary(context.excerpt.len().saturating_sub(3600))..],
                "character_cards": context.brief.story.character_cards,
            }
        }),
    )?;
    let json = reply
        .find('{')
        .zip(reply.rfind('}'))
        .and_then(|(start, end)| reply.get(start..=end))
        .ok_or_else(invalid)?;
    let mut reflection: DraftReflection = serde_json::from_str(json).map_err(|_| invalid())?;
    let anchored = |text: &str, span: std::ops::Range<usize>, quote: &str| {
        let quote = quote.trim();
        if quote.is_empty() {
            return span.is_empty();
        }
        (2..=120).contains(&quote.chars().count())
            && text
                .match_indices(quote)
                .any(|(at, _)| at < span.end && at + quote.len() > span.start)
    };
    if !anchored(before, change.before, &reflection.before_quote)
        || !anchored(after, change.after, &reflection.after_quote)
        || reflection.before_quote.trim() == reflection.after_quote.trim()
        || !(6..=240).contains(&reflection.lesson.trim().chars().count())
    {
        return Err(invalid());
    }
    reflection.after_hash = ruwriter_core::sync::content_hash(after);
    reflection.imported = false;
    Ok(reflection)
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    Continuity,
    Voice,
    Intent,
    Disclosure,
    Character,
    Progress,
    UnsupportedHistory,
}

impl IssueKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Continuity => "连续性",
            Self::Voice => "文风",
            Self::Intent => "作者要求",
            Self::Disclosure => "知情边界",
            Self::Character => "角色或关系",
            Self::Progress => "剧情推进",
            Self::UnsupportedHistory => "未经铺垫的往事",
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

impl DraftIssue {
    fn actionable(&self) -> bool {
        self.basis.is_some()
            || matches!(
                self.kind,
                IssueKind::Disclosure | IssueKind::UnsupportedHistory
            )
    }
}

#[derive(Clone)]
pub enum Review {
    NotRequested,
    Incomplete,
    Checked(Vec<DraftIssue>),
    Rejected,
}

impl Review {
    pub fn blocks_approval(&self) -> bool {
        matches!(self, Self::Checked(issues) if issues.iter().any(DraftIssue::actionable))
    }

    pub fn blocking_issues(&self) -> impl Iterator<Item = &DraftIssue> {
        match self {
            Self::Checked(issues) => issues.as_slice(),
            _ => &[],
        }
        .iter()
        .filter(|issue| issue.actionable())
    }

    pub fn introduces_blocker(&self, baseline: &Self) -> bool {
        self.blocking_issues().any(|issue| {
            !baseline.blocking_issues().any(|prior| {
                prior.kind == issue.kind
                    && prior.character == issue.character
                    && (prior.quote.contains(&issue.quote) || issue.quote.contains(&prior.quote))
            })
        })
    }

    pub fn addressed_quote(&self, revised: &str) -> bool {
        matches!(self, Self::Checked(issues) if issues.iter().any(|issue| {
            issue.actionable() && !revised.contains(&issue.quote)
        }))
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
                    issue.basis.as_ref()?;
                    let character = issue.character.as_deref()?;
                    return Some(format!(
                        "按「{character}」的角色卡和关系边界重写「{}」。保留角色在当前触发下可观察的反应。",
                        issue.quote,
                    ));
                }
                if matches!(issue.kind, IssueKind::UnsupportedHistory) {
                    return Some(format!(
                        "删掉没有出处的往事「{}」，用眼前的行动或对白做到本场要求的变化。",
                        issue.quote
                    ));
                }
                let basis = issue.basis.as_ref()?;
                if protect_truth {
                    return Some(format!(
                        "彻底重写「{}」的{}，对照安全依据「{basis}」。改掉原句的作用和结构，不要只换词或补写读者尚不知道的内容。",
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
            let mut counts = [0usize; 7];
            for issue in issues {
                counts[issue.kind as usize] += 1;
            }
            counts
        };
        let before_counts = counts(before);
        let after_counts = counts(after);
        let actionable =
            |issues: &[DraftIssue]| issues.iter().filter(|issue| issue.actionable()).count();
        before_counts
            .iter()
            .zip(after_counts)
            .all(|(prior, later)| later <= *prior)
            && actionable(after) < actionable(before)
    }
}

#[derive(Clone)]
pub struct DraftContext {
    pub task: WritingTask,
    pub request: String,
    pub excerpt: String,
    pub plan: Option<ScenePlan>,
    pub brief: WritingBrief,
    pub voice_samples: Vec<String>,
    pub local_reference: String,
}

pub struct RevisionRequest<'a> {
    pub instruction: &'a str,
    pub whole_draft: bool,
    pub ask_user: Option<AskUser>,
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
            "author_sample_shapes": crate::ai_voice::voice_shapes(&self.voice_samples),
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
                        || self
                            .plan
                            .as_ref()
                            .is_some_and(|plan| plan.advance_to().contains(quote))
                        || self.request.contains(quote)
                        || [
                            &self.brief.story.outline,
                            &self.brief.story.detailed_outline,
                            &self.brief.story.direction,
                            &self.brief.canon,
                            &self.brief.voice,
                            &self.brief.avoid,
                        ]
                        .iter()
                        .any(|field| field.contains(quote))
                        || self.brief.story.character_cards.iter().any(|card| {
                            [&card.surface, &card.motive, &card.defense, &card.boundary]
                                .iter()
                                .any(|field| field.contains(quote))
                        })
                        || self.brief.story.relationships.iter().any(|relation| {
                            [
                                &relation.state,
                                &relation.direction,
                                &relation.boundary,
                                &relation.evidence,
                                &relation.from_knows,
                                &relation.to_knows,
                                &relation.reveal_condition,
                            ]
                            .iter()
                            .any(|field| field.contains(quote))
                        })
                        || self
                            .brief
                            .memories
                            .iter()
                            .any(|entry| entry.text.contains(quote))
                        || revisions
                            .iter()
                            .any(|instruction| instruction.contains(quote))
                        || self.brief.feedback.iter().any(|record| {
                            [
                                &record.instruction,
                                &record.before,
                                &record.after,
                                &record.lesson,
                            ]
                            .iter()
                            .any(|field| field.contains(quote))
                        })
                })?;
                if let Review::Checked(issues) = &mut review {
                    for issue in issues {
                        if matches!(issue.kind, IssueKind::Voice)
                            && !issue.basis.as_ref().is_some_and(|quote| {
                                self.voice_samples
                                    .iter()
                                    .any(|sample| sample.contains(quote))
                                    || self.brief.voice.contains(quote)
                                    || self.brief.avoid.contains(quote)
                                    || self.brief.feedback.iter().any(|entry| {
                                        [&entry.after, &entry.lesson, &entry.instruction]
                                            .iter()
                                            .any(|field| field.contains(quote))
                                    })
                            })
                        {
                            issue.basis = None;
                        }
                        if matches!(issue.kind, IssueKind::Progress)
                            && !self.plan.as_ref().is_some_and(|plan| {
                                issue
                                    .basis
                                    .as_ref()
                                    .is_some_and(|basis| plan.advance_to().contains(basis))
                            })
                        {
                            issue.basis = None;
                        }
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
        request: RevisionRequest<'_>,
    ) -> Result<String, AgentError> {
        let RevisionRequest {
            instruction,
            whole_draft,
            ask_user,
        } = request;
        let mut input = self.payload(draft, sources, revisions);
        input["revision"] = json!(instruction);
        let edit_format = if whole_draft {
            "candidate 为空时，从 manuscript_excerpt 的落笔处起草。只输出完整的修订后候选正文。"
        } else {
            PATCH_REVISION
        };
        let instructions = format!("{REVISE}\n{edit_format}");
        let answer = if let Some(ask) = ask_user {
            let toolbox = AgentToolbox::new(
                vec![ask_user_tool()],
                std::sync::Arc::new(move |_, input| ask_questions(&ask, input, 1, false)),
            )?;
            AgentSession::new(toolbox, &AgentSkillLibrary::default(), &[], 3, model.clone())?
                .run(vec![
                    AgentMessage::text(AgentRole::System, format!("{instructions}\n如果改稿要求和作者已确认的角色动机、关系边界或剧情结果冲突且意图不明确，先 ask_user，再继续改稿。可以依据现有资料确定的写法直接改。")),
                    AgentMessage::user(input.to_string()),
                ])?
                .answer
        } else {
            prose_reply(model, &instructions, &input)?
        };
        if !whole_draft {
            #[derive(Deserialize)]
            struct Edits {
                edits: Vec<Replacement>,
            }
            #[derive(Deserialize)]
            struct Replacement {
                before: String,
                after: String,
            }
            let invalid = || AgentError::Model("未收到可定位的改动，已保留原草稿。".to_owned());
            let reply = answer.trim();
            let reply = reply
                .strip_prefix("```json")
                .and_then(|text| text.strip_suffix("```"))
                .unwrap_or(reply);
            let edits: Edits = serde_json::from_str(reply).map_err(|_| invalid())?;
            let revised = cove_agent::revision::apply_replacements(
                draft,
                edits
                    .edits
                    .iter()
                    .map(|edit| (edit.before.as_str(), edit.after.as_str())),
            )
            .ok_or_else(invalid)?;
            check_prose(revised.trim())?;
            return Ok(revised);
        }
        let prose = if self.task == WritingTask::Continue {
            continuation_body(&self.excerpt, &answer)
        } else {
            &answer
        };
        check_prose(prose)?;
        Ok(prose.to_owned())
    }

    pub fn local_warning(
        &self,
        draft: &str,
        sources: &[Source],
        revisions: &[String],
    ) -> Option<String> {
        if let Some(phrase) = crate::ai_voice::rejected_expression_in(draft, revisions) {
            return Some(format!(
                "草稿又用了这一轮明确否掉的表达「{phrase}」，应重新安排这一句的动作或对白。"
            ));
        }
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
        issue.quote = quote.to_owned();
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
    fn suspicious_unplanted_history_has_a_repair_and_blocks_adoption() {
        let draft = "赵玥说：“上个月在钟楼画了三张，收了四十块钱。”";
        let report = r#"{"usable":true,"issues":[{"kind":"unsupported_history","quote":"上个月在钟楼画了三张","reason":"原文没有她曾卖画的经历，改由她接着展示眼前的画。","basis":null}]}"#;
        let result = parse_review(report, draft, &[], |_| false).unwrap();
        assert!(result.blocks_approval());
        assert!(
            result
                .revision_direction(true)
                .unwrap()
                .contains("上个月在钟楼画了三张")
        );
        assert!(parse_review(report, "她将平板揣回包里。", &[], |_| false).is_none());
    }

    #[test]
    fn progress_audit_and_author_rejection_block_adoption_with_grounded_quotes() {
        let result = parse_review(
            r#"{"usable":true,"issues":[
                {"kind":"progress","quote":"我还是问她那幅画。","reason":"仍重复刚问过的问题","basis":"让她决定把画留下"},
                {"kind":"intent","quote":"没急着接","reason":"复用了作者否掉的停顿句式","basis":"没急着接腔"}
            ]}"#,
            "我没急着接。我还是问她那幅画。",
            &[],
            |basis| matches!(basis, "让她决定把画留下" | "没急着接腔"),
        ).unwrap();
        assert!(result.blocks_approval());
        assert!(!result.addressed_quote("我没急着接，然后我还是问她那幅画。"));
        let revision = "她把纸压在桌角。“这张你先留着。”";
        assert!(result.addressed_quote(revision));
        assert!(
            result
                .revision_direction(false)
                .unwrap()
                .contains("让她决定把画留下")
        );
    }

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
