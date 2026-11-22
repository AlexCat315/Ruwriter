//! Pick author-approved samples with pacing similar to the current scene.

const RECENT_TEXT: usize = 1200;
const MAX_SAMPLES: usize = 3;
const SAMPLE_BYTES: usize = 3600;

/// Local author prose supplies a baseline even when no samples were pinned.
pub fn with_local_sample(scene: &str, samples: &[String]) -> Vec<String> {
    let mut selected = select_samples(scene, samples);
    let recent = recent_text(scene, 1250);
    let local_sample = if recent.len() < scene.len() {
        recent.split_once('\n').map_or(recent, |(cut, body)| {
            if cut.chars().count() < 240 {
                body
            } else {
                recent
            }
        })
    } else {
        recent
    }
    .trim();
    if !local_sample.is_empty()
        && !selected
            .iter()
            .any(|reference| reference.contains(local_sample))
    {
        selected.insert(0, local_sample.to_owned());
    }
    selected
}

/// Measurements are used to rank samples and explain the reference to the
/// writer. They are descriptive and never act as pass/fail thresholds.
#[derive(Clone, Copy, serde::Serialize)]
pub struct ProseShape {
    dialogue_percent: usize,
    median_sentence_chars: usize,
    longer_sentence_chars: usize,
    median_paragraph_chars: usize,
}

impl ProseShape {
    fn measure(text: &str) -> Self {
        let lengths = |segments: std::str::Split<'_, fn(char) -> bool>| {
            let mut lengths: Vec<_> = segments
                .map(|segment| segment.chars().filter(|ch| ch.is_alphanumeric()).count())
                .filter(|&length| length > 0)
                .collect();
            lengths.sort_unstable();
            lengths
        };
        let sentences =
            lengths(text.split((|ch: char| "\n。！？!?".contains(ch)) as fn(char) -> bool));
        let paragraphs = lengths(text.split((|ch| ch == '\n') as fn(char) -> bool));
        let percentile = |data: &[usize], numerator: usize, denominator: usize| {
            data.get(data.len().saturating_sub(1) * numerator / denominator)
                .copied()
                .unwrap_or_default()
        };
        Self {
            dialogue_percent: dialogue_share(text),
            median_sentence_chars: percentile(&sentences, 1, 2),
            longer_sentence_chars: percentile(&sentences, 3, 4),
            median_paragraph_chars: percentile(&paragraphs, 1, 2),
        }
    }

    fn distance(self, other: Self) -> usize {
        self.dialogue_percent.abs_diff(other.dialogue_percent) * 2
            + self
                .median_sentence_chars
                .abs_diff(other.median_sentence_chars)
                .min(60)
            + self
                .longer_sentence_chars
                .abs_diff(other.longer_sentence_chars)
                .min(60)
            + self
                .median_paragraph_chars
                .abs_diff(other.median_paragraph_chars)
                .min(100)
                / 2
    }
}

pub fn voice_shapes(samples: &[String]) -> Vec<ProseShape> {
    samples
        .iter()
        .map(|sample| ProseShape::measure(sample))
        .collect()
}

pub fn summarize_author_feedback(instruction: &str) -> String {
    let rejected = rejected_expressions(instruction);
    if rejected.is_empty() {
        return instruction.trim().chars().take(440).collect();
    }
    format!(
        "作者要求避开{}及同一语用功能的近义改写。{}",
        rejected
            .iter()
            .map(|phrase| format!("「{phrase}」"))
            .collect::<Vec<_>>()
            .join("、"),
        instruction.trim().chars().take(360).collect::<String>()
    )
}

pub fn lasting_author_direction(instruction: &str) -> Option<String> {
    let instruction = instruction.trim();
    if ["修复以下", "草稿重用了", "初稿用了", "修复并复审"]
        .iter()
        .any(|prefix| instruction.starts_with(prefix))
    {
        return None;
    }
    let durable = [
        "以后写",
        "以后都",
        "后面都",
        "统一文风",
        "全书都",
        "后续所有",
    ]
    .iter()
    .any(|hint| instruction.contains(hint));
    let style = [
        "文风", "叙述", "语言", "句式", "用词", "比喻", "段落", "语气",
    ]
    .iter()
    .any(|hint| instruction.contains(hint));
    if durable && style {
        return Some(instruction.chars().take(1800).collect());
    }
    let rejected = rejected_expressions(instruction);
    (!rejected.is_empty()).then(|| {
        rejected
            .iter()
            .take(8)
            .map(|quote| format!("不要复用「{quote}」"))
            .collect::<Vec<_>>()
            .join("。")
    })
}

pub fn rejected_expressions(instruction: &str) -> Vec<String> {
    let mut phrases = Vec::new();
    let mut rest = instruction;
    while let Some((at, open)) = rest
        .char_indices()
        .find(|(_, ch)| matches!(ch, '「' | '“' | '『' | '"'))
    {
        let end_quote = match open {
            '「' => '」',
            '“' => '”',
            '『' => '』',
            _ => '"',
        };
        let content = &rest[at + open.len_utf8()..];
        let Some((quote, tail)) = content.split_once(end_quote) else {
            break;
        };
        let before = rest[..at]
            .rsplit(['，', '。', '！', '？', '；', ';', '\n'])
            .next()
            .unwrap_or("");
        let after = tail
            .split([
                '，', '。', '！', '？', '；', ';', '\n', '「', '“', '『', '"',
            ])
            .next()
            .unwrap_or("");
        let before_is_negative = [
            "不要", "别用", "别写", "别再", "避开", "删掉", "去掉", "不用", "换掉", "禁止", "反对",
        ]
        .iter()
        .any(|cue| before.contains(cue));
        let after_is_negative = [
            "AI味",
            "AI 味",
            "太像AI",
            "套话",
            "套式",
            "写得不对",
            "不成立",
            "换掉",
            "删掉",
            "别再",
        ]
        .iter()
        .any(|cue| after.contains(cue));
        let quote = quote.trim();
        if (before_is_negative || after_is_negative)
            && (4..=140).contains(&quote.chars().count())
            && !phrases.iter().any(|prior| prior == quote)
        {
            phrases.push(quote.to_owned());
        }
        rest = tail;
    }
    phrases
}

pub fn rejected_expression_in(
    draft: &str,
    instructions: impl IntoIterator<Item = impl AsRef<str>>,
) -> Option<String> {
    for instruction in instructions {
        for phrase in rejected_expressions(instruction.as_ref()) {
            if draft.contains(&phrase) {
                return Some(phrase);
            }
            for fragment in phrase.split(['，', '、', '；', '：', '。']) {
                let count = fragment.chars().count();
                if count >= 5 {
                    let prefix: String = fragment.chars().take(count - 1).collect();
                    if draft.contains(&prefix) {
                        return Some(phrase);
                    }
                }
            }
        }
    }
    None
}

pub fn select_samples(scene: &str, samples: &[String]) -> Vec<String> {
    let tail = scene
        .char_indices()
        .rev()
        .nth(RECENT_TEXT)
        .map_or(0, |(index, _)| index);
    let style = ProseShape::measure(&scene[tail..]);
    let mut ranked: Vec<_> = samples
        .iter()
        .map(|sample| sample.trim())
        .filter(|sample| !sample.is_empty())
        .map(|sample| (sample, ProseShape::measure(sample).distance(style)))
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
    let query = format!("{request} {}", recent_text(scene, 600));
    let lowercase_query = query.to_lowercase();
    let lowercase_scene = scene.to_lowercase();
    let records: Vec<_> = records
        .iter()
        .rev()
        .filter(|record| {
            !record.held
                && (!record.lesson.trim().is_empty()
                    || !record.instruction.trim().is_empty()
                    || record.before != record.after)
                && (scope_matches(&record.subject, &lowercase_query)
                    || scope_matches(&record.subject, &lowercase_scene))
        })
        .map(revision_excerpt)
        .collect();
    let mut index = SearchIndex::default();
    for record in &records {
        index.add(
            &format!("{} {}", record.subject, record.instruction),
            &format!("{} {} {}", record.lesson, record.before, record.after),
        );
    }
    let ranked = index.search(&query, records.len());
    let mut selected = Vec::new();
    for index in (0..records.len())
        .filter(|&index| records[index].before.is_empty() && records[index].after.is_empty())
        .take(2)
        .chain(
            ranked
                .iter()
                .copied()
                .chain(0..records.len())
                .filter(|&index| !records[index].subject.trim().is_empty())
                .chain(ranked.iter().copied()),
        )
        .chain(0..records.len())
    {
        if !selected.contains(&index) {
            selected.push(index);
        }
        if selected.len() == 5 {
            break;
        }
    }
    selected
        .into_iter()
        .map(|index| records[index].clone())
        .collect()
}

fn scope_matches(subject: &str, query: &str) -> bool {
    subject.trim().is_empty()
        || subject
            .split([',', '，', '、', '；', ';', '\n'])
            .map(str::trim)
            .any(|term| !term.is_empty() && query.contains(&term.to_lowercase()))
}

fn revision_excerpt(record: &ruwriter_core::WritingFeedback) -> ruwriter_core::WritingFeedback {
    let common = cove_agent::revision::change_span(&record.before, &record.after)
        .map_or(0, |span| span.before.start);
    let excerpt = |text: &str| {
        let start = text.floor_char_boundary(common.saturating_sub(180));
        let end = text.floor_char_boundary((common + 1800).min(text.len()));
        text[start..end].to_owned()
    };
    ruwriter_core::WritingFeedback {
        before: excerpt(&record.before),
        after: excerpt(&record.after),
        ..record.clone()
    }
}

fn recent_text(text: &str, count: usize) -> &str {
    let start = text
        .char_indices()
        .rev()
        .nth(count)
        .map_or(0, |(start, _)| start);
    &text[start..]
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
    let query = format!("{request} {}", recent_text(scene, 400));
    let ranked = index.search(&query, 8);
    let mut budget: usize = 12 * 1024;
    let mut selected = std::collections::BTreeSet::new();
    (0..active.len())
        .filter(|&index| active[index].pinned)
        .chain(ranked)
        .chain(
            (0..active.len())
                .rev()
                .filter(|&index| {
                    active[index].subject.trim().is_empty()
                        && active[index].title == "改稿对照的写法归纳"
                })
                .take(1),
        )
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

    #[test]
    fn learns_rejected_phrasing_without_replaying_the_old_scene_instruction() {
        let instruction = "把画收到桌边。不要用「我点点头，没急着接腔。」";
        let saved = lasting_author_direction(instruction).unwrap();
        assert!(saved.contains("没急着接腔"));
        assert!(!saved.contains("把画收到"));
        assert!(rejected_expression_in("我嗯了一声，没急着接。", [&saved]).is_some());
        assert!(rejected_expression_in("她将画塞回袋子。", [&saved]).is_none());
        assert!(lasting_author_direction("这次先让她把画收走。").is_none());
    }
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
                ..memory("阿青", "旧设定")
            },
            memory("阿青", "把真心当作随手一笔"),
        ];
        let selected = select_memories("阿青", "她收起了本子", &entries);
        assert_eq!(
            selected
                .iter()
                .map(|entry| entry.title.as_str())
                .collect::<Vec<_>>(),
            ["全书", "阿青"]
        );
        let revisions = [
            WritingFeedback {
                subject: "量子通讯".into(),
                lesson: "窗口仅持续数秒".into(),
                ..Default::default()
            },
            WritingFeedback {
                subject: "阿青".into(),
                lesson: "旧写法".into(),
                held: true,
                ..Default::default()
            },
            WritingFeedback {
                subject: "阿青".into(),
                lesson: "被质疑时，她会把认真准备说成随手做的。".into(),
                ..Default::default()
            },
        ];
        let selected = select_feedback("续写", "阿青合上了本子。\n我没再说话。", &revisions);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].lesson, revisions[2].lesson);
        assert!(select_feedback("续写", "她按下通讯开关。", &revisions).is_empty());
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

    #[test]
    fn keeps_the_current_passage_first_and_prefers_its_sentence_rhythm() {
        let local = "我起身。\n门没锁。\n她在门外。";
        let short = "袋子空了。\n我看了看桌下。\n地上也没有。";
        let long = "我顺着惟一没有被废弃的石阶往下走，台阶的缝里到处都是前一晚下雨留下来的积水。";
        let authored = [long.to_owned(), short.to_owned()];
        let references = with_local_sample(local, &authored);
        assert_eq!(references, [local, short, long]);
        let shapes = voice_shapes(&references);
        assert!(shapes[1].median_sentence_chars < shapes[2].median_sentence_chars);
        assert_eq!(shapes[0].dialogue_percent, shapes[2].dialogue_percent);
    }
}
