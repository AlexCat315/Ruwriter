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

#[cfg(test)]
mod tests {
    use super::*;

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
