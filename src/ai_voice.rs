//! Pick author-approved samples with pacing similar to the current scene.

const RECENT_TEXT: usize = 1200;
const MAX_SAMPLES: usize = 3;
const SAMPLE_BYTES: usize = 3600;

pub fn select_samples(scene: &str, samples: &[String]) -> Vec<String> {
    let tail = scene
        .char_indices()
        .rev()
        .nth(RECENT_TEXT)
        .map_or(0, |(index, _)| index);
    let style = dialogue_share(&scene[tail..]);
    let mut ranked: Vec<_> = samples
        .iter()
        .map(|sample| sample.trim())
        .filter(|sample| !sample.is_empty())
        .map(|sample| (sample, dialogue_share(sample).abs_diff(style)))
        .collect();
    ranked.sort_by_key(|(_, distance)| *distance);
    ranked
        .into_iter()
        .take(MAX_SAMPLES)
        .map(|(sample, _)| {
            let end = sample.floor_char_boundary(SAMPLE_BYTES.min(sample.len()));
            sample[..end].to_owned()
        })
        .collect()
}

fn dialogue_share(text: &str) -> usize {
    let mut in_dialogue = false;
    let mut dialogue = 0;
    let mut prose = 0;
    for character in text.chars() {
        match character {
            '“' | '「' | '『' | '«' => in_dialogue = true,
            '”' | '」' | '』' | '»' => in_dialogue = false,
            '"' => in_dialogue = !in_dialogue,
            _ if character.is_alphanumeric() => {
                prose += 1;
                dialogue += usize::from(in_dialogue);
            }
            _ => {}
        }
    }
    dialogue * 100 / prose.max(1)
}

/// Reuse the generic Cove text index for author-approved revision examples.
pub fn select_feedback(
    request: &str,
    scene: &str,
    records: &[ruwriter_core::WritingFeedback],
) -> Vec<ruwriter_core::WritingFeedback> {
    use cove_agent::retrieval::SearchIndex;
    use ruwriter_core::WritingFeedback;
    let records: Vec<_> = records
        .iter()
        .filter(|record| {
            !record.held && (!record.lesson.trim().is_empty() || record.before != record.after)
        })
        .collect();
    let mut index = SearchIndex::default();
    for record in &records {
        index.add(
            &format!("{} {}", record.subject, record.instruction),
            &format!("{} {}", record.lesson, record.after),
        );
    }
    let sentence = scene.trim_end().lines().last().unwrap_or_default();
    let query = format!("{request} {sentence}");
    let mut selected = index.search(&query, 3);
    if selected.is_empty() {
        selected.extend(
            (0..records.len())
                .rev()
                .filter(|&index| records[index].subject.trim().is_empty())
                .take(2),
        );
    }
    selected
        .into_iter()
        .map(|index| {
            let record = &records[index];
            let common = cove_agent::revision::change_span(&record.before, &record.after)
                .map_or(0, |span| span.before.start);
            let excerpt = |text: &str| {
                let start = text.floor_char_boundary(common.saturating_sub(180));
                let end = text.floor_char_boundary((common + 1800).min(text.len()));
                text[start..end].to_owned()
            };
            WritingFeedback {
                instruction: record.instruction.clone(),
                before: excerpt(&record.before),
                after: excerpt(&record.after),
                ..(*record).clone()
            }
        })
        .collect()
}

pub fn select_memories(
    request: &str,
    scene: &str,
    entries: &[ruwriter_core::WritingMemory],
) -> Vec<ruwriter_core::WritingMemory> {
    let active: Vec<_> = entries
        .iter()
        .filter(|entry| !entry.held && !entry.text.trim().is_empty())
        .collect();
    let mut index = cove_agent::retrieval::SearchIndex::default();
    for entry in &active {
        index.add(&format!("{} {}", entry.title, entry.subject), &entry.text);
    }
    let recent = scene
        .char_indices()
        .rev()
        .nth(400)
        .map_or(0, |(start, _)| start);
    let query = format!("{request} {}", &scene[recent..]);
    let ranked = index.search(&query, 8);
    let mut budget: usize = 12 * 1024;
    let mut selected = std::collections::BTreeSet::new();
    (0..active.len())
        .filter(|&index| active[index].pinned)
        .chain(ranked)
        .filter(|index| selected.insert(*index))
        .filter_map(|index| {
            let entry = active[index];
            let bytes = serde_json::to_vec(entry).ok()?.len();
            budget = budget.checked_sub(bytes)?;
            Some(entry.clone())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruwriter_core::{WritingFeedback, WritingMemory};

    #[test]
    fn recall_respects_scope_and_paused_author_entries() {
        let memory = |title: &str, text: &str| WritingMemory {
            title: title.into(),
            text: text.into(),
            ..Default::default()
        };
        let entries = [
            memory("量子通讯", "窗口仅持续数秒"),
            WritingMemory {
                pinned: true,
                ..memory("全书", "采用第一人称")
            },
            WritingMemory {
                held: true,
                pinned: true,
                ..memory("赵玥", "旧设定")
            },
            memory("赵玥", "把真心当作随手一笔"),
        ];
        let selected = select_memories("赵玥", "她收起了平板", &entries);
        assert_eq!(
            selected
                .iter()
                .map(|entry| entry.title.as_str())
                .collect::<Vec<_>>(),
            ["全书", "赵玥"]
        );
        let revisions = [
            WritingFeedback {
                subject: "量子通讯".into(),
                lesson: "窗口仅持续数秒".into(),
                ..Default::default()
            },
            WritingFeedback {
                subject: "赵玥".into(),
                lesson: "旧写法".into(),
                held: true,
                ..Default::default()
            },
            WritingFeedback {
                subject: "赵玥".into(),
                lesson: "被质疑时，她会把热爱说成兴趣。".into(),
                ..Default::default()
            },
        ];
        let selected = select_feedback("她会怎么回答", "赵玥合上了平板。", &revisions);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].lesson, revisions[2].lesson);
    }

    #[test]
    fn orders_the_same_works_samples_by_scene_instead_of_genre() {
        let dialogue = "「你确定那艘船会回来？」她问。\n「我可没这么说。」";
        let description = "磁层的边缘泛起蓝光。几个信标已经绕到了影子背后。";
        let both = "她关上观测窗，灯仍闪着。\n「再量一次。」";
        let samples = [dialogue.to_owned(), description.to_owned(), both.to_owned()];
        assert_eq!(
            select_samples("“告诉我下一站在哪。”“我怎么知道？”", &samples)[0],
            dialogue
        );
        assert_eq!(
            select_samples("山口的斜坡伸进云里。石阶没入冰雪。", &samples)[0],
            description
        );
    }
}
