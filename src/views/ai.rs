use cove_agent::AgentError;
use cove_ui::compose::modifiers::layout::{EdgeInsets, FlexExt, LayoutFrameExt, PaddingExt};
use cove_ui::compose::modifiers::style::SemanticStyleViewExt;
use cove_ui::compose::primitives::controls::{Button, MenuButton, Toggle};
use cove_ui::compose::primitives::overlays::{MenuEntry, MenuItem, PopoverPlacement};
use cove_ui::compose::primitives::text::text_field::TextFieldBehavior;
use cove_ui::compose::primitives::text::{Text, TextBuffer, TextField};
use cove_ui::compose::theme::styles::buttons::{
    ButtonStyleViewExt, PrimaryButtonStyle, ToggleButtonStyle,
};
use cove_ui::compose::theme::{SemanticColorRef, SemanticFontRef};
use cove_ui::compose::{ScrollAxis, ScrollView};
use cove_ui::composition::{AnyView, BoxView, EmptyView, Scope, TupleView, View};
use cove_ui::core::scene::{Align, FlexDirection, Justify, Wrap};
use cove_ui::core::{Binding, Signal};
use ruwriter_core::{AiPreferences, AiSettings, MAX_PARALLEL_AI_MODELS};

use super::ui::{hstack, spacer, vstack};
use crate::ai::{AiState, Batch, Catalog, ModelStatus, WritingResult, WritingTask, requires_key};
use crate::ai_research::ResearchScope;
use crate::model::Model;

pub struct AiPanel {
    pub model: Model,
    pub docked: bool,
    pub top_inset: f32,
    pub bottom_inset: f32,
}

const WRITING_ACTIONS: [(&str, WritingTask, Option<&str>); 5] = [
    ("续写", WritingTask::Continue, None),
    ("润色", WritingTask::Polish, None),
    (
        "摘要",
        WritingTask::Request,
        Some("概括当前文稿片段（或当前章节）的主要事件和人物进展。"),
    ),
    (
        "提纲",
        WritingTask::Request,
        Some("根据当前文稿片段（或当前章节）提取章节提纲。"),
    ),
    (
        "查连续性",
        WritingTask::Request,
        Some(
            "对照前文，检查人物、时间线、物品归属和未收束伏笔。优先报告有原文证据的冲突，附上引用和修订建议。证据不足时指出缺口。",
        ),
    ),
];

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
        rows.push(caption("硬设定和既定事实"));
        rows.push(notes("硬设定和既定事实", ai.canon, 105.0));
        rows.push(caption("文风要求和认可的样段"));
        rows.push(notes("文风要求和认可的样段", ai.voice, 105.0));
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

    fn writer(&self, preview_context: Signal<bool>) -> (Vec<AnyView>, AnyView) {
        let ai = &self.model.ai;
        let batch = ai.batch.get();
        let busy = batch.as_ref().is_some_and(Batch::busy);
        if let Some(batch) = batch.as_ref() {
            if !ai.show_request.get() || busy {
                let mut rows = Vec::new();
                self.results(&mut rows, batch);
                let request = ai.show_request;
                let stop = ai.clone();
                let action = if busy {
                    AnyView::new(Button::title("停止等待", move || stop.stop()))
                } else {
                    AnyView::new(Button::title("修改写作要求", move || {
                        request.set(true)
                    }))
                };
                return (rows, action);
            }
        }
        let chosen = ai.include_context.get();
        let mode = ai.task.get();
        let continuing = mode == WritingTask::Continue;
        let polishing = mode == WritingTask::Polish;
        let general_context = ai.context.get();
        let has_general_context = general_context.as_ref().is_ok_and(|text| !text.is_empty());
        let target = ai.target.get();
        let tail = target.as_ref().map_or_else(
            || Ok(TextBuffer::empty()),
            |target| target.continuation_context(),
        );
        let has_tail = tail.as_ref().is_ok_and(|text| !text.is_empty());
        let context = if continuing { tail } else { general_context };
        let has_context = if continuing {
            has_tail
        } else {
            has_general_context
        };
        let selected = target
            .as_ref()
            .is_some_and(|target| target.selection.is_some());
        let preferences = self.model.ai_settings();
        let can_research = !preferences.selected.is_empty()
            && preferences
                .selected
                .iter()
                .all(|&index| preferences.models[index].tools);
        let scope = ai.scope.get();
        let researching = !polishing && scope != ResearchScope::Excerpt;
        let folder = self.model.ai_project_folder();
        let actions: Vec<_> = WRITING_ACTIONS
            .into_iter()
            .filter(|(label, _, _)| researching || *label != "查连续性")
            .map(|action| {
                let attach = match action.1 {
                    WritingTask::Continue => has_tail,
                    WritingTask::Polish => selected && has_general_context,
                    WritingTask::Request => has_general_context,
                };
                let enabled = if action.1 == WritingTask::Request {
                    has_general_context || researching
                } else {
                    target.is_some()
                };
                task(action, ai, attach, !busy && enabled)
            })
            .collect();
        let manage = self.model.clone();
        let mut model_controls = vec![model_picker(&self.model, &preferences, !busy)];
        if preferences.models.len() > 1 {
            model_controls.push(parallel_picker(&self.model, &preferences, !busy));
        }
        model_controls.push(AnyView::new(
            Button::title("管理模型", move || manage.manage_ai()).enabled(!busy),
        ));
        let mut rows = vec![wrap_controls(actions), wrap_controls(model_controls)];
        let mut work_controls = Vec::new();
        if self.docked {
            let align = self.model.clone();
            work_controls.push(AnyView::new(Button::title(
                "对准当前光标",
                move || align.refresh_ai_target(),
            )));
        }
        if self.model.ai_work().is_some() {
            let notes = self.model.clone();
            work_controls.push(AnyView::new(Button::title("作品手记", move || {
                notes.edit_ai_brief()
            })));
            if !self.model.ai_brief().is_empty() {
                work_controls.push(AnyView::new(Toggle::labeled(
                    "采用手记",
                    ai.use_brief.binding(),
                )));
            }
        }
        if !work_controls.is_empty() {
            rows.push(wrap_controls(work_controls));
        }
        if can_research && target.is_some() && !polishing {
            let mut scopes = vec![
                ("仅片段", ResearchScope::Excerpt),
                ("本文稿", ResearchScope::Document),
            ];
            if let Some(folder) = &folder {
                scopes.push((
                    if folder.is_empty() {
                        "整个资料库"
                    } else {
                        "作品文件夹"
                    },
                    ResearchScope::Project,
                ));
            }
            let entries = scopes
                .iter()
                .enumerate()
                .map(|(i, &(label, value))| {
                    let choose = ai.scope;
                    MenuEntry::Item(
                        MenuItem::new(i as u64 + 1, label, move || choose.set(value))
                            .selected(scope == value),
                    )
                })
                .collect();
            let label = scopes
                .iter()
                .find(|(_, value)| *value == scope)
                .map_or("仅片段", |&(label, _)| label);
            rows.push(hstack(
                6.0,
                Align::Center,
                vec![caption("参考范围"), chooser(label, entries, !busy)],
            ));
        }
        if researching {
            let scope_note = if let Some(folder) =
                folder.filter(|folder| scope == ResearchScope::Project && !folder.is_empty())
            {
                format!("在本机搜索「{folder}」，所选模型会收到命中的原文。")
            } else {
                "在本机检索，所选模型会收到查阅的原文。".to_owned()
            };
            rows.push(caption(scope_note));
        }
        if continuing && let Some(target) = &target {
            rows.push(caption(target.continuation_anchor().map_or_else(
                || "从当前光标位置起写".to_owned(),
                |anchor| format!("接在「{anchor}」后"),
            )));
        }
        rows.push(AnyView::new(
            TextField::new(
                match mode {
                    WritingTask::Continue => "接下来发生什么？可留空",
                    WritingTask::Polish => "润色要求（可选）",
                    WritingTask::Request => "输入写作要求",
                },
                ai.prompt.binding(),
            )
            .multiline()
            .behavior(TextFieldBehavior {
                disabled: busy,
                ..Default::default()
            })
            .frame_height(76.0),
        ));
        if has_context {
            let mut controls = if polishing {
                vec![caption("润色当前选区")]
            } else {
                vec![AnyView::new(
                    Toggle::labeled(
                        if continuing && selected {
                            "参考选区末尾"
                        } else if continuing {
                            "参考落笔前文"
                        } else if selected {
                            "附带选区"
                        } else {
                            "附带光标前文（最多 2,000 字符）"
                        },
                        ai.include_context.binding(),
                    )
                    .enabled(!busy),
                )]
            };
            if chosen || polishing {
                controls.push(AnyView::new(Button::title(
                    if preview_context.get() {
                        "收起原文"
                    } else {
                        "查看参考原文"
                    },
                    move || preview_context.update(|shown| *shown = !*shown),
                )));
            }
            rows.push(wrap_controls(controls));
        }
        match context {
            Ok(text) if (chosen || polishing) && !text.is_empty() && preview_context.get() => {
                rows.push(text_box("将提供的文稿片段", text, 88.0))
            }
            Err(message) => rows.push(caption(message)),
            _ => {}
        }
        if batch.is_some() {
            let show = ai.show_request;
            rows.push(AnyView::new(Button::title(
                "查看当前草稿",
                move || show.set(false),
            )));
        }
        let send = self.model.clone();
        let action = if preferences.models.is_empty() {
            primary("添加模型", true, move || send.manage_ai())
        } else {
            primary(
                if preferences.selected.len() > 1 {
                    "并行生成"
                } else {
                    match mode {
                        WritingTask::Continue => "生成续写",
                        WritingTask::Polish => "生成润色",
                        WritingTask::Request => "发送",
                    }
                },
                if polishing {
                    selected && has_context
                } else {
                    (continuing && chosen && has_context) || !ai.prompt.get().is_empty()
                },
                move || send.run_ai(),
            )
        };
        (rows, action)
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
                    let source = ai.viewed_source;
                    let label = format!("{} · {}", run.profile.model, status_title(&run.status));
                    AnyView::new(
                        Button::new(Text::new(label).truncated(), move || {
                            result.set(index);
                            source.set(None);
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
            ModelStatus::Running => rows.push(caption("正在等待模型生成…")),
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
            ModelStatus::Failed(error) => rows.push(error_label(format!("请求失败。{error}"))),
            ModelStatus::Ready(result) => {
                if result.task == WritingTask::Continue
                    && let Some(anchor) = batch
                        .target
                        .as_ref()
                        .and_then(|target| target.continuation_anchor())
                {
                    rows.push(caption(format!("接在「{anchor}」后")));
                }
                if !result.continuity_notes.is_empty() {
                    rows.push(caption("设定疑点 · 请核对"));
                    for note in result.continuity_notes.iter() {
                        rows.push(caption(note));
                    }
                }
                let modified = result.text != result.original_text;
                let repeats = !modified && result.repetition_warning.is_some();
                let binding = ai.draft_binding(batch, current);
                let mut header = vec![caption("草稿可直接编辑")];
                if modified {
                    let original = result.original_text.clone();
                    let restore = binding.clone();
                    header.push(AnyView::new(Button::title("恢复初稿", move || {
                        restore.set(original.clone())
                    })));
                }
                rows.push(wrap_controls(header));
                if let Some(warning) = &result.repetition_warning {
                    rows.push(if modified {
                        caption(warning)
                    } else {
                        error_label(warning.clone())
                    });
                }
                rows.push(AnyView::new(
                    TextField::new("生成结果", binding)
                        .multiline()
                        .frame_height(145.0),
                ));
                show_sources(rows, result, ai.viewed_source);
                if has_target {
                    let insert = self.model.clone();
                    let replace = self.model.clone();
                    let ready = !result.text.is_empty();
                    let mut buttons = Vec::new();
                    if result.task == WritingTask::Polish {
                        if selected {
                            buttons.push(primary("替换选区", ready, move || {
                                replace.apply_ai(current, true, docked)
                            }));
                        }
                    } else if result.task == WritingTask::Continue {
                        buttons.push(primary(
                            if repeats {
                                "仍然接在这里"
                            } else {
                                "接在这里"
                            },
                            ready,
                            move || insert.apply_ai(current, false, docked),
                        ));
                    } else {
                        buttons.push(primary("插入", ready, move || {
                            insert.apply_ai(current, false, docked)
                        }));
                        if selected {
                            buttons.push(AnyView::new(
                                Button::title("替换选区", move || {
                                    replace.apply_ai(current, true, docked);
                                })
                                .enabled(ready),
                            ));
                        }
                    }
                    rows.push(wrap_controls(buttons));
                }
            }
        }
    }
}

fn status_title(status: &ModelStatus) -> &'static str {
    match status {
        ModelStatus::Indexing => "建索引",
        ModelStatus::Running => "生成中",
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

fn task(
    (label, mode, instruction): (&str, WritingTask, Option<&'static str>),
    ai: &AiState,
    attach: bool,
    enabled: bool,
) -> AnyView {
    let (current, prompt, include, error) = (ai.task, ai.prompt, ai.include_context, ai.error);
    let selected = mode != WritingTask::Request && current.get() == mode;
    let button = Button::title(label, move || {
        if mode == WritingTask::Polish && !attach {
            error.set("先在正文里选中要润色的文字，再打开助手。".to_owned());
            return;
        }
        error.set(String::new());
        if let Some(instruction) = instruction {
            current.set(WritingTask::Request);
            prompt.set(TextBuffer::new(instruction));
            include.set(attach);
        } else {
            let active = current.get_untracked() == mode;
            current.set(if active { WritingTask::Request } else { mode });
            include.set(!active && attach);
            let typed = prompt.get_untracked().string_value();
            if !active
                && WRITING_ACTIONS
                    .iter()
                    .any(|(_, _, instruction)| *instruction == Some(typed.as_str()))
            {
                prompt.set(TextBuffer::empty());
            }
        }
    })
    .selected(selected)
    .enabled(enabled);
    if mode != WritingTask::Request {
        AnyView::new(button.button_style(ToggleButtonStyle::default()))
    } else {
        AnyView::new(button)
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
        let settings = self.model.ai.settings_open.get();
        let brief = self.model.ai.brief_open.get();
        let (mut rows, action) = if settings {
            self.settings()
        } else if brief {
            self.brief()
        } else {
            self.writer(preview)
        };
        let error = self.model.ai.error.get();
        if !error.is_empty() {
            rows.insert(0, error_label(error));
        }
        let feedback = self.model.ai.feedback.get();
        if !feedback.is_empty() && !settings && !brief {
            rows.insert(0, caption(feedback));
        }
        let title = if settings {
            "AI 模型设置"
        } else if brief {
            "作品手记"
        } else {
            "AI 写作"
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
        let body = AnyView::new(ScrollView::new(
            ScrollAxis::Vertical,
            AnyView::new(vstack(9.0, Align::Stretch, rows).padding_edges(14.0, 8.0)),
        ))
        .flex_with(1.0, 1.0, Some(0.0));
        let footer = hstack(6.0, Align::Center, vec![spacer(), action])
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
