use std::{cell::RefCell, rc::Rc};

use cove_agent::AgentError;
use cove_ui::compose::modifiers::layout::{EdgeInsets, FlexExt, LayoutFrameExt, PaddingExt};
use cove_ui::compose::modifiers::style::{AppearanceExt, SemanticStyleViewExt};
use cove_ui::compose::primitives::controls::{Button, MenuButton, Toggle};
use cove_ui::compose::primitives::overlays::{MenuEntry, MenuItem, PopoverPlacement};
use cove_ui::compose::primitives::text::text_field::{TextFieldBehavior, TextFieldEvents};
use cove_ui::compose::primitives::text::{Text, TextBuffer, TextField};
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
    is_continuation_command, requires_key,
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

    fn brief(&self) -> (Vec<AnyView>, AnyView) {
        let ai = &self.model.ai;
        let notes = |label: &str, signal: Signal<TextBuffer>, height: f32| {
            AnyView::new(
                TextField::new(label, signal.binding())
                    .multiline()
                    .frame_height(height),
            )
        };
        let mut rows = vec![];
        if let Some((_, name)) = self.model.ai_work() {
            rows.push(caption(name));
        }
        rows.push(caption(
            "启用手记后，会把这些内容连同本次任务发给所选模型。",
        ));
        rows.push(caption("剧情大纲"));
        rows.push(notes("已发生的剧情和作者规划", ai.story_outline, 120.0));
        rows.push(caption("人物小传与关系"));
        rows.push(notes(
            "人物身份、关系和当前状态",
            ai.story_characters,
            105.0,
        ));
        rows.push(caption("待续线索"));
        rows.push(notes("还没写完的线索", ai.story_threads, 85.0));
        rows.push(caption("硬设定和既定事实"));
        rows.push(notes("硬设定和既定事实", ai.canon, 105.0));
        rows.push(caption("文风要求"));
        rows.push(notes("文风要求", ai.voice, 85.0));
        for (index, sample) in ai.voice_samples.get().iter().enumerate() {
            let remove = ai.voice_samples;
            let excerpt: String = sample.chars().take(90).collect();
            rows.push(wrap_controls(vec![
                caption(excerpt),
                AnyView::new(Button::title("移除此样段", move || {
                    remove.update(|samples| {
                        if index < samples.len() {
                            samples.remove(index);
                        }
                    });
                })),
            ]));
        }
        if ai
            .target
            .get()
            .is_some_and(|target| target.selection.is_some())
        {
            let take = self.model.clone();
            rows.push(AnyView::new(Button::title(
                "把选区加入文风样段",
                move || take.ai_voice_from_selection(),
            )));
        }
        rows.push(caption("避免的词语或句式（每行一条）"));
        rows.push(notes("避免的词语或句式", ai.avoid, 72.0));
        let save = self.model.clone();
        (
            rows,
            primary("保存手记", true, move || save.save_ai_brief()),
        )
    }

    fn writer(
        &self,
        preview_context: Signal<bool>,
        show_controls: Signal<bool>,
    ) -> (Vec<AnyView>, AnyView) {
        let ai = &self.model.ai;
        let batch = ai.batch();
        let story = ai.story_status();
        let preparing_story = matches!(story, Some(StoryStatus::Preparing { .. }));
        let busy = batch.as_ref().is_some_and(Batch::busy);
        let index = ai.viewed_result.get();
        let prompt = ai.prompt.get().string_value();
        let following =
            ai.follow_draft.get() && batch.as_ref().is_some_and(|batch| batch.can_revise(index));
        let starts_continuation = following && is_continuation_command(&prompt);
        let revising = following && !starts_continuation;
        let mut rows = Vec::new();
        if let Some(batch) = &batch {
            for turn in batch.history.iter() {
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
                    rows.push(assistant_message(answer));
                }
            }
            rows.push(author_message(&batch.prompt));
            self.results(&mut rows, batch);
        }
        let mode = if starts_continuation {
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
        if batch.as_ref().is_some_and(|batch| batch.can_revise(index)) {
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
                        model.refresh_ai_target();
                    })
                    .selected(!revising && mode == task),
                ));
            }
        }
        let mode_label = if revising {
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
            if self.model.ai_work().is_some() {
                let notes = self.model.clone();
                let mut brief = vec![AnyView::new(Button::title("作品手记", move || {
                    notes.edit_ai_brief()
                }))];
                if !self.model.ai_brief().is_empty() {
                    brief.push(AnyView::new(Toggle::labeled(
                        "采用手记",
                        ai.use_brief.binding(),
                    )));
                }
                rows.push(wrap_controls(brief));
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
            && if revising {
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
                if needs_story {
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
                match (revising, mode) {
                    (true, _) => "接着说你想怎么改…",
                    (false, WritingTask::Continue) => "这一段接下来发生什么？",
                    (false, WritingTask::Polish) => "这段正文要怎么改？",
                    (false, WritingTask::Request) => "问一个关于作品的问题…",
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
                rows.push(text_box(
                    "剧情大纲",
                    TextBuffer::new(&draft.profile.outline),
                    135.0,
                ));
                rows.push(text_box(
                    "人物小传与关系",
                    TextBuffer::new(&draft.profile.characters),
                    110.0,
                ));
                if !draft.profile.open_threads.trim().is_empty() {
                    rows.push(text_box(
                        "待续线索",
                        TextBuffer::new(&draft.profile.open_threads),
                        94.0,
                    ));
                }
                let confirm = self.model.clone();
                let edit = self.model.clone();
                let mut actions = vec![primary("确认并续写", true, move || {
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

    fn results(&self, rows: &mut Vec<AnyView>, batch: &Batch) {
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
                    let result = ai.viewed_result;
                    let ai = ai.clone();
                    let label = format!("{} · {}", run.profile.model, status_title(&run.status));
                    AnyView::new(
                        Button::new(Text::new(label).truncated(), move || {
                            result.set(index);
                            ai.viewed_source.set(None);
                            ai.clear_revision();
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
                if let Some(draft) = run.draft_in_progress() {
                    rows.push(text_box("当前草稿", draft.text.clone(), 206.0));
                }
            }
            ModelStatus::Cancelled => rows.push(caption("已停止等待这个模型。")),
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
                if !batch.applied() && editing && result.task != WritingTask::Request {
                    let lines = result
                        .text
                        .line_count()
                        .max(result.text.character_count().div_ceil(22));
                    rows.push(AnyView::new(
                        TextField::new("可编辑草稿", binding)
                            .multiline()
                            .frame_height((lines as f32 * 22.0 + 20.0).clamp(92.0, 230.0)),
                    ));
                } else {
                    rows.push(assistant_message(&result.text));
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
                        instructions.push(note);
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

fn assistant_message(text: &TextBuffer) -> AnyView {
    AnyView::new(
        Text::new(text.string_value())
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
        let scroll = cx.remember(|| RefCell::new(None));
        let settings = self.model.ai.settings_open.get();
        let brief = self.model.ai.brief_open.get();
        let (mut rows, action) = if settings {
            self.settings()
        } else if brief {
            self.brief()
        } else {
            self.writer(preview, controls)
        };
        let error = self.model.ai.error.get();
        if !error.is_empty() && (settings || brief) {
            rows.insert(0, error_label(error.clone()));
        }
        let title = if settings {
            "AI 模型设置"
        } else if brief {
            "作品手记"
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
        let heading = hstack(
            6.0,
            Align::Center,
            vec![
                AnyView::new(Text::new(title).semantic_font(SemanticFontRef::headline())),
                spacer(),
                AnyView::new(Button::title(
                    if settings || brief {
                        "返回写作"
                    } else if self.docked {
                        "收起"
                    } else {
                        "返回正文"
                    },
                    back,
                )),
            ],
        )
        .padding_insets(EdgeInsets::new(self.top_inset + 10.0, 14.0, 8.0, 14.0));
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
            let key = (
                self.model.document.get(),
                controls.get(),
                story_stage,
                self.model
                    .ai
                    .batch()
                    .map(|batch| (batch.history.len(), batch.busy(), batch.applied())),
            );
            let mut previous = scroll.borrow_mut();
            if previous.as_ref().is_none_or(|(last, _)| last != &key) {
                *previous = Some((key, ScrollRequest::new(Point::new(0.0, 1.0e9), 0.0)));
            }
            if let Some((_, request)) = &*previous {
                body = body.scroll_request(request.clone());
            }
        } else {
            *scroll.borrow_mut() = None;
        }
        let body = AnyView::new(body).flex_with(1.0, 1.0, Some(0.0));
        let footer = if settings || brief {
            hstack(6.0, Align::Center, vec![spacer(), action])
        } else {
            let mut composer = Vec::new();
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
        AnyView::new(
            vstack(
                0.0,
                Align::Stretch,
                vec![
                    AnyView::new(heading),
                    AnyView::new(body),
                    AnyView::new(footer),
                ],
            )
            .fill(),
        )
    }
}
