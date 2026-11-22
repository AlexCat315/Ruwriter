use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cove_agent::AgentError;
use cove_ui::compose::modifiers::interaction::AccessibilityExt;
use cove_ui::compose::modifiers::layout::{EdgeInsets, FlexExt, LayoutFrameExt, PaddingExt};
use cove_ui::compose::modifiers::style::{AppearanceExt, SemanticStyleViewExt};
use cove_ui::compose::primitives::controls::{
    Button, ButtonIcon, ButtonIconSource, MenuButton, Toggle,
};
use cove_ui::compose::primitives::media::ui_icon_catalog as icons;
use cove_ui::compose::primitives::overlays::{MenuEntry, MenuItem, MenuSubmenu, PopoverPlacement};
use cove_ui::compose::primitives::text::text_field::{
    TextFieldBehavior, TextFieldEvents, TextFieldNavigationRequest,
};
use cove_ui::compose::primitives::text::{
    Divider, MarkdownView, Text, TextBuffer, TextField, TextRun, TextTraits,
};
use cove_ui::compose::theme::styles::buttons::{
    ButtonStyleViewExt, GhostButtonStyle, PrimaryButtonStyle, ToggleButtonStyle,
};
use cove_ui::compose::theme::styles::components::text_field_style::TextFieldStyleViewExt;
use cove_ui::compose::theme::{SemanticColorRef, SemanticFontRef};
use cove_ui::compose::{ScrollAxis, ScrollRequest, ScrollView};
use cove_ui::composition::{AnyView, BoxView, EmptyView, Scancode, Scope, TupleView, View};
use cove_ui::core::scene::{Align, ClipboardHolder, FlexDirection, Justify, Wrap};
use cove_ui::core::{Binding, Point, Signal};
use ruwriter_core::{AiPreferences, AiSettings, MAX_PARALLEL_AI_MODELS};

use super::ui::{ProseFieldStyle, ToolButtonStyle, hstack, icon_button, spacer, vstack};
use crate::ai::{
    Batch, Catalog, Connection, ModelStatus, StoryStatus, WritingResult, WritingStage, WritingTask,
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

fn field(
    label: &str,
    placeholder: &str,
    binding: Binding<TextBuffer>,
    secret: bool,
    limit: usize,
) -> AnyView {
    vstack(
        5.0,
        Align::Stretch,
        vec![
            caption(label),
            AnyView::new(
                TextField::new(placeholder, binding)
                    .behavior(TextFieldBehavior {
                        secure: secret,
                        max_length: Some(limit),
                        ..Default::default()
                    })
                    .accessibility_label(label.to_owned()),
            ),
        ],
    )
}

fn preset(label: &str, url: &str, model: &Model) -> AnyView {
    let ai = model.ai.clone();
    let endpoint = TextBuffer::new(url);
    let current = ai.endpoint.get().string_value();
    let selected = cove_agent::openai_compatible::api_base(&current).ok() == Some(url);
    AnyView::new(
        Button::title(label, move || ai.change_endpoint(endpoint.clone()))
            .selected(selected)
            .button_style(ToolButtonStyle),
    )
}

fn chooser(label: impl Into<String>, entries: Vec<MenuEntry>, enabled: bool) -> AnyView {
    AnyView::new(
        MenuButton::new(entries, Text::new(label.into()).truncated())
            .composed()
            .enabled(enabled),
    )
}

fn glyph_menu(label: &str, entries: Vec<MenuEntry>) -> AnyView {
    AnyView::new(
        MenuButton::new(
            entries,
            ButtonIcon::new(
                ButtonIconSource::resource(icons::action::more_horizontal()),
                Some(16.0),
                None,
            ),
        )
        .ghost()
        .composed()
        .accessibility_label(label)
        .tooltip(label),
    )
}

fn composer_menu(text: &str, label: String, entries: Vec<MenuEntry>, enabled: bool) -> AnyView {
    AnyView::new(
        MenuButton::new(
            entries,
            hstack(
                4.0,
                Align::Center,
                vec![
                    AnyView::new(
                        Text::new(text)
                            .truncated()
                            .semantic_font(SemanticFontRef::caption()),
                    ),
                    AnyView::new(ButtonIcon::new(
                        ButtonIconSource::resource(icons::navigation::chevron_down()),
                        Some(10.0),
                        None,
                    )),
                ],
            ),
        )
        .ghost()
        .composed()
        .enabled(enabled)
        .accessibility_label(label.clone())
        .tooltip(label),
    )
}

fn send_button(label: &str, enabled: bool, stopping: bool, action: impl Fn() + 'static) -> AnyView {
    let icon = if stopping {
        icons::shape::square()
    } else {
        icons::navigation::arrow_up()
    };
    AnyView::new(
        Button::icon(ButtonIconSource::resource(icon), 17.0, action)
            .enabled(enabled)
            .accessibility_text(label)
            .tooltip(label)
            .button_style(PrimaryButtonStyle),
    )
}

fn muted_line(text: String) -> AnyView {
    AnyView::new(
        Text::new(text)
            .truncated()
            .semantic_font(SemanticFontRef::caption())
            .semantic_foreground(SemanticColorRef::on_surface_muted()),
    )
}

fn profile_title(profile: &AiPreferences) -> String {
    format!("{} · {}", profile.model, profile.endpoint)
}

impl AiPanel {
    fn conversation_branches(&self) -> (Vec<AnyView>, AnyView) {
        let chat = self.model.ai.displayed_chat();
        let alternatives = self.model.ai.branches.get();
        let mut rows = Vec::new();
        if !alternatives.is_empty() {
            rows.push(caption("其他分支"));
            for branch in alternatives {
                let model = self.model.clone();
                rows.push(branch_row(branch.title, "切换到此分支", move || {
                    model.move_ai_conversation(Some(&branch.id));
                }));
            }
        }
        if chat.len() > 1 {
            rows.push(caption("当前对话"));
            for turn in chat.iter().rev().skip(1) {
                let model = self.model.clone();
                let id = turn.id.clone();
                rows.push(branch_row(
                    crate::ai_chat::turn_title(turn),
                    "回退到此轮",
                    move || {
                        model.move_ai_conversation(Some(&id));
                    },
                ));
            }
        } else if chat.is_empty() && rows.is_empty() {
            rows.push(caption("还没有可回退的对话。"));
        }
        let model = self.model.clone();
        (
            rows,
            AnyView::new(
                Button::title("回到对话开头", move || {
                    model.move_ai_conversation(None);
                })
                .enabled(!chat.is_empty()),
            ),
        )
    }

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
            AnyView::new(
                Button::title("添加模型", move || add.edit_ai_profile(None))
                    .button_style(GhostButtonStyle),
            ),
        ];
        if let Some(index) = editing {
            header.push(glyph_menu(
                "模型操作",
                vec![MenuEntry::Item(MenuItem::new(
                    1,
                    "移除模型",
                    move || remove.remove_ai_profile(index),
                ))],
            ));
        }
        let endpoint = ai.endpoint;
        let endpoint_editor = ai.clone();
        let key = ai.api_key;
        let key_editor = ai.clone();
        let address = endpoint.get().string_value();
        let mut rows = vec![
            caption("已保存模型"),
            wrap_controls(header),
            AnyView::new(Divider::horizontal()),
            caption("服务"),
            wrap_controls(vec![
                preset("DeepSeek", "https://api.deepseek.com", &self.model),
                preset("Ollama", "http://127.0.0.1:11434/v1", &self.model),
                preset("LM Studio", "http://127.0.0.1:1234/v1", &self.model),
            ]),
            field(
                "API 地址",
                "https://…",
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
                "",
                Binding::new(move || key.get(), move |value| key_editor.change_key(value)),
                true,
                4096,
            ),
            caption("同一服务共用会话密钥。"),
            AnyView::new(Divider::horizontal()),
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
            .enabled(!loading && !address.is_empty())
            .button_style(GhostButtonStyle),
        )];
        let catalog_notice = match catalog {
            Catalog::Loaded(models) if !models.is_empty() => {
                let entries = models
                    .iter()
                    .enumerate()
                    .map(|(index, id)| {
                        let name = TextBuffer::new(id);
                        let edit = ai.clone();
                        MenuEntry::Item(MenuItem::new(index as u64 + 1, id.clone(), move || {
                            edit.change_model(name.clone())
                        }))
                    })
                    .collect();
                catalog_controls.push(chooser("从列表选择", entries, true));
                None
            }
            Catalog::Loaded(_) => Some(caption("服务未返回模型，可以手动填写。")),
            Catalog::Failed(AgentError::HttpStatus(401 | 403)) => {
                Some(error_label("请检查此服务的 API 密钥。".to_owned()))
            }
            Catalog::Failed(error) => Some(caption(format!(
                "无法获取模型列表，可手动填写模型名称。{error}"
            ))),
            _ => None,
        };
        let model_name = ai.model;
        let edit_model = ai.clone();
        rows.push(field(
            "模型名称",
            "",
            Binding::new(
                move || model_name.get(),
                move |value| edit_model.change_model(value),
            ),
            false,
            512,
        ));
        rows.push(wrap_controls(catalog_controls));
        rows.extend(catalog_notice);
        rows.push(AnyView::new(Toggle::labeled(
            "允许模型查阅文稿",
            ai.tools.binding(),
        )));
        let connection = ai.connection.get();
        let test = ai.clone();
        let testing = matches!(connection, Connection::Testing(_));
        let valid = !address.trim().is_empty() && !ai.model.get().string_value().trim().is_empty();
        let mut validation = vec![AnyView::new(
            Button::title(
                if testing {
                    "正在测试连接…"
                } else {
                    "测试连接"
                },
                move || test.test_connection(),
            )
            .enabled(valid && !testing),
        )];
        match connection {
            Connection::Connected => validation.push(caption("模型已成功响应。")),
            Connection::Failed(AgentError::HttpStatus(401 | 403)) => {
                validation.push(error_label("服务拒绝了 API 密钥。".to_owned()))
            }
            Connection::Failed(error) => validation.push(error_label(format!("连接失败。{error}"))),
            _ => {}
        }
        rows.push(wrap_controls(validation));
        let model = self.model.clone();
        let activate = self.model.clone();
        let action = wrap_controls(vec![
            AnyView::new(
                Button::title("保存模型", move || model.save_ai_preferences(false))
                    .enabled(valid)
                    .button_style(GhostButtonStyle),
            ),
            primary("保存并使用", valid, move || {
                activate.save_ai_preferences(true)
            }),
        ]);
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
        composer_focus: Signal<bool>,
        composer_navigation: Signal<u64>,
        review_expanded: Signal<bool>,
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
        let archived_pending = batch
            .is_none()
            .then(|| crate::ai_scene::pending(&history))
            .flatten()
            .map(|(turn, _)| turn.id.as_str());
        for turn in history.iter() {
            let mut message = vec![self.chat_author(
                &turn.author,
                &turn.id,
                composer_navigation,
                !busy && !preparing_story,
            )];
            if let Some(answer) = &turn.answer {
                let copy = answer.clone();
                message.push(assistant_message(answer, turn.markdown));
                let mut actions = Vec::new();
                if archived_pending == Some(turn.id.as_str()) {
                    actions.push(caption("待回答"));
                } else if turn.adopted {
                    actions.push(caption("已采纳"));
                }
                actions.push(spacer());
                actions.push(icon_button(
                    icons::action::copy(),
                    "复制",
                    false,
                    move || {
                        ClipboardHolder::write(&copy.string_value());
                    },
                ));
                message.push(hstack(4.0, Align::Center, actions));
                if archived_pending == Some(turn.id.as_str()) {
                    let model = self.model.clone();
                    message.push(wrap_controls(vec![AnyView::new(Button::title(
                        "回到落笔点并回答",
                        move || {
                            model.resume_ai_scene();
                        },
                    ))]));
                }
            }
            rows.push(vstack(8.0, Align::Stretch, message));
        }
        if let Some(batch) = &batch {
            if let Some(error) = &batch.archive_error {
                rows.push(error_label(format!("无法保存对话。{error}")));
            }
            rows.push(self.chat_author(
                &batch.prompt,
                batch.turn_id(),
                composer_navigation,
                !busy && !preparing_story,
            ));
            self.results(&mut rows, batch, show_beats, review_expanded);
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
                    rows.push(caption(if continuing && scope == ResearchScope::Project {
                        "续写检索光标前的原文，以及其他文稿中编号更早的章节。"
                    } else {
                        "在本机检索。所选模型会收到相关原文，支持工具的模型可以继续查阅。"
                    }));
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
            if has_context && (include || polishing) {
                rows.push(AnyView::new(Button::title(
                    if preview_context.get() {
                        "收起原文"
                    } else {
                        "查看参考原文"
                    },
                    move || preview_context.update(|shown| *shown = !*shown),
                )));
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
            send_button("停止梳理", true, true, move || ai.clear_story())
        } else if busy {
            let ai = ai.clone();
            send_button("停止", true, true, move || ai.stop())
        } else if preferences.models.is_empty() {
            let model = self.model.clone();
            primary("添加模型", true, move || model.manage_ai())
        } else {
            let submit = send.clone();
            if needs_story && ready {
                primary("先梳理作品", true, move || submit())
            } else {
                send_button(
                    if answering {
                        "回答并继续"
                    } else {
                        "发送"
                    },
                    ready,
                    false,
                    move || submit(),
                )
            }
        };
        let mut composer = Vec::new();
        if continuing && !revising {
            composer.push(muted_line(
                target
                    .as_ref()
                    .and_then(|target| target.continuation_anchor())
                    .map_or_else(
                        || "将光标放到续写处".to_owned(),
                        |anchor| format!("接在「{anchor}」后"),
                    ),
            ));
        } else if polishing {
            let selected = target.as_ref().and_then(|target| target.selection.as_ref());
            composer.push(muted_line(selected.map_or_else(
                || "在正文中框选要润色的片段".to_owned(),
                |range| format!("润色正文选区 · {} 字", range.len()),
            )));
        } else if mode == WritingTask::Request && has_context {
            let attachment = ai.include_context;
            let label = match (include, selected) {
                (true, true) => "已附选区",
                (true, false) => "已附前文",
                (false, true) => "附带选区",
                (false, false) => "附带前文",
            };
            composer.push(hstack(
                0.0,
                Align::Center,
                vec![
                    AnyView::new(
                        Button::title(label, move || attachment.update(|value| *value = !*value))
                            .selected(include)
                            .tooltip(if include {
                                "移除这段正文"
                            } else {
                                "把这段正文发给模型"
                            })
                            .enabled(!busy)
                            .button_style(GhostButtonStyle),
                    ),
                    spacer(),
                ],
            ));
        }
        let input_label = match (answering, revising, mode) {
            (true, _, _) => "补充这一场的方向…",
            (_, true, _) => "接着说你想怎么改…",
            (_, _, WritingTask::Continue) => "这一段接下来发生什么？",
            (_, _, WritingTask::Polish) => "这段正文要怎么改？",
            (_, _, WritingTask::Request) => "问一个关于作品的问题…",
        };
        let lines = prompt
            .lines()
            .take(5)
            .map(|line| line.chars().take(140).count().div_ceil(26).max(1))
            .sum::<usize>()
            .clamp(1, 5);
        let mut input = TextField::new(input_label, ai.prompt.binding())
            .multiline()
            .events(TextFieldEvents {
                on_focus: Some(Rc::new(move || composer_focus.set(true))),
                on_blur: Some(Rc::new(move || composer_focus.set(false))),
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
            });
        let focus = composer_navigation.get();
        if focus > 0 {
            input.navigation = TextFieldNavigationRequest {
                focus_request_id: Some(focus),
                ..Default::default()
            };
        }
        let input = AnyView::new(
            input
                .text_field_style(ProseFieldStyle)
                .frame_height(49.0 + (lines - 1) as f32 * 18.0),
        );
        let references = if show_controls.get() {
            "收起资料"
        } else {
            "参考资料"
        };
        let references = AnyView::new(
            Button::icon(
                ButtonIconSource::resource(icons::shape::paperclip()),
                15.0,
                move || show_controls.update(|shown| *shown = !*shown),
            )
            .enabled(!busy)
            .selected(show_controls.get())
            .tooltip(references)
            .accessibility_text(references)
            .button_style(ToolButtonStyle),
        );
        let draft_model = if revising {
            batch
                .as_ref()
                .and_then(|batch| batch.runs.get(index))
                .map(|run| run.profile.model.as_str())
        } else {
            None
        };
        let model_control = if let Some(model) = draft_model {
            let manage = self.model.clone();
            let name: String = model.chars().take(19).collect();
            composer_menu(
                &name,
                format!("修订模型 · {model}"),
                vec![MenuEntry::Item(MenuItem::new(
                    1,
                    "模型设置",
                    move || manage.manage_ai(),
                ))],
                !busy,
            )
        } else {
            model_picker(&self.model, &preferences, !busy)
        };
        let compact_mode = match mode_label {
            "修改当前草稿" => "改稿",
            "补充场景方向" => "补充方向",
            "先确认本场" => "先确认",
            _ => mode_label,
        };
        let tools = hstack(
            2.0,
            Align::Center,
            vec![
                references,
                composer_menu(compact_mode, mode_label.to_owned(), modes, !busy),
                AnyView::new(model_control.flex_with(0.0, 1.0, None).frame_min_width(0.0)),
                spacer(),
                action,
            ],
        );
        let border = if composer_focus.get() {
            SemanticColorRef::accent().opacity(0.62)
        } else {
            SemanticColorRef::border()
        };
        composer.push(AnyView::new(
            vstack(0.0, Align::Stretch, vec![input, tools])
                .padding_edges(6.0, 5.0)
                .semantic_background(SemanticColorRef::surface_raised())
                .corner_radius(14.0)
                .semantic_border(border, 1.0),
        ));
        (rows, vstack(7.0, Align::Stretch, composer))
    }

    fn chat_author(
        &self,
        author: &TextBuffer,
        id: &str,
        composer_navigation: Signal<u64>,
        enabled: bool,
    ) -> AnyView {
        let model = self.model.clone();
        let id = id.to_owned();
        let edit = enabled.then(|| {
            icon_button(
                icons::action::pencil(),
                "编辑并重发",
                false,
                move || match model.ai.edit_conversation_turn(&id) {
                    Ok(()) => {
                        model.ai.error.set(String::new());
                        composer_navigation.update(|value| *value += 1);
                    }
                    Err(error) => model.ai.error.set(error),
                },
            )
        });
        author_message(author, edit)
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

    fn results(
        &self,
        rows: &mut Vec<AnyView>,
        batch: &Batch,
        show_beats: Signal<bool>,
        review_expanded: Signal<bool>,
    ) {
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
                if !batch.same_scene(ai.target.get().as_ref()) {
                    let model = self.model.clone();
                    rows.push(wrap_controls(vec![AnyView::new(
                        Button::title("回到落笔点并回答", move || {
                            model.resume_ai_scene();
                        })
                        .enabled(!batch.busy()),
                    )]));
                }
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
                let editing = ai.edit_candidate.get();
                let modified = result.text != result.original_text;
                let binding = ai.draft_binding(batch, current);
                let mut controls = Vec::new();
                if batch.adopted == Some(current) {
                    controls.push(caption("已采纳"));
                } else if result.task != WritingTask::Request {
                    controls.push(caption("草稿"));
                }
                controls.push(spacer());
                if !batch.applied() && result.task != WritingTask::Request && !batch.busy() {
                    let edit = ai.edit_candidate;
                    controls.push(AnyView::new(
                        Button::title(
                            if editing {
                                "完成编辑"
                            } else {
                                "编辑草稿"
                            },
                            move || edit.update(|active| *active = !*active),
                        )
                        .button_style(GhostButtonStyle),
                    ));
                }
                controls.push(icon_button(
                    icons::action::copy(),
                    "复制草稿",
                    false,
                    move || {
                        ClipboardHolder::write(&copied.string_value());
                        feedback.set("已复制草稿。".to_owned());
                    },
                ));
                let mut more = Vec::new();
                if !batch.applied() && modified {
                    let original = result.original_text.clone();
                    let restore = binding.clone();
                    more.push(MenuEntry::Item(MenuItem::new(
                        1,
                        "恢复生成稿",
                        move || {
                            restore.set(original.clone());
                        },
                    )));
                }
                if run.previous.is_some() {
                    let previous = ai.show_previous;
                    more.push(MenuEntry::Item(MenuItem::new(
                        2,
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
                    more.push(MenuEntry::Item(MenuItem::new(
                        3,
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
                    more.push(MenuEntry::Item(MenuItem::new(
                        4,
                        "用正文选区作为改写版",
                        move || {
                            model.remember_ai_revision(current, true);
                        },
                    )));
                }
                if !more.is_empty() {
                    controls.push(glyph_menu("草稿操作", more));
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
                rows.push(hstack(2.0, Align::Center, controls));
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
                self.review_candidate(
                    rows,
                    result,
                    current,
                    !batch.applied() && !batch.busy(),
                    review_expanded,
                );
                if has_target && !batch.applied() {
                    let ready = result.can_adopt();
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
        expanded: Signal<bool>,
    ) {
        if result.task == WritingTask::Request {
            return;
        }
        let changed = !result.text.has_same_contents(&result.reviewed_text);
        let issues = match &result.review {
            Review::Checked(issues) if !changed => issues.as_slice(),
            _ => &[],
        };
        let count = if changed {
            0
        } else {
            issues.len()
                + result.continuity_notes.len()
                + usize::from(result.repetition_warning.is_some())
        };
        let title = if changed {
            "草稿已编辑，待重新审稿".to_owned()
        } else {
            match &result.review {
                Review::Rejected => "回复尚未交付正文".to_owned(),
                Review::Incomplete => "本轮审稿未完成".to_owned(),
                Review::NotRequested => "待审稿".to_owned(),
                Review::Checked(_) if count == 0 => "审稿未发现具体问题".to_owned(),
                Review::Checked(_) => format!("{count} 项待核对"),
            }
        };
        let mut controls = vec![AnyView::new(
            Text::new(title).truncated().flex_with(1.0, 1.0, None),
        )];
        let show_details = expanded.get();
        if count > 0 {
            controls.push(AnyView::new(
                Button::title(
                    if show_details {
                        "收起意见"
                    } else {
                        "展开意见"
                    },
                    move || expanded.set(!show_details),
                )
                .button_style(GhostButtonStyle),
            ));
        }
        let mut details = vec![hstack(3.0, Align::Center, controls)];
        if !changed && result.review.has_character_violation() {
            details.push(error_label(
                "草稿偏离了角色卡或关系边界，修正后可采纳。".to_owned(),
            ));
        }
        if !changed && result.review.has_disclosure() {
            details.push(error_label("草稿提前揭晓了信息，修正后可采纳。".to_owned()));
        }
        if result.needs_private_review() {
            details.push(caption("采纳前需核对角色卡和知情边界。"));
        }
        if count > 0 && show_details {
            for note in result.continuity_notes.iter() {
                details.push(caption(format!("接续待核对 · {note}")));
            }
            if let Some(warning) = &result.repetition_warning {
                details.push(error_label(warning.clone()));
            }
            for issue in issues {
                let mut quote = vec![
                    caption(issue.kind.label()),
                    AnyView::new(Text::new(format!("「{}」", issue.quote.trim()))),
                    caption(issue.reason.trim()),
                ];
                if let Some(basis) = &issue.basis {
                    quote.push(caption(format!("依据 · 「{basis}」")));
                }
                if let Some(source) = &issue.source {
                    quote.push(caption(format!("原文来源 · [{source}]")));
                }
                details.push(vstack(3.0, Align::Stretch, quote));
            }
        }
        if available {
            let review = self.model.ai.clone();
            let mut actions = vec![AnyView::new(
                Button::title("重新审稿", move || {
                    submit_refinement(&review, index, None)
                })
                .button_style(GhostButtonStyle),
            )];
            if !changed {
                let instructions = result
                    .repetition_warning
                    .iter()
                    .cloned()
                    .chain(result.review_revision_instruction())
                    .collect::<Vec<_>>();
                if !instructions.is_empty() {
                    let revise = self.model.ai.clone();
                    let instructions = instructions.join("\n");
                    actions.push(AnyView::new(Button::title(
                        "按审稿意见修订",
                        move || {
                            submit_refinement(&revise, index, Some(instructions.clone()));
                        },
                    )));
                }
            }
            details.push(wrap_controls(actions));
        }
        rows.push(AnyView::new(
            vstack(5.0, Align::Stretch, details)
                .padding_edges(10.0, 8.0)
                .semantic_background(SemanticColorRef::surface_raised())
                .corner_radius(9.0),
        ));
    }
}

fn author_message(text: &TextBuffer, edit: Option<AnyView>) -> AnyView {
    let message = text.string_value();
    let label = format!("你说，{message}");
    let bubble = Text::new(message)
        .accessibility_label(label)
        .padding_edges(11.0, 7.0)
        .semantic_background(SemanticColorRef::accent_muted())
        .corner_radius(12.0)
        .flex_with(0.0, 1.0, None);
    let mut row = vec![spacer()];
    row.extend(edit);
    row.push(AnyView::new(bubble));
    hstack(5.0, Align::Center, row)
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
    if markdown {
        AnyView::new(MarkdownView::new(text))
    } else {
        AnyView::new(Text::new(text))
    }
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
        return caption("未选择模型");
    }
    let mut entries: Vec<_> = settings
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
    if settings.models.len() > 1 {
        entries.push(MenuEntry::Submenu(
            MenuSubmenu::new(u64::MAX - 1, "并行比较", parallel_choices(model, settings))
                .width(280.0),
        ));
    }
    let manage = model.clone();
    entries.push(MenuEntry::Item(MenuItem::new(
        u64::MAX,
        "模型设置",
        move || manage.manage_ai(),
    )));
    let (text, label) = if let [index] = settings.selected.as_slice() {
        let name = &settings.models[*index].model;
        let short: String = name.chars().take(19).collect();
        (short, format!("模型 · {name}"))
    } else {
        let count = settings.selected.len();
        (format!("{count} 个模型"), format!("并行模型 · {count}"))
    };
    composer_menu(&text, label, entries, enabled)
}

fn parallel_choices(model: &Model, settings: &AiSettings) -> Vec<MenuEntry> {
    settings
        .models
        .iter()
        .enumerate()
        .map(|(index, profile)| {
            let choose = model.clone();
            let selected = settings.selected.contains(&index);
            let action = if selected {
                "移出比较"
            } else {
                "加入比较"
            };
            MenuEntry::Item(
                MenuItem::new(
                    index as u64 + 1,
                    format!("{action} · {}", profile_title(profile)),
                    move || {
                        choose.toggle_ai_profile(index);
                    },
                )
                .selected(selected)
                .enabled(if selected {
                    settings.selected.len() > 1
                } else {
                    settings.selected.len() < MAX_PARALLEL_AI_MODELS
                }),
            )
        })
        .collect()
}

fn branch_row(title: String, label: &str, action: impl Fn() + 'static) -> AnyView {
    hstack(
        8.0,
        Align::Center,
        vec![
            AnyView::new(Text::new(title).truncated().flex_with(1.0, 1.0, Some(0.0))),
            AnyView::new(Button::title(label, action)),
        ],
    )
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
        let composer_focus = cx.state(|| false);
        let composer_navigation = cx.state(|| 0_u64);
        let review_expanded = cx.state(|| false);
        let scroll = cx.remember(|| Rc::new(RefCell::new(None::<(_, Option<ScrollRequest>)>)));
        let turn = cx.remember(|| Rc::new(Cell::new(None)));
        let brief_scroll = cx.remember(|| Rc::new(RefCell::new(None::<(_, ScrollRequest)>)));
        let follow_tail = cx.remember(|| Rc::new(Cell::new(true)));
        let detached = cx.state(|| false);
        let last_geometry = cx.remember(|| Rc::new(Cell::new(None::<(f32, f32)>)));
        let jump = cx.state(|| 0_u64);
        let settings = self.model.ai.settings_open.get();
        let brief = self.model.ai.brief_open.get();
        let branches = self.model.ai.branches_open.get();
        let secondary = settings || brief || branches;
        let (mut rows, action) = if settings {
            self.settings()
        } else if brief {
            self.brief()
        } else if branches {
            self.conversation_branches()
        } else {
            self.writer(
                preview,
                controls,
                show_beats,
                composer_focus,
                composer_navigation,
                review_expanded,
            )
        };
        let error = self.model.ai.error.get();
        if !error.is_empty() && secondary {
            rows.insert(0, error_label(error.clone()));
        }
        let title = if settings {
            "AI 模型设置"
        } else if brief {
            "作品资料"
        } else if branches {
            "回退与分支"
        } else {
            "写作对话"
        };
        let close = self.model.clone();
        let back = move || {
            if secondary {
                close.ai.settings_open.set(false);
                close.ai.brief_open.set(false);
                close.ai.branches_open.set(false);
                close.ai.error.set(String::new());
            } else {
                close.close_ai();
            }
        };
        let mut header = Vec::new();
        if secondary {
            header.push(icon_button(
                icons::navigation::arrow_left(),
                "返回写作",
                false,
                back.clone(),
            ));
        }
        header.push(AnyView::new(
            Text::new(title)
                .truncated()
                .semantic_font(SemanticFontRef::headline()),
        ));
        header.push(spacer());
        if !secondary && self.model.ai.target.get().is_some() {
            if self.model.ai_work().is_some() {
                let notes = self.model.clone();
                header.push(icon_button(
                    icons::shape::book_open(),
                    "作品资料",
                    false,
                    move || notes.edit_ai_brief(),
                ));
            }
            let import = self.model.clone();
            let export = self.model.clone();
            let rollback = self.model.clone();
            let busy = self.model.ai.batch().is_some_and(|batch| batch.busy());
            header.push(glyph_menu(
                "对话记录",
                vec![
                    MenuEntry::Item(
                        MenuItem::new(4, "回退与分支", move || rollback.show_ai_branches())
                            .enabled(!busy),
                    ),
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
            ));
        }
        if !secondary {
            let (glyph, label) = if self.docked {
                (icons::layout::panel_right(), "收起")
            } else {
                (icons::navigation::arrow_left(), "返回正文")
            };
            header.push(icon_button(glyph, label, false, back));
        }
        let heading = hstack(2.0, Align::Center, header).padding_insets(EdgeInsets::new(
            self.top_inset + 3.0,
            14.0,
            5.0,
            14.0,
        ));
        let mut body = ScrollView::new(
            ScrollAxis::Vertical,
            vstack(if secondary { 10.0 } else { 16.0 }, Align::Stretch, rows)
                .padding_edges(16.0, 12.0),
        );
        if !secondary {
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
                composer_navigation.get(),
                batch.as_ref().map_or_else(
                    || self.model.ai.archived_chat().len(),
                    |batch| batch.history.len() + 1,
                ),
            );
            let stream = batch.as_ref().and_then(|batch| {
                batch
                    .runs
                    .get(self.model.ai.viewed_result.get())
                    .map(|run| (run.streamed_text.utf8_length(), batch.busy()))
            });
            let key = (
                last_turn,
                story_stage,
                controls.get(),
                stream,
                batch.as_ref().is_some_and(Batch::applied),
            );
            let follow = follow_tail.clone();
            let position = last_geometry.clone();
            body = body.on_geometry_change(move |geometry| {
                let end = (geometry.content_size.height - geometry.viewport_size.height).max(0.0);
                let y = geometry.offset.y;
                let previous = position.replace(Some((end, y)));
                let near_tail = end - y <= 28.0;
                if near_tail
                    || previous.is_some_and(|(old_end, old_y)| {
                        let moved_up = y < old_y - 1.0 && end >= old_end - 1.0;
                        moved_up || ((end - old_end).abs() < 1.0 && (y - old_y).abs() > 1.0)
                    })
                {
                    follow.set(near_tail);
                    detached.set_if_changed(!near_tail);
                }
            });
            let mut previous = scroll.borrow_mut();
            if previous.as_ref().is_none_or(|(last, _)| last != &key) {
                if turn.replace(Some(last_turn)) != Some(last_turn) {
                    follow_tail.set(true);
                    detached.set_if_changed(false);
                }
                let request = follow_tail
                    .get()
                    .then(|| ScrollRequest::new(Point::new(0.0, 1.0e9), 0.0));
                *previous = Some((key, request));
            }
            if follow_tail.get()
                && let Some((_, Some(request))) = &*previous
            {
                body = body.scroll_request(request.clone());
            }
        } else {
            *scroll.borrow_mut() = None;
            turn.set(None);
            last_geometry.set(None);
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
        let footer = if secondary {
            hstack(6.0, Align::Center, vec![spacer(), action])
        } else {
            let mut composer = Vec::new();
            if detached.get() {
                composer.push(hstack(
                    0.0,
                    Align::Center,
                    vec![
                        spacer(),
                        AnyView::new(
                            Button::title("回到最新", move || jump.update(|value| *value += 1))
                                .button_style(GhostButtonStyle),
                        ),
                    ],
                ));
            }
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
        .padding_insets(EdgeInsets::new(6.0, 14.0, 12.0 + self.bottom_inset, 14.0));
        let mut panel = vec![AnyView::new(heading), AnyView::new(Divider::horizontal())];
        if brief {
            panel.push(self.brief_tabs());
        }
        panel.extend([AnyView::new(body), AnyView::new(footer)]);
        AnyView::new(vstack(0.0, Align::Stretch, panel).fill())
    }
}
