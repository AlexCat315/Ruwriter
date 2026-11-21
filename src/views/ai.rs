use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cove_agent::AgentError;
use cove_ui::compose::modifiers::interaction::AccessibilityExt;
use cove_ui::compose::modifiers::layout::{EdgeInsets, FlexExt, LayoutFrameExt, PaddingExt};
use cove_ui::compose::modifiers::style::{AppearanceExt, SemanticStyleViewExt};
use cove_ui::compose::primitives::controls::{Button, MenuButton, Toggle};
use cove_ui::compose::primitives::overlays::{MenuEntry, MenuItem, PopoverPlacement};
use cove_ui::compose::primitives::text::text_field::{TextFieldBehavior, TextFieldEvents};
use cove_ui::compose::primitives::text::{
    MarkdownView, Text, TextBuffer, TextField, TextRun, TextTraits,
};
use cove_ui::compose::theme::styles::buttons::{
    ButtonStyleViewExt, PrimaryButtonStyle, ToggleButtonStyle,
};
use cove_ui::compose::theme::{SemanticColorRef, SemanticFontRef};
use cove_ui::compose::{ScrollAxis, ScrollRequest, ScrollView};
use cove_ui::composition::{AnyView, BoxView, EmptyView, Scancode, Scope, TupleView, View};
use cove_ui::core::scene::{Align, ClipboardHolder, FlexDirection, Justify, Wrap};
use cove_ui::core::{Binding, Point, Signal};
use ruwriter_core::{AiPreferences, AiSettings, MAX_PARALLEL_AI_MODELS};

use super::ui::{hstack, spacer, vstack};
use crate::ai::{
    Batch, Catalog, ModelStatus, StoryStatus, WritingResult, WritingStage, WritingTask,
    is_continuation_command, requires_key, scene_confirmation,
};
use crate::ai_research::ResearchScope;
use crate::ai_review::Review;
use crate::model::Model;

pub struct AiPanel {
    pub model: Model,
    pub docked: bool,
    pub top_inset: f32,
    pub bottom_inset: f32,
}

fn caption(text: impl Into<String>) -> AnyView {
    AnyView::new(
        Text::new(text.into())
            .semantic_font(SemanticFontRef::caption())
            .semantic_foreground(SemanticColorRef::on_surface_muted()),
    )
}

fn text_box(label: &str, buffer: TextBuffer, height: f32) -> AnyView {
    AnyView::new(
        TextField::new(label, Binding::new(move || buffer.clone(), |_| {}))
            .multiline()
            .behavior(TextFieldBehavior {
                read_only: true,
                ..Default::default()
            })
            .frame_height(height),
    )
}

fn primary(label: &str, enabled: bool, action: impl Fn() + 'static) -> AnyView {
    AnyView::new(
        AnyView::new(Button::title(label, action).enabled(enabled))
            .button_style(PrimaryButtonStyle),
    )
}

fn field(label: &str, binding: Binding<TextBuffer>, secret: bool, limit: usize) -> AnyView {
    AnyView::new(TextField::new(label, binding).behavior(TextFieldBehavior {
        secure: secret,
        max_length: Some(limit),
        ..Default::default()
    }))
}

fn preset(label: &str, url: &str, model: &Model) -> AnyView {
    let ai = model.ai.clone();
    let endpoint = TextBuffer::new(url);
    AnyView::new(Button::title(label, move || {
        ai.change_endpoint(endpoint.clone())
    }))
}

fn chooser(label: impl Into<String>, entries: Vec<MenuEntry>, enabled: bool) -> AnyView {
    AnyView::new(
        MenuButton::new(entries, Text::new(label.into()).truncated())
            .composed()
            .enabled(enabled),
    )
}

fn profile_title(profile: &AiPreferences) -> String {
    format!("{} · {}", profile.model, profile.endpoint)
}

impl AiPanel {
    fn settings(&self) -> (Vec<AnyView>, AnyView) {
        let ai = &self.model.ai;
        let settings = self.model.ai_settings();
        let editing = ai.editing.get();
        let selected_profile = editing.and_then(|index| settings.models.get(index));
        let profiles = settings
            .models
            .iter()
            .enumerate()
            .map(|(index, profile)| {
                let model = self.model.clone();
                MenuEntry::Item(
                    MenuItem::new(index as u64 + 1, profile_title(profile), move || {
                        model.edit_ai_profile(Some(index));
                    })
                    .selected(editing == Some(index)),
                )
            })
            .collect();
        let add = self.model.clone();
        let remove = self.model.clone();
        let mut header = vec![
            chooser(
                selected_profile.map_or_else(
                    || "未保存的模型".to_owned(),
                    |profile| profile.model.clone(),
                ),
                profiles,
                !settings.models.is_empty(),
            ),
            AnyView::new(Button::title("添加模型", move || {
                add.edit_ai_profile(None)
            })),
        ];
        if let Some(index) = editing {
            header.push(AnyView::new(Button::title("移除", move || {
                remove.remove_ai_profile(index)
            })));
        }
        let endpoint = ai.endpoint;
        let endpoint_editor = ai.clone();
        let key = ai.api_key;
        let key_editor = ai.clone();
        let address = endpoint.get().string_value();
        let mut rows = vec![
            wrap_controls(header),
            wrap_controls(vec![
                preset("DeepSeek", "https://api.deepseek.com", &self.model),
                preset("Ollama", "http://127.0.0.1:11434/v1", &self.model),
                preset("LM Studio", "http://127.0.0.1:1234/v1", &self.model),
            ]),
            field(
                "API 地址",
                Binding::new(
                    move || endpoint.get(),
                    move |value| endpoint_editor.change_endpoint(value),
                ),
                false,
                2048,
            ),
            field(
                if requires_key(&address) {
                    "DeepSeek API 密钥"
                } else {
                    "API 密钥（可选）"
                },
                Binding::new(move || key.get(), move |value| key_editor.change_key(value)),
                true,
                4096,
            ),
        ];
        let catalog = ai.catalog.get();
        let fetch = ai.clone();
        let loading = matches!(catalog, Catalog::Loading(_));
        let mut catalog_controls = vec![AnyView::new(
            Button::title(
                if loading {
                    "正在获取模型…"
                } else {
                    "获取模型列表"
                },
                move || fetch.fetch_models(),
            )
            .enabled(!loading && !address.is_empty()),
        )];
        match catalog {
            Catalog::Loaded(models) if !models.is_empty() => {
                let field = ai.model;
                let entries = models
                    .iter()
                    .enumerate()
                    .map(|(index, id)| {
                        let name = TextBuffer::new(id);
                        MenuEntry::Item(MenuItem::new(index as u64 + 1, id.clone(), move || {
                            field.set(name.clone())
                        }))
                    })
                    .collect();
                catalog_controls.push(chooser("从列表选择", entries, true));
            }
            Catalog::Loaded(_) => rows.push(caption("服务未返回模型，可以手动填写。")),
            Catalog::Failed(AgentError::HttpStatus(401 | 403)) => {
                rows.push(error_label("请检查此服务的 API 密钥。".to_owned()))
            }
            Catalog::Failed(error) => rows.push(caption(format!(
                "无法获取模型列表，可手动填写模型名称。{error}"
            ))),
            _ => {}
        }
        rows.push(wrap_controls(catalog_controls));
        rows.push(field("模型名称", ai.model.binding(), false, 512));
        rows.push(AnyView::new(Toggle::labeled(
            "允许模型查阅文稿",
            ai.tools.binding(),
        )));
        rows.push(caption("同一 API 地址共用会话密钥。"));
        let model = self.model.clone();
        let action = primary(
            "保存模型",
            !ai.endpoint.get().is_empty() && !ai.model.get().is_empty(),
            move || model.save_ai_preferences(),
        );
        (rows, action)
    }

    fn brief_tabs(&self) -> AnyView {
        let page = self.model.ai.brief_page;
        let selected = self.model.ai.brief_item;
        AnyView::new(
            wrap_controls(
                ["大纲", "角色卡", "关系", "记忆", "DiffData", "文风"]
                    .into_iter()
                    .enumerate()
                    .map(|(index, label)| {
                        AnyView::new(
                            Button::title(label, move || {
                                page.set(index);
                                selected.set(0);
                            })
                            .selected(page.get() == index)
                            .button_style(ToggleButtonStyle::default()),
                        )
                    })
                    .collect(),
            )
            .padding_edges(14.0, 4.0),
        )
    }

    fn brief(&self) -> (Vec<AnyView>, AnyView) {
        let ai = &self.model.ai;
        let mut rows = Vec::new();
        let page = ai.brief_page;
        let selected = ai.brief_item;
        match page.get() {
            1 => {
                if let Some(index) =
                    record_picker(&mut rows, ai.story_cards, selected, "添加角色", |card| {
                        card.name.clone()
                    })
                {
                    let cards = ai.story_cards;
                    rows.push(study_field(
                        "姓名",
                        record_field(cards, index, |c| &c.name, |c, v| c.name = v),
                        44.0,
                    ));
                    rows.push(study_field(
                        "表面言行",
                        record_field(cards, index, |c| &c.surface, |c, v| c.surface = v),
                        70.0,
                    ));
                    rows.push(study_field(
                        "内核与动机",
                        record_field(cards, index, |c| &c.motive, |c, v| c.motive = v),
                        100.0,
                    ));
                    rows.push(study_field(
                        "什么会触发自我保护",
                        record_field(cards, index, |c| &c.defense, |c, v| c.defense = v),
                        93.0,
                    ));
                    rows.push(study_field(
                        "不能写反的边界",
                        record_field(cards, index, |c| &c.boundary, |c, v| c.boundary = v),
                        93.0,
                    ));
                }
            }
            2 => {
                if let Some(index) = record_picker(
                    &mut rows,
                    ai.story_relationships,
                    selected,
                    "添加关系",
                    |edge| {
                        if edge.from.is_empty() && edge.to.is_empty() {
                            String::new()
                        } else {
                            format!("{} → {}", edge.from, edge.to)
                        }
                    },
                ) {
                    let edges = ai.story_relationships;
                    let cards = ai.story_cards.get();
                    for (label, field) in [
                        (
                            "从",
                            record_field(edges, index, |e| &e.from, |e, v| e.from = v),
                        ),
                        ("到", record_field(edges, index, |e| &e.to, |e, v| e.to = v)),
                    ] {
                        let name = field.get().string_value();
                        let entries = cards
                            .iter()
                            .filter(|c| !c.name.trim().is_empty())
                            .enumerate()
                            .map(|(i, card)| {
                                let value = TextBuffer::new(&card.name);
                                let binding = field.clone();
                                MenuEntry::Item(
                                    MenuItem::new(i as u64 + 1, card.name.clone(), move || {
                                        binding.set(value.clone())
                                    })
                                    .selected(card.name == name),
                                )
                            })
                            .collect();
                        rows.push(wrap_controls(vec![
                            caption(label),
                            chooser(
                                if name.is_empty() {
                                    "选择角色".into()
                                } else {
                                    name
                                },
                                entries,
                                !cards.is_empty(),
                            ),
                        ]));
                    }
                    rows.push(study_field(
                        "当前关系",
                        record_field(edges, index, |e| &e.state, |e, v| e.state = v),
                        70.0,
                    ));
                    rows.push(study_field(
                        "希望怎样变化",
                        record_field(edges, index, |e| &e.direction, |e, v| e.direction = v),
                        95.0,
                    ));
                    rows.push(study_field(
                        "现阶段别跨过的分寸",
                        record_field(edges, index, |e| &e.boundary, |e, v| e.boundary = v),
                        95.0,
                    ));
                }
            }
            3 => {
                let memories = ai.writing_memories;
                if let Some(index) =
                    record_picker(&mut rows, memories, selected, "添加记忆", |entry| {
                        entry.title.clone()
                    })
                {
                    rows.push(study_field(
                        "主题",
                        record_field(memories, index, |m| &m.title, |m, v| m.title = v),
                        48.0,
                    ));
                    rows.push(study_field(
                        "关联角色或情节",
                        record_field(memories, index, |m| &m.subject, |m, v| m.subject = v),
                        48.0,
                    ));
                    rows.push(study_field(
                        "已确认的记忆",
                        record_field(memories, index, |m| &m.text, |m, v| m.text = v),
                        110.0,
                    ));
                    rows.push(study_field(
                        "来源或原文位置",
                        record_field(memories, index, |m| &m.source, |m, v| m.source = v),
                        66.0,
                    ));
                    rows.push(wrap_controls(vec![
                        AnyView::new(Toggle::labeled(
                            "置顶",
                            record_toggle(memories, index, |m| m.pinned, |m, v| m.pinned = v),
                        )),
                        AnyView::new(Toggle::labeled(
                            "待复核",
                            record_toggle(memories, index, |m| m.held, |m, v| m.held = v),
                        )),
                    ]));
                }
            }
            4 => {
                let feedback = ai.reviewed_feedback;
                if let Some(index) = record_picker(
                    &mut rows,
                    feedback,
                    selected,
                    "新建改稿对照",
                    |entry| entry.instruction.clone(),
                ) {
                    rows.push(study_field(
                        "修改意图",
                        record_field(
                            feedback,
                            index,
                            |f| &f.instruction,
                            |f, v| f.instruction = v,
                        ),
                        66.0,
                    ));
                    rows.push(study_field(
                        "适用关键词（用顿号分隔）",
                        record_field(feedback, index, |f| &f.subject, |f, v| f.subject = v),
                        50.0,
                    ));
                    rows.push(study_field(
                        "以后应沿用的改法",
                        record_field(feedback, index, |f| &f.lesson, |f, v| f.lesson = v),
                        84.0,
                    ));
                    rows.push(study_field(
                        "改稿来源",
                        record_field(feedback, index, |f| &f.source, |f, v| f.source = v),
                        54.0,
                    ));
                    if let Some(entry) = feedback.get().get(index)
                        && let Some(change) =
                            cove_agent::revision::change_span(&entry.before, &entry.after)
                    {
                        rows.push(writing_change(
                            "移除",
                            &entry.before,
                            change.before,
                            TextTraits::STRIKETHROUGH,
                        ));
                        rows.push(writing_change(
                            "增加",
                            &entry.after,
                            change.after,
                            TextTraits::BOLD,
                        ));
                    }
                    rows.push(study_field(
                        "改前",
                        record_field(feedback, index, |f| &f.before, |f, v| f.before = v),
                        108.0,
                    ));
                    rows.push(study_field(
                        "作者认可的改写",
                        record_field(feedback, index, |f| &f.after, |f, v| f.after = v),
                        108.0,
                    ));
                    rows.push(AnyView::new(Toggle::labeled(
                        "暂停用于续写",
                        record_toggle(feedback, index, |f| f.held, |f, v| f.held = v),
                    )));
                }
            }
            5 => {
                rows.push(study_field("文风要求", ai.voice.binding(), 95.0));
                rows.push(study_field("避免的词语或句式", ai.avoid.binding(), 75.0));
                if ai
                    .target
                    .get()
                    .is_some_and(|target| target.selection.is_some())
                {
                    let take = self.model.clone();
                    rows.push(wrap_controls(vec![AnyView::new(Button::title(
                        "用正文选区作为样段",
                        move || take.ai_voice_from_selection(),
                    ))]));
                }
                for (index, sample) in ai.voice_samples.get().iter().enumerate() {
                    let remove = ai.voice_samples;
                    rows.push(wrap_controls(vec![
                        caption(sample.chars().take(90).collect::<String>()),
                        AnyView::new(Button::title("移除此样段", move || {
                            remove.update(|samples| {
                                if index < samples.len() {
                                    samples.remove(index);
                                }
                            })
                        })),
                    ]));
                }
            }
            _ => {
                rows.push(study_field("剧情大纲", ai.story_outline.binding(), 165.0));
                rows.push(study_field("待续线索", ai.story_threads.binding(), 92.0));
                rows.push(study_field("题材", ai.story_genre.binding(), 46.0));
                rows.push(study_field("叙事口味", ai.story_tone.binding(), 62.0));
                rows.push(study_field("创作方向", ai.story_direction.binding(), 82.0));
                rows.push(study_field(
                    "卡片之外的角色概览",
                    ai.story_characters.binding(),
                    82.0,
                ));
                rows.push(study_field(
                    "角色说法与知情范围",
                    ai.story_claims.binding(),
                    100.0,
                ));
                rows.push(study_field(
                    "私设与揭晓点 · 审稿用",
                    ai.story_truth.binding(),
                    100.0,
                ));
                rows.push(study_field("硬设定", ai.canon.binding(), 95.0));
            }
        }
        let save = self.model.clone();
        (
            rows,
            primary("保存作品资料", true, move || save.save_ai_brief()),
        )
    }

    fn writer(
        &self,
        preview_context: Signal<bool>,
        show_controls: Signal<bool>,
        show_beats: Signal<bool>,
    ) -> (Vec<AnyView>, AnyView) {
        let ai = &self.model.ai;
        let batch = ai.batch();
        let story = ai.story_status();
        let preparing_story = matches!(story, Some(StoryStatus::Preparing { .. }));
        let busy = batch.as_ref().is_some_and(Batch::busy);
        let index = ai.viewed_result.get();
        let prompt = ai.prompt.get().string_value();
        let confirming = scene_confirmation(&prompt).is_some();
        let has_question = batch
            .as_ref()
            .is_some_and(|batch| batch.scene_question(index).is_some());
        let answering = ai.follow_draft.get() && has_question;
        let following =
            ai.follow_draft.get() && batch.as_ref().is_some_and(|batch| batch.can_revise(index));
        let starts_continuation = following && is_continuation_command(&prompt);
        let revising = following && !starts_continuation;
        let mut rows = Vec::new();
        let history = batch
            .as_ref()
            .map_or_else(|| ai.archived_chat(), |batch| batch.history.clone());
        for turn in history.iter() {
            rows.push(author_message(&turn.author));
            if let Some(answer) = &turn.answer {
                let copy = answer.clone();
                rows.push(hstack(
                    4.0,
                    Align::Center,
                    vec![
                        caption(if turn.adopted { "已采纳" } else { "AI" }),
                        spacer(),
                        AnyView::new(Button::title("复制", move || {
                            ClipboardHolder::write(&copy.string_value());
                        })),
                    ],
                ));
                rows.push(assistant_message(answer, turn.markdown));
            }
        }
        if let Some(batch) = &batch {
            if let Some(error) = &batch.archive_error {
                rows.push(error_label(format!("无法保存对话。{error}")));
            }
            rows.push(author_message(&batch.prompt));
            self.results(&mut rows, batch, show_beats);
        }
        let mode = if starts_continuation || answering {
            WritingTask::Continue
        } else {
            ai.task.get()
        };
        let continuing = mode == WritingTask::Continue;
        let needs_story = continuing && !self.model.ai_story().is_ready();
        if let Some(status) = &story {
            self.story_progress(&mut rows, status);
        }
        let polishing = mode == WritingTask::Polish;
        let include = mode != WritingTask::Request || ai.include_context.get();
        let target = ai.target.get();
        let context = if continuing {
            target.as_ref().map_or_else(
                || Ok(TextBuffer::empty()),
                |target| target.continuation_context(),
            )
        } else {
            ai.context.get()
        };
        let has_context = context
            .as_ref()
            .is_ok_and(|text| !text.string_value().trim().is_empty());
        let selected = target
            .as_ref()
            .is_some_and(|target| target.selection.is_some());
        let preferences = self.model.ai_settings();
        let can_research = !preferences.selected.is_empty();
        let scope = ai.scope.get();
        let folder = self.model.ai_project_folder();
        let research_label = match scope {
            ResearchScope::Excerpt => "当前片段",
            ResearchScope::Document => "本文稿",
            ResearchScope::Project if folder.as_deref() == Some("") => "整个资料库",
            ResearchScope::Project => "作品文件夹",
        };

        let mut modes = Vec::new();
        if has_question {
            let model = self.model.clone();
            modes.push(MenuEntry::Item(
                MenuItem::new(1, "补充场景方向", move || {
                    model.ai.task.set(WritingTask::Continue);
                    model.ai.follow_draft.set(true);
                })
                .selected(answering),
            ));
        } else if batch.as_ref().is_some_and(|batch| batch.can_revise(index)) {
            let follow = ai.follow_draft;
            modes.push(MenuEntry::Item(
                MenuItem::new(1, "修改当前草稿", move || follow.set(true)).selected(revising),
            ));
        }
        for (i, (label, task, enabled)) in [
            ("续写", WritingTask::Continue, target.is_some()),
            ("润色选区", WritingTask::Polish, selected),
            ("问答", WritingTask::Request, true),
        ]
        .into_iter()
        .enumerate()
        {
            if enabled {
                let model = self.model.clone();
                modes.push(MenuEntry::Item(
                    MenuItem::new(i as u64 + 2, label, move || {
                        model.ai.follow_draft.set(false);
                        model.ai.task.set(task);
                        if let Some(request) =
                            scene_confirmation(&model.ai.prompt.get_untracked().string_value())
                        {
                            model.ai.prompt.set(TextBuffer::new(request));
                        }
                        model.refresh_ai_target();
                    })
                    .selected(!answering && !confirming && !revising && mode == task),
                ));
            }
        }
        if target.is_some() {
            let model = self.model.clone();
            modes.push(MenuEntry::Item(
                MenuItem::new(5, "先确认本场", move || {
                    let prompt = model.ai.prompt.get_untracked().string_value();
                    if scene_confirmation(&prompt).is_none() {
                        model
                            .ai
                            .prompt
                            .set(TextBuffer::new(&format!("先确认：{prompt}")));
                    }
                    model.ai.task.set(WritingTask::Continue);
                    model.ai.follow_draft.set(false);
                    model.refresh_ai_target();
                })
                .selected(confirming && continuing && !answering),
            ));
        }
        let mode_label = if answering {
            "补充场景方向"
        } else if confirming && continuing {
            "先确认本场"
        } else if revising {
            "修改当前草稿"
        } else {
            match mode {
                WritingTask::Continue => "续写",
                WritingTask::Polish => "润色选区",
                WritingTask::Request => "问答",
            }
        };

        if show_controls.get() {
            let manage = self.model.clone();
            let mut model_controls = vec![model_picker(&self.model, &preferences, true)];
            if preferences.models.len() > 1 {
                model_controls.push(parallel_picker(&self.model, &preferences, true));
            }
            model_controls.push(AnyView::new(Button::title("管理模型", move || {
                manage.manage_ai()
            })));
            rows.push(wrap_controls(model_controls));
            if self.model.ai_work().is_some() && !self.model.ai_brief().is_empty() {
                rows.push(AnyView::new(Toggle::labeled(
                    "采用作品资料",
                    ai.use_brief.binding(),
                )));
            }
            if can_research && target.is_some() && !polishing {
                let mut scopes = vec![
                    ("当前片段", ResearchScope::Excerpt),
                    ("本文稿", ResearchScope::Document),
                ];
                if folder.is_some() {
                    scopes.push((
                        if folder.as_deref() == Some("") {
                            "整个资料库"
                        } else {
                            "作品文件夹"
                        },
                        ResearchScope::Project,
                    ));
                }
                let entries = scopes
                    .into_iter()
                    .enumerate()
                    .map(|(index, (label, value))| {
                        let scope_signal = ai.scope;
                        MenuEntry::Item(
                            MenuItem::new(index as u64 + 1, label, move || scope_signal.set(value))
                                .selected(scope == value),
                        )
                    })
                    .collect();
                rows.push(wrap_controls(vec![
                    caption("检索范围"),
                    chooser(research_label, entries, true),
                ]));
                if scope != ResearchScope::Excerpt {
                    rows.push(caption(
                        "在本机检索。所选模型会收到相关原文，支持工具的模型可以继续查阅。",
                    ));
                }
                if !needs_story && !preparing_story {
                    let refresh = self.model.clone();
                    rows.push(AnyView::new(
                        Button::title("重新梳理剧情与人物", move || {
                            refresh.prepare_ai_story()
                        })
                        .enabled(!busy && has_context),
                    ));
                }
            }
            if has_context {
                let mut controls = Vec::new();
                if mode == WritingTask::Request {
                    controls.push(AnyView::new(Toggle::labeled(
                        if selected {
                            "附带选区"
                        } else {
                            "附带前文"
                        },
                        ai.include_context.binding(),
                    )));
                }
                if include || polishing {
                    controls.push(AnyView::new(Button::title(
                        if preview_context.get() {
                            "收起原文"
                        } else {
                            "查看参考原文"
                        },
                        move || preview_context.update(|shown| *shown = !*shown),
                    )));
                }
                if !controls.is_empty() {
                    rows.push(wrap_controls(controls));
                }
            }
            if let Ok(text) = &context
                && (include || polishing)
                && !text.is_empty()
                && preview_context.get()
            {
                rows.push(text_box("提供给模型的片段", text.clone(), 100.0));
            }
        }
        if let Err(message) = context
            && (include || polishing)
        {
            rows.push(caption(message));
        }

        let ready = !busy
            && !preparing_story
            && !(needs_story && matches!(story, Some(StoryStatus::Ready(_))))
            && if revising || answering {
                !prompt.trim().is_empty()
            } else {
                match mode {
                    WritingTask::Continue => target.is_some() && has_context,
                    WritingTask::Polish => selected && has_context,
                    WritingTask::Request => !prompt.trim().is_empty(),
                }
            };
        let model = self.model.clone();
        let send: Rc<dyn Fn()> = Rc::new(move || model.send_ai_message());
        let action = if preparing_story {
            let ai = ai.clone();
            AnyView::new(Button::title("停止梳理", move || ai.clear_story()))
        } else if busy {
            let ai = ai.clone();
            AnyView::new(Button::title("停止", move || ai.stop()))
        } else if preferences.models.is_empty() {
            let model = self.model.clone();
            primary("添加模型", true, move || model.manage_ai())
        } else {
            let submit = send.clone();
            primary(
                if answering {
                    "回答并继续"
                } else if needs_story {
                    "先梳理作品"
                } else {
                    "发送"
                },
                ready,
                move || submit(),
            )
        };
        let mut composer = Vec::new();
        if needs_story && story.is_none() {
            composer.push(caption(if scope == ResearchScope::Project {
                "先阅读所选作品范围，整理剧情与人物。"
            } else {
                "先阅读本文稿落笔点之前的正文，整理剧情与人物。"
            }));
        }
        if continuing
            && !revising
            && let Some(target) = &target
        {
            composer.push(caption(target.continuation_anchor().map_or_else(
                || "请把光标放在需要接着写的正文之后".to_owned(),
                |anchor| format!("接在「{anchor}」后"),
            )));
        }
        composer.push(AnyView::new(
            TextField::new(
                match (answering, revising, mode) {
                    (true, _, _) => "补充这一场的方向…",
                    (_, true, _) => "接着说你想怎么改…",
                    (_, _, WritingTask::Continue) => "这一段接下来发生什么？",
                    (_, _, WritingTask::Polish) => "这段正文要怎么改？",
                    (_, _, WritingTask::Request) => "问一个关于作品的问题…",
                },
                ai.prompt.binding(),
            )
            .multiline()
            .events(TextFieldEvents {
                on_key_down: Some(Rc::new(move |event| {
                    if ready
                        && !event.is_repeat
                        && matches!(event.scancode, Scancode::RETURN | Scancode::KEYPAD_ENTER)
                        && (event.modifiers.is_empty()
                            || crate::shortcuts::has_primary(event.modifiers))
                        && !event.modifiers.has_shift()
                        && !crate::shortcuts::has_composition()
                    {
                        send();
                        return true;
                    }
                    false
                })),
                ..Default::default()
            })
            .frame_height(72.0),
        ));
        let settings = AnyView::new(
            Button::title(
                if show_controls.get() {
                    "收起资料"
                } else {
                    "参考资料与模型"
                },
                move || show_controls.update(|shown| *shown = !*shown),
            )
            .enabled(!busy),
        );
        composer.push(hstack(
            6.0,
            Align::Center,
            vec![
                chooser(mode_label, modes, !busy),
                settings,
                spacer(),
                action,
            ],
        ));
        (rows, vstack(7.0, Align::Stretch, composer))
    }

    fn story_progress(&self, rows: &mut Vec<AnyView>, status: &StoryStatus) {
        match status {
            StoryStatus::Preparing { batch, .. } => {
                rows.push(caption(if *batch == 0 {
                    "正在读取文稿目录…".to_owned()
                } else {
                    format!("正在分批整理剧情与人物 · 第 {batch} 批")
                }));
            }
            StoryStatus::Ready(draft) => {
                rows.push(caption(draft.coverage.clone()));
                rows.push(caption(format!(
                    "建议题材 · {}　叙事口味 · {}",
                    draft.profile.genre, draft.profile.tone
                )));
                let page = self.model.ai.story_page;
                let current = page.get();
                let sections = [
                    ("剧情大纲", &draft.profile.outline),
                    ("人物与关系", &draft.profile.characters),
                    ("待续线索", &draft.profile.open_threads),
                    ("角色说法", &draft.profile.character_claims),
                ];
                rows.push(wrap_controls(
                    sections
                        .iter()
                        .enumerate()
                        .map(|(index, (label, _))| {
                            AnyView::new(
                                Button::title(*label, move || page.set(index))
                                    .selected(index == current)
                                    .button_style(ToggleButtonStyle::default()),
                            )
                        })
                        .collect(),
                ));
                if current == 1 && !draft.profile.character_cards.is_empty() {
                    let cards = draft
                        .profile
                        .character_cards
                        .iter()
                        .map(|card| {
                            format!(
                                "**{}**  \n表面 · {}  \n内核 · {}  \n防御 · {}  \n边界 · {}",
                                card.name, card.surface, card.motive, card.defense, card.boundary
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    rows.push(AnyView::new(
                        ScrollView::new(ScrollAxis::Vertical, MarkdownView::new(cards))
                            .frame_height(185.0),
                    ));
                } else if let Some((label, content)) = sections.get(current) {
                    rows.push(text_box(label, TextBuffer::new(content), 145.0));
                }
                if !draft.profile.has_character_cards() {
                    let retry = self.model.clone();
                    rows.push(wrap_controls(vec![
                        caption("请先核对角色姓名、动机及关系卡。"),
                        AnyView::new(Button::title("重新提取角色卡", move || {
                            retry.prepare_ai_story()
                        })),
                    ]));
                }
                let direction = self.model.ai.story_direction;
                rows.push(AnyView::new(
                    TextField::new(
                        "接下来想怎么发展？关系、节奏或哪些真相先别揭晓",
                        direction.binding(),
                    )
                    .multiline()
                    .frame_height(75.0),
                ));
                if direction.get().is_empty() {
                    rows.push(AnyView::new(Button::title("沿现有主线，保持原稿口味", move || {
                        direction.set(TextBuffer::new("沿现有主线和人物关系小步推进，保持原稿口味，未揭露的信息继续保留。"));
                    })));
                }
                let confirm = self.model.clone();
                let edit = self.model.clone();
                let ready = !direction.get().string_value().trim().is_empty()
                    && !draft.profile.genre.trim().is_empty()
                    && !draft.profile.tone.trim().is_empty()
                    && draft.profile.has_character_cards();
                let mut actions = vec![primary("确认并续写", ready, move || {
                    confirm.confirm_ai_story()
                })];
                if self.model.ai_work().is_some() {
                    actions.push(AnyView::new(Button::title(
                        "调整作品资料",
                        move || edit.edit_prepared_story(),
                    )));
                }
                rows.push(wrap_controls(actions));
            }
            StoryStatus::Failed(error) => {
                rows.push(error_label(format!("未能完成作品梳理。{error}")))
            }
            StoryStatus::Confirmed(_) => {}
        }
    }

    fn results(&self, rows: &mut Vec<AnyView>, batch: &Batch, show_beats: Signal<bool>) {
        let selected = batch
            .target
            .as_ref()
            .is_some_and(|target| target.selection.is_some());
        let has_target = batch.target.is_some();
        let docked = self.docked;
        let ai = &self.model.ai;
        let current = ai.viewed_result.get();
        let Some(run) = batch.runs.get(current) else {
            return;
        };
        if batch.runs.len() > 1 {
            let tabs = batch
                .runs
                .iter()
                .enumerate()
                .map(|(index, run)| {
                    let ai = ai.clone();
                    let label = format!("{} · {}", run.profile.model, status_title(&run.status));
                    AnyView::new(
                        Button::new(Text::new(label).truncated(), move || {
                            ai.select_draft(index);
                        })
                        .selected(index == current)
                        .button_style(ToggleButtonStyle::default()),
                    )
                })
                .collect();
            rows.push(wrap_controls(tabs));
        }
        match &run.status {
            ModelStatus::Indexing => rows.push(caption("正在建立本地章节索引…")),
            ModelStatus::Running(stage) => {
                rows.push(caption(match stage {
                    WritingStage::Research => "正在梳理接续点和可用的章节证据…",
                    WritingStage::Draft => "正在起草…",
                    WritingStage::Review => "正在核对剧情连续性、视角和语气…",
                    WritingStage::Revise => "正在修订当前草稿…",
                }));
                if !run.streamed_text.is_empty() && !matches!(stage, WritingStage::Review) {
                    rows.push(assistant_message(
                        &run.streamed_text,
                        batch.writing_task() == WritingTask::Request,
                    ));
                } else if let Some(draft) = run.draft_in_progress() {
                    rows.push(assistant_message(&draft.text, false));
                }
            }
            ModelStatus::Cancelled => rows.push(caption("已停止等待这个模型。")),
            ModelStatus::NeedsDirection(question) => {
                rows.push(caption(format!("原文 · 「{}」", question.quote)));
                rows.push(assistant_message(
                    &TextBuffer::new(&question.question),
                    false,
                ));
            }
            ModelStatus::Failed(AgentError::HttpStatus(401 | 403)) => {
                let missing = ai.missing_key(&run.profile.endpoint);
                rows.push(error_label(if missing {
                    "此服务需要 API 密钥。".to_owned()
                } else {
                    "此服务拒绝了 API 凭据。".to_owned()
                }));
                let index = self.model.ai_settings().models.iter().position(|profile| {
                    profile.model == run.profile.model && profile.endpoint == run.profile.endpoint
                });
                if let Some(index) = index {
                    let model = self.model.clone();
                    rows.push(wrap_controls(vec![AnyView::new(
                        Button::title(
                            if missing {
                                "填写 API 密钥"
                            } else {
                                "更新此服务密钥"
                            },
                            move || model.edit_ai_profile(Some(index)),
                        )
                        .enabled(!batch.busy()),
                    )]));
                }
            }
            ModelStatus::Failed(error) => {
                rows.push(error_label(format!("请求失败。{error}")));
                let retry = self.model.clone();
                rows.push(wrap_controls(vec![AnyView::new(
                    Button::title("按当前落笔位置重试", move || retry.retry_ai())
                        .enabled(!batch.busy()),
                )]));
            }
            ModelStatus::Ready(result) => {
                if !batch.applied()
                    && let Some(anchor) = batch
                        .target
                        .as_ref()
                        .and_then(|target| target.continuation_anchor())
                    && result.task == WritingTask::Continue
                {
                    rows.push(caption(format!("接在「{anchor}」后")));
                }
                if let Some(error) = &run.revision_error {
                    rows.push(error_label(format!("这次修订失败。{error}")));
                }

                let copied = result.text.clone();
                let feedback = ai.feedback;
                let mut controls = vec![
                    caption(if batch.adopted == Some(current) {
                        "已采纳草稿"
                    } else if result.task == WritingTask::Request {
                        "AI"
                    } else {
                        "草稿"
                    }),
                    AnyView::new(Button::title("复制草稿", move || {
                        ClipboardHolder::write(&copied.string_value());
                        feedback.set("已复制草稿。".to_owned());
                    })),
                ];
                let editing = ai.edit_candidate.get();
                if !batch.applied() && result.task != WritingTask::Request && !batch.busy() {
                    let edit = ai.edit_candidate;
                    controls.push(AnyView::new(Button::title(
                        if editing {
                            "完成编辑"
                        } else {
                            "编辑草稿"
                        },
                        move || edit.update(|active| *active = !*active),
                    )));
                }
                let modified = result.text != result.original_text;
                let binding = ai.draft_binding(batch, current);
                if !batch.applied() && modified {
                    let original = result.original_text.clone();
                    let restore = binding.clone();
                    controls.push(AnyView::new(Button::title("恢复生成稿", move || {
                        restore.set(original.clone());
                    })));
                }
                if run.previous.is_some() {
                    let previous = ai.show_previous;
                    controls.push(AnyView::new(Button::title(
                        if previous.get() {
                            "收起上一版"
                        } else {
                            "查看上一版"
                        },
                        move || previous.update(|shown| *shown = !*shown),
                    )));
                }
                if self.model.ai_work().is_some() && run.feedback_example().is_some() {
                    let model = self.model.clone();
                    controls.push(AnyView::new(Button::title(
                        "记住这次改法",
                        move || {
                            model.remember_ai_revision(current, false);
                        },
                    )));
                }
                if self.model.ai_work().is_some()
                    && self.model.caret.get().selected_range.is_some()
                    && result.task != WritingTask::Request
                {
                    let model = self.model.clone();
                    controls.push(AnyView::new(Button::title(
                        "用正文选区作为改写版",
                        move || {
                            model.remember_ai_revision(current, true);
                        },
                    )));
                }
                if !batch.applied() && editing && result.task != WritingTask::Request {
                    let lines = result
                        .text
                        .line_count()
                        .max(result.text.character_count().div_ceil(22));
                    let save = ai.clone();
                    rows.push(AnyView::new(
                        TextField::new("可编辑草稿", binding)
                            .multiline()
                            .events(TextFieldEvents {
                                on_blur: Some(Rc::new(move || save.flush_chat())),
                                ..Default::default()
                            })
                            .frame_height((lines as f32 * 22.0 + 20.0).clamp(92.0, 230.0)),
                    ));
                } else {
                    rows.push(assistant_message(
                        &result.text,
                        result.task == WritingTask::Request,
                    ));
                }
                rows.push(wrap_controls(controls));
                if ai.show_previous.get()
                    && let Some(previous) = &run.previous
                {
                    rows.push(text_box("上一版", previous.text.clone(), 140.0));
                    if !batch.applied() && !batch.busy() {
                        let ai = ai.clone();
                        rows.push(AnyView::new(Button::title("恢复上一版", move || {
                            ai.restore_previous(current);
                        })));
                    }
                }
                if has_target && !batch.applied() {
                    let ready = result.can_adopt();
                    if result.task != WritingTask::Request {
                        if result.text != result.reviewed_text {
                            rows.push(caption("当前编辑稿尚未审阅。"));
                        } else if result.repetition_warning.is_some()
                            || !result.continuity_notes.is_empty()
                            || matches!(&result.review, Review::Checked(issues) if !issues.is_empty())
                        {
                            rows.push(caption("审稿有待核对项，见下方意见。"));
                        }
                    }
                    let insert = self.model.clone();
                    let replace = self.model.clone();
                    let mut actions = Vec::new();
                    if result.task == WritingTask::Continue {
                        actions.push(primary("采纳并接入正文", ready, move || {
                            insert.apply_ai(current, false, docked)
                        }));
                    } else if result.task == WritingTask::Polish && selected {
                        actions.push(primary("采纳并替换选区", ready, move || {
                            replace.apply_ai(current, true, docked);
                        }));
                    } else if result.task == WritingTask::Request {
                        actions.push(primary("插入正文", ready, move || {
                            insert.apply_ai(current, false, docked);
                        }));
                        if selected {
                            actions.push(AnyView::new(
                                Button::title("替换选区", move || {
                                    replace.apply_ai(current, true, docked);
                                })
                                .enabled(ready),
                            ));
                        }
                    }
                    if !actions.is_empty() {
                        rows.push(wrap_controls(actions));
                    }
                }
                self.review_candidate(rows, result, current, !batch.applied() && !batch.busy());
                if let Some(plan) = result.scene_plan()
                    && (!plan.character_beats().is_empty() || !plan.revision_lessons().is_empty())
                {
                    let shown = show_beats.get();
                    rows.push(AnyView::new(Button::title(
                        if shown {
                            "收起场景推演"
                        } else {
                            "查看场景推演"
                        },
                        move || show_beats.set(!shown),
                    )));
                    if shown {
                        for beat in plan.character_beats() {
                            rows.push(vstack(
                                4.0,
                                Align::Stretch,
                                vec![
                                    AnyView::new(Text::runs(vec![TextRun::with_traits(
                                        &beat.character,
                                        TextTraits::BOLD,
                                    )])),
                                    caption(format!("触发 · {}", beat.trigger)),
                                    caption(format!("原文 · 「{}」", beat.trigger_quote.trim())),
                                    caption(format!("反应 · {}", beat.surface_response)),
                                    caption(format!("关系 · {}", beat.relation_effect)),
                                ],
                            ));
                        }
                        for lesson in plan.revision_lessons() {
                            rows.push(vstack(
                                4.0,
                                Align::Stretch,
                                vec![
                                    caption(format!("沿用改法 · {}", lesson.lesson)),
                                    caption(format!("改稿依据 · 「{}」", lesson.basis.trim())),
                                ],
                            ));
                        }
                    }
                }
                show_sources(rows, result, ai.viewed_source);
            }
        }
    }

    fn review_candidate(
        &self,
        rows: &mut Vec<AnyView>,
        result: &WritingResult,
        index: usize,
        available: bool,
    ) {
        if result.task == WritingTask::Request {
            return;
        }
        let ai = &self.model.ai;
        for note in result.continuity_notes.iter() {
            rows.push(caption(format!("接续待核对 · {note}")));
        }
        let changed = result.text != result.reviewed_text;
        let mut instructions = Vec::new();
        if result.review.has_character_violation() {
            rows.push(error_label(
                "草稿偏离了角色卡或关系边界，修正后才能采纳。".to_owned(),
            ));
        }
        if result.review.has_disclosure() {
            rows.push(error_label(
                "草稿泄露了尚不该揭晓的信息，核对修订后才能采纳。".to_owned(),
            ));
        }
        if result.needs_private_review() {
            rows.push(caption("当前版本需要核对角色卡和知情边界，才能采纳。"));
        }
        if changed {
            rows.push(caption("草稿已编辑，可以重新审稿。"));
        } else {
            if let Some(warning) = &result.repetition_warning {
                rows.push(error_label(warning.clone()));
                instructions.push(warning.clone());
            }
            match &result.review {
                Review::NotRequested => {}
                Review::Incomplete => rows.push(caption("本轮未能完成模型审稿。")),
                Review::Rejected => rows.push(error_label(
                    "这条回复没有交付正文。请修订后重新审稿。".to_owned(),
                )),
                Review::Checked(issues) if issues.is_empty() && instructions.is_empty() => {
                    rows.push(caption("审稿未发现具体问题。"));
                }
                Review::Checked(issues) => {
                    for issue in issues {
                        let source = issue
                            .source
                            .as_ref()
                            .map_or_else(String::new, |id| format!(" [{id}]"));
                        let note =
                            format!("「{}」 {}{source}", issue.quote.trim(), issue.reason.trim());
                        rows.push(caption(format!("{} · {note}", issue.kind.label())));
                    }
                }
            }
        }
        if !available {
            return;
        }
        let refine = ai.clone();
        let mut actions = vec![AnyView::new(Button::title("重新审稿", move || {
            submit_refinement(&refine, index, None);
        }))];
        if !changed && let Some(direction) = result.review_revision_instruction() {
            instructions.push(direction);
        }
        if !instructions.is_empty() {
            let refine = ai.clone();
            let instructions = instructions.join("\n");
            actions.push(AnyView::new(Button::title(
                "按审稿意见修订",
                move || {
                    submit_refinement(&refine, index, Some(instructions.clone()));
                },
            )));
        }
        rows.push(wrap_controls(actions));
    }
}

fn author_message(text: &TextBuffer) -> AnyView {
    let bubble = vstack(
        3.0,
        Align::Stretch,
        vec![caption("你"), AnyView::new(Text::new(text.string_value()))],
    )
    .padding_edges(12.0, 8.0)
    .semantic_background(SemanticColorRef::accent_muted())
    .corner_radius(10.0)
    .flex_with(0.0, 1.0, None);
    hstack(0.0, Align::FlexStart, vec![spacer(), AnyView::new(bubble)])
}

fn writing_change(
    label: &str,
    text: &str,
    range: std::ops::Range<usize>,
    style: TextTraits,
) -> AnyView {
    let start = text.floor_char_boundary(range.start.saturating_sub(105));
    let end = text.floor_char_boundary((range.end + 105).min(text.len()));
    let highlighted_end = text.floor_char_boundary((range.start + 620).min(range.end));
    AnyView::new(
        vstack(
            5.0,
            Align::Stretch,
            vec![
                caption(label),
                AnyView::new(Text::runs(vec![
                    TextRun::new(&text[start..range.start]),
                    TextRun::with_traits(&text[range.start..highlighted_end], style),
                    TextRun::new(if highlighted_end < range.end {
                        "…"
                    } else {
                        &text[range.end..end]
                    }),
                ])),
            ],
        )
        .padding_edges(10.0, 7.0)
        .semantic_background(SemanticColorRef::surface())
        .corner_radius(7.0),
    )
}

fn study_field(label: &str, value: Binding<TextBuffer>, height: f32) -> AnyView {
    vstack(
        4.0,
        Align::Stretch,
        vec![
            caption(label),
            AnyView::new(
                TextField::new("", value)
                    .multiline()
                    .frame_height(height)
                    .accessibility_label(label.to_owned()),
            ),
        ],
    )
}

fn record_field<T: Clone + 'static>(
    records: Signal<Vec<T>>,
    index: usize,
    read: fn(&T) -> &str,
    write: fn(&mut T, String),
) -> Binding<TextBuffer> {
    Binding::new(
        move || {
            records.with_untracked(|items| {
                items
                    .get(index)
                    .map_or_else(TextBuffer::empty, |item| TextBuffer::new(read(item)))
            })
        },
        move |text: TextBuffer| {
            records.update(|items| {
                if let Some(item) = items.get_mut(index) {
                    write(item, text.string_value());
                }
            })
        },
    )
}

fn record_toggle<T: Clone + 'static>(
    records: Signal<Vec<T>>,
    index: usize,
    read: fn(&T) -> bool,
    write: fn(&mut T, bool),
) -> Binding<bool> {
    Binding::new(
        move || records.with_untracked(|items| items.get(index).is_some_and(read)),
        move |checked| {
            records.update(|items| {
                if let Some(item) = items.get_mut(index) {
                    write(item, checked);
                }
            })
        },
    )
}

fn record_picker<T: Clone + Default + 'static>(
    rows: &mut Vec<AnyView>,
    records: Signal<Vec<T>>,
    selected: Signal<usize>,
    add_title: &str,
    title: impl Fn(&T) -> String,
) -> Option<usize> {
    let items = records.get();
    let index = selected.get().min(items.len().saturating_sub(1));
    let add = AnyView::new(Button::title(add_title, move || {
        records.update(|items| {
            items.push(T::default());
            selected.set(items.len() - 1);
        });
    }));
    if items.is_empty() {
        rows.push(wrap_controls(vec![add]));
        return None;
    }
    let title = |item| {
        let text = title(item);
        if text.trim().is_empty() {
            "未命名".to_owned()
        } else {
            text
        }
    };
    let entries = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            MenuEntry::Item(
                MenuItem::new(i as u64 + 1, title(item), move || selected.set(i))
                    .selected(i == index),
            )
        })
        .collect();
    rows.push(wrap_controls(vec![
        chooser(title(&items[index]), entries, true),
        add,
        AnyView::new(Button::title("移除", move || {
            records.update(|items| {
                if index < items.len() {
                    items.remove(index);
                }
            })
        })),
    ]));
    Some(index)
}

fn assistant_message(text: &TextBuffer, markdown: bool) -> AnyView {
    let text = text.string_value();
    let message = if markdown {
        AnyView::new(MarkdownView::new(text))
    } else {
        AnyView::new(Text::new(text))
    };
    AnyView::new(
        message
            .padding_edges(10.0, 8.0)
            .semantic_background(SemanticColorRef::surface())
            .corner_radius(8.0),
    )
}

fn submit_refinement(ai: &crate::ai::AiState, index: usize, revision: Option<String>) {
    ai.feedback.set(String::new());
    ai.error.set(
        ai.refine(index, revision)
            .err()
            .map_or_else(String::new, |error| error.to_string()),
    );
}

fn status_title(status: &ModelStatus) -> &'static str {
    match status {
        ModelStatus::Indexing => "建索引",
        ModelStatus::Running(stage) => match stage {
            WritingStage::Research => "梳理前文",
            WritingStage::Draft => "起草中",
            WritingStage::Review => "审稿中",
            WritingStage::Revise => "修订中",
        },
        ModelStatus::Ready(_) => "完成",
        ModelStatus::NeedsDirection(_) => "待回答",
        ModelStatus::Failed(_) => "失败",
        ModelStatus::Cancelled => "已停止",
    }
}

fn wrap_controls(controls: Vec<AnyView>) -> AnyView {
    AnyView::new(BoxView::new(
        FlexDirection::Row,
        Align::Center,
        Justify::FlexStart,
        Wrap::Wrap,
        6.0,
        AnyView::new(TupleView::new(controls)),
    ))
}

fn model_picker(model: &Model, settings: &AiSettings, enabled: bool) -> AnyView {
    if settings.models.is_empty() {
        return caption("尚未保存模型");
    }
    let entries = settings
        .models
        .iter()
        .enumerate()
        .map(|(index, profile)| {
            let choose = model.clone();
            MenuEntry::Item(
                MenuItem::new(index as u64 + 1, profile_title(profile), move || {
                    choose.select_ai_profile(index);
                })
                .selected(settings.selected.as_slice() == [index]),
            )
        })
        .collect();
    let label = if let [index] = settings.selected.as_slice() {
        settings.models[*index].model.clone()
    } else {
        "切换到单个模型".to_owned()
    };
    chooser(label, entries, enabled)
}

fn parallel_picker(model: &Model, settings: &AiSettings, enabled: bool) -> AnyView {
    let entries = settings
        .models
        .iter()
        .enumerate()
        .map(|(index, profile)| {
            let choose = model.clone();
            let selected = settings.selected.contains(&index);
            MenuEntry::Item(
                MenuItem::new(index as u64 + 1, profile_title(profile), move || {
                    choose.toggle_ai_profile(index);
                })
                .selected(selected)
                .enabled(if selected {
                    settings.selected.len() > 1
                } else {
                    settings.selected.len() < MAX_PARALLEL_AI_MODELS
                }),
            )
        })
        .collect();
    let label = if settings.selected.len() > 1 {
        format!("并行模型（{}）", settings.selected.len())
    } else {
        "并行选模型".to_owned()
    };
    chooser(label, entries, enabled)
}

fn show_sources(rows: &mut Vec<AnyView>, result: &WritingResult, viewed: Signal<Option<usize>>) {
    let Some(coverage) = &result.coverage else {
        return;
    };
    if result.sources.is_empty() {
        rows.push(caption(format!("{coverage} 模型未通过章节索引回读原文。")));
    } else {
        let current = viewed.get();
        let mut entries: Vec<_> = result
            .sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                MenuEntry::Item(
                    MenuItem::new(
                        index as u64 + 1,
                        format!("[{}] {}", source.id, source.title),
                        move || {
                            viewed.set(Some(index));
                        },
                    )
                    .selected(current == Some(index)),
                )
            })
            .collect();
        if current.is_some() {
            entries.push(MenuEntry::Item(MenuItem::new(
                0,
                "收起原文",
                move || viewed.set(None),
            )));
        }
        rows.push(hstack(
            6.0,
            Align::Center,
            vec![
                caption(coverage),
                spacer(),
                AnyView::new(
                    MenuButton::new(
                        entries,
                        Text::new(format!("原文来源（{}）", result.sources.len())),
                    )
                    .composed()
                    .placement(PopoverPlacement::BottomTrailing),
                ),
            ],
        ));
        if let Some(source) = current.and_then(|index| result.sources.get(index)) {
            rows.push(caption(format!(
                "[{}] {} · {} · 第 {}–{} 行",
                source.id, source.title, source.file, source.first_line, source.last_line
            )));
            rows.push(text_box("检索原文", TextBuffer::new(&source.text), 103.0));
        }
    }
    if result.unknown_references {
        rows.push(caption("结果中含有未找到的来源编号，请核对相关结论。"));
    }
}

fn error_label(message: String) -> AnyView {
    AnyView::new(Text::new(message).semantic_foreground(SemanticColorRef::error()))
}

impl View for AiPanel {
    fn body(&self, cx: &Scope) -> AnyView {
        if !self.model.ai.open.get() {
            return AnyView::new(EmptyView);
        }
        let preview = cx.state(|| false);
        let controls = cx.state(|| false);
        let show_beats = cx.state(|| false);
        let scroll = cx.remember(|| RefCell::new(None::<(_, Option<ScrollRequest>)>));
        let turn = cx.remember(|| Cell::new(None));
        let brief_scroll = cx.remember(|| RefCell::new(None::<(_, ScrollRequest)>));
        let follow_tail = cx.remember(|| Cell::new(true));
        let last_geometry = cx.remember(|| Cell::new((0_f32, 0_f32)));
        let jump = cx.state(|| 0_u64);
        let settings = self.model.ai.settings_open.get();
        let brief = self.model.ai.brief_open.get();
        let (mut rows, action) = if settings {
            self.settings()
        } else if brief {
            self.brief()
        } else {
            self.writer(preview, controls, show_beats)
        };
        let error = self.model.ai.error.get();
        if !error.is_empty() && (settings || brief) {
            rows.insert(0, error_label(error.clone()));
        }
        let title = if settings {
            "AI 模型设置"
        } else if brief {
            "作品资料"
        } else {
            "写作对话"
        };
        let close = self.model.clone();
        let back = move || {
            if settings || brief {
                close.ai.settings_open.set(false);
                close.ai.brief_open.set(false);
                close.ai.error.set(String::new());
            } else {
                close.close_ai();
            }
        };
        let mut header = vec![
            AnyView::new(Text::new(title).semantic_font(SemanticFontRef::headline())),
            spacer(),
        ];
        if !settings && !brief && self.model.ai.target.get().is_some() {
            if self.model.ai_work().is_some() {
                let notes = self.model.clone();
                header.push(AnyView::new(Button::title("作品资料", move || {
                    notes.edit_ai_brief()
                })));
            }
            let import = self.model.clone();
            let export = self.model.clone();
            let busy = self.model.ai.batch().is_some_and(|batch| batch.busy());
            header.push(chooser(
                "对话记录",
                vec![
                    MenuEntry::Item(
                        MenuItem::new(1, "导入对话", move || import.import_ai_chat())
                            .enabled(!busy),
                    ),
                    MenuEntry::Item(MenuItem::new(2, "导出对话", move || {
                        export.export_ai_chat()
                    })),
                    MenuEntry::Item(MenuItem::new(3, "回到最新", move || {
                        jump.update(|value| *value += 1);
                    })),
                ],
                true,
            ));
        }
        header.push(AnyView::new(Button::title(
            if settings || brief {
                "返回写作"
            } else if self.docked {
                "收起"
            } else {
                "返回正文"
            },
            back,
        )));
        let heading = hstack(6.0, Align::Center, header).padding_insets(EdgeInsets::new(
            self.top_inset + 10.0,
            14.0,
            8.0,
            14.0,
        ));
        let mut body = ScrollView::new(
            ScrollAxis::Vertical,
            vstack(10.0, Align::Stretch, rows).padding_edges(14.0, 8.0),
        );
        if !settings && !brief {
            let story_stage = match self.model.ai.story_status() {
                Some(StoryStatus::Preparing { batch, .. }) => (1, batch),
                Some(StoryStatus::Ready(_)) => (2, 0),
                Some(StoryStatus::Failed(_)) => (3, 0),
                _ => (0, 0),
            };
            let batch = self.model.ai.batch();
            let last_turn = (
                self.model.document.get(),
                jump.get(),
                story_stage,
                batch
                    .as_ref()
                    .map(|batch| (batch.history.len(), batch.applied())),
            );
            let stream = batch.as_ref().and_then(|batch| {
                batch
                    .runs
                    .get(self.model.ai.viewed_result.get())
                    .map(|run| (run.streamed_text.utf8_length(), batch.busy()))
            });
            let key = (last_turn, controls.get(), stream);
            let follow = follow_tail.clone();
            let position = last_geometry.clone();
            body = body.on_geometry_change(move |geometry| {
                let end = (geometry.content_size.height - geometry.viewport_size.height).max(0.0);
                let (previous_end, previous_y) = position.replace((end, geometry.offset.y));
                if (end - previous_end).abs() < 1.0 && (geometry.offset.y - previous_y).abs() > 1.0
                {
                    follow.set(end - geometry.offset.y <= 30.0);
                }
            });
            let mut previous = scroll.borrow_mut();
            if previous.as_ref().is_none_or(|(last, _)| last != &key) {
                if turn.replace(Some(last_turn)) != Some(last_turn) {
                    follow_tail.set(true);
                }
                let request = follow_tail
                    .get()
                    .then(|| ScrollRequest::new(Point::new(0.0, 1.0e9), 0.0));
                *previous = Some((key, request));
            }
            if let Some((_, Some(request))) = &*previous {
                body = body.scroll_request(request.clone());
            }
        } else {
            *scroll.borrow_mut() = None;
            turn.set(None);
        }
        if brief {
            let page = self.model.ai.brief_page.get();
            let mut previous = brief_scroll.borrow_mut();
            if previous.as_ref().is_none_or(|(prior, _)| *prior != page) {
                *previous = Some((page, ScrollRequest::new(Point::new(0.0, 0.0), 0.0)));
            }
            if let Some((_, request)) = &*previous {
                body = body.scroll_request(request.clone());
            }
        } else {
            *brief_scroll.borrow_mut() = None;
        }
        let body = AnyView::new(body).flex_with(1.0, 1.0, Some(0.0));
        let footer = if settings || brief {
            hstack(6.0, Align::Center, vec![spacer(), action])
        } else {
            let mut composer = Vec::new();
            let archive_error = self.model.ai.archive_read_error.get();
            if !archive_error.is_empty() {
                composer.push(error_label(archive_error));
            }
            let feedback = self.model.ai.feedback.get();
            if !error.is_empty() {
                composer.push(error_label(error));
            } else if !feedback.is_empty() {
                composer.push(caption(feedback));
            }
            composer.push(action);
            vstack(5.0, Align::Stretch, composer)
        }
        .padding_insets(EdgeInsets::new(8.0, 14.0, 10.0 + self.bottom_inset, 14.0));
        let mut panel = vec![AnyView::new(heading)];
        if brief {
            panel.push(self.brief_tabs());
        }
        panel.extend([AnyView::new(body), AnyView::new(footer)]);
        AnyView::new(vstack(0.0, Align::Stretch, panel).fill())
    }
}
