//! Local checkpoints for the author's decisions at a manuscript position.

use ruwriter_core::sync::content_hash;
use serde::{Deserialize, Serialize};

use crate::ai::{ChatTurn, WritingTarget};
use crate::ai_continuation::SceneQuestion;
use crate::ai_research::ResearchScope;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneAnchor {
    insertion: usize,
    selection_start: Option<usize>,
    digest: String,
}

impl SceneAnchor {
    pub fn capture(target: &WritingTarget) -> Option<Self> {
        let context = target.continuation_context().ok()?.string_value();
        if context.trim().is_empty() {
            return None;
        }
        let end = target
            .insertion
            .saturating_add(96)
            .min(target.buffer.character_count());
        let trailing = target.buffer.substring(target.insertion..end);
        Some(Self {
            insertion: target.insertion,
            selection_start: target.selection.as_ref().map(|range| range.start),
            digest: content_hash(&format!("{}\n{}{}", context.len(), context, trailing)),
        })
    }

    pub fn restore(&self, current: &WritingTarget) -> Option<WritingTarget> {
        if self.insertion > current.buffer.character_count()
            || self
                .selection_start
                .is_some_and(|start| start >= self.insertion)
        {
            return None;
        }
        let target = WritingTarget {
            insertion: self.insertion,
            selection: self.selection_start.map(|start| start..self.insertion),
            ..current.clone()
        };
        (Self::capture(&target).as_ref() == Some(self)).then_some(target)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PendingScene {
    pub anchor: SceneAnchor,
    pub instruction: String,
    pub question: SceneQuestion,
    pub scope: ResearchScope,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SceneAnswer {
    pub anchor: SceneAnchor,
    pub question: SceneQuestion,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct LocalTurn {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<PendingScene>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_answer: Option<SceneAnswer>,
}

pub fn pending(turns: &[ChatTurn]) -> Option<(&ChatTurn, &PendingScene)> {
    let turn = turns.iter().rev().find(|turn| turn.local.is_some())?;
    Some((turn, turn.local.as_ref()?.pending.as_ref()?))
}

/// Confirmed replies are advisory for later requests at this exact anchor.
/// The most recent answer to a repeated question wins.
pub fn append_answers(prompt: &str, target: Option<&WritingTarget>, turns: &[ChatTurn]) -> String {
    let Some(anchor) = target.and_then(SceneAnchor::capture) else {
        return prompt.to_owned();
    };
    let mut seen = std::collections::BTreeSet::new();
    let recalled: Vec<_> = turns
        .iter()
        .rev()
        .filter_map(|turn| {
            let answer = turn.local.as_ref()?.scene_answer.as_ref()?;
            if answer.anchor != anchor || !seen.insert(&answer.question.question) {
                return None;
            }
            let reply = turn.author.string_value();
            (!(prompt.contains(&answer.question.question) && prompt.contains(&reply)))
                .then_some((&answer.question, reply))
        })
        .take(3)
        .collect();
    if recalled.is_empty() {
        return prompt.to_owned();
    }
    let mut context = String::from(prompt);
    context.push_str("\n\n作者在同一落笔点的已确认回答（本轮要求优先）");
    for (question, answer) in recalled.into_iter().rev() {
        let end = answer
            .char_indices()
            .nth(1200)
            .map_or(answer.len(), |(index, _)| index);
        context.push_str(&format!(
            "\n原文「{}」\n问题 {}\n作者答复 {}",
            question.quote,
            question.question,
            &answer[..end]
        ));
    }
    context
}
