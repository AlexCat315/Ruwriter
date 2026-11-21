use cove_agent::AgentError;
use cove_ui::compose::modifiers::layout::{LayoutFrameExt, PaddingExt};
use cove_ui::compose::modifiers::style::SemanticStyleViewExt;
use cove_ui::compose::primitives::controls::{Button, MenuButton, Toggle};
use cove_ui::compose::primitives::overlays::{Dialog, MenuEntry, MenuItem, PopoverPlacement};
use cove_ui::compose::primitives::text::text_field::TextFieldBehavior;
use cove_ui::compose::primitives::text::{Text, TextBuffer, TextField};
use cove_ui::compose::theme::styles::buttons::{
    ButtonStyleViewExt, PrimaryButtonStyle, ToggleButtonStyle,
};
use cove_ui::compose::theme::{SemanticColorRef, SemanticFontRef};
use cove_ui::composition::{AnyView, BoxView, EmptyView, Scope, TupleView, View};
use cove_ui::core::scene::{Align, FlexDirection, Justify, Wrap};
use cove_ui::core::{Binding, Signal};

use super::ui::{hstack, spacer, vstack};
use crate::ai::{Response, WritingResult, requires_key};
use crate::ai_research::ResearchScope;
use crate::model::Model;

pub struct AiDialog {
    pub model: Model,
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

impl AiDialog {
    fn settings(&self) -> (Vec<AnyView>, AnyView) {
        let ai = &self.model.ai;
        let endpoint = ai.endpoint;
        let endpoint_editor = ai.clone();
        let address = endpoint.get().string_value();
        let key_required = requires_key(&address);
        let rows = vec![
            hstack(
                6.0,
                Align::Center,
                vec![
                    preset("Ollama", "http://127.0.0.1:11434/v1", &self.model),
                    preset("LM Studio", "http://127.0.0.1:1234/v1", &self.model),
                ],
            ),
            field(
                "API 地址",
                Binding::new(
                    move || endpoint.get(),
                    move |value| endpoint_editor.change_endpoint(value),
                ),
                false,
                2048,
            ),
            field("模型名称", ai.model.binding(), false, 256),
            field(
                if key_required {
                    "DeepSeek API 密钥"
                } else {
                    "API 密钥（可选）"
                },
                ai.api_key.binding(),
                true,
                4096,
            ),
            AnyView::new(Toggle::labeled("使用只读文稿工具", ai.tools.binding())),
            caption("开启工具调用时，请使用支持工具调用的模型。API 密钥仅保留在当前应用会话中。"),
        ];
        let model = self.model.clone();
        let action = primary(
            "使用此模型",
            !ai.endpoint.get().is_empty() && !ai.model.get().is_empty(),
            move || model.save_ai_preferences(),
        );
        (rows, action)
    }

    fn writer(&self) -> (Vec<AnyView>, AnyView) {
        let ai = &self.model.ai;
        let response = ai.response.get();
        let busy = matches!(response, Response::Running(..));
        let chosen = ai.include_context.get();
        let context = ai.context.get();
        let has_context = context.as_ref().is_ok_and(|text| !text.is_empty());
        let target = ai.target.get();
        let selected = target
            .as_ref()
            .is_some_and(|target| target.selection.is_some());
        let preferences = self
            .model
            .read(|session| session.ai_preferences().clone())
            .unwrap_or_default();
        let endpoint = preferences.endpoint;
        let scope = ai.scope.get();
        let researching = scope != ResearchScope::Excerpt;
        let folder = self.model.ai_project_folder();
        let settings = ai.settings_open;
        let error = ai.error;
        let missing_key = ai.missing_key(&endpoint);
        let unauthorized = matches!(response, Response::Failed(AgentError::HttpStatus(401)));
        let edit_settings = move || {
            error.set(String::new());
            settings.set(true);
        };
        let actions: Vec<_> = [
            ("润色", "润色提供的文稿片段，保留事实、人物和叙述视角。只给出润色后的正文。"),
            ("续写", "从当前落笔位置接着写一小段，延续原文语气。若可检索，先核对当前情节涉及的人物和伏笔。只给出新增正文。"),
            ("摘要", "概括当前文稿片段（或当前章节）的主要事件和人物进展。"),
            ("提纲", "根据当前文稿片段（或当前章节）提取章节提纲。"),
            ("查连续性", "对照授权范围内的前文，检查当前文稿片段（或当前章节）涉及的人物、时间线、物品归属和未收束伏笔。优先报告有原文证据的冲突，附上引用和修订建议；如证据不足，请指出具体缺口。"),
        ]
        .into_iter()
        .filter(|(label, _)| researching || *label != "查连续性")
        .map(|(label, prompt)| {
            task(
                label,
                prompt,
                ai.prompt,
                ai.include_context,
                has_context,
                !busy && (has_context || (researching && label != "润色")),
            )
        })
        .collect();
        let mut scopes = Vec::new();
        if preferences.tools && target.is_some() {
            scopes.push(scope_button(
                "仅片段",
                ResearchScope::Excerpt,
                ai.scope,
                scope,
                !busy,
            ));
            scopes.push(scope_button(
                "本文稿",
                ResearchScope::Document,
                ai.scope,
                scope,
                !busy,
            ));
            if let Some(folder) = &folder {
                let label = if folder.is_empty() {
                    "整个资料库"
                } else {
                    "作品文件夹"
                };
                scopes.push(scope_button(
                    label,
                    ResearchScope::Project,
                    ai.scope,
                    scope,
                    !busy,
                ));
            }
        }
        let mut rows = vec![hstack(
            6.0,
            Align::Center,
            vec![
                caption(format!("发送到 {endpoint}")),
                spacer(),
                AnyView::new(Button::title("模型设置", edit_settings).enabled(!busy)),
            ],
        )];
        if !scopes.is_empty() {
            rows.push(hstack(6.0, Align::Center, scopes));
        }
        if researching {
            if scope == ResearchScope::Project
                && let Some(folder) = folder.filter(|folder| !folder.is_empty())
            {
                rows.push(caption(format!("搜索「{folder}」及其子文件夹")));
            }
            rows.push(caption(
                "文稿在本机索引。所选模型会收到工具返回的目录和原文片段。",
            ));
        }
        rows.push(AnyView::new(BoxView::new(
            FlexDirection::Row,
            Align::Center,
            Justify::FlexStart,
            Wrap::Wrap,
            6.0,
            AnyView::new(TupleView::new(actions)),
        )));
        rows.push(AnyView::new(
            TextField::new("输入写作要求", ai.prompt.binding())
                .multiline()
                .behavior(TextFieldBehavior {
                    disabled: busy,
                    max_length: Some(4000),
                    ..Default::default()
                })
                .frame_height(76.0),
        ));
        if has_context {
            rows.push(AnyView::new(
                Toggle::labeled(
                    if selected {
                        "附带选区"
                    } else {
                        "附带光标前文（最多 2,000 字符）"
                    },
                    ai.include_context.binding(),
                )
                .enabled(!busy),
            ));
        }
        match context {
            Ok(text) if chosen && !text.is_empty() => {
                rows.push(text_box("将提供的文稿片段", text, 88.0))
            }
            Err(message) => rows.push(caption(message)),
            _ => {}
        }
        if missing_key {
            rows.push(caption("本次运行尚未填写 DeepSeek API 密钥。"));
        }
        match response {
            Response::Idle => {}
            Response::Running(_, true) => rows.push(caption("正在建立本地章节索引…")),
            Response::Running(_, false) => rows.push(caption("正在等待模型生成…")),
            Response::Failed(AgentError::HttpStatus(401)) => {
                rows.push(error_label(
                    "模型服务未接受当前凭据（401）。请填写或更新 API 密钥。".to_owned(),
                ));
            }
            Response::Failed(error) => rows.push(error_label(format!("AI 请求失败：{error}"))),
            Response::Ready(result) => {
                rows.push(caption("生成结果"));
                rows.push(text_box("生成结果", result.text.clone(), 145.0));
                show_sources(&mut rows, &result, ai.viewed_source);
                if target.is_some() {
                    let insert = self.model.clone();
                    let replace = self.model.clone();
                    let mut buttons = vec![AnyView::new(Button::title("插入", move || {
                        insert.apply_ai(false)
                    }))];
                    if selected {
                        buttons.push(AnyView::new(Button::title("替换选区", move || {
                            replace.apply_ai(true)
                        })));
                    }
                    rows.push(hstack(8.0, Align::Center, buttons));
                }
            }
        }
        let run = self.model.clone();
        let cancel = ai.clone();
        let action = if busy {
            AnyView::new(Button::title("停止等待", move || cancel.cancel()))
        } else if missing_key || unauthorized {
            let label = if missing_key || ai.api_key.get().is_empty() {
                "填写 API 密钥"
            } else {
                "更新 API 密钥"
            };
            primary(label, true, edit_settings)
        } else {
            primary("发送", !ai.prompt.get().is_empty(), move || run.run_ai())
        };
        (rows, action)
    }
}

fn scope_button(
    label: &str,
    value: ResearchScope,
    scope: Signal<ResearchScope>,
    current: ResearchScope,
    enabled: bool,
) -> AnyView {
    AnyView::new(
        AnyView::new(
            Button::title(label, move || scope.set(value))
                .selected(current == value)
                .enabled(enabled),
        )
        .button_style(ToggleButtonStyle::default()),
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

fn task(
    label: &str,
    instruction: &'static str,
    prompt: Signal<TextBuffer>,
    include: Signal<bool>,
    has_context: bool,
    enabled: bool,
) -> AnyView {
    AnyView::new(
        Button::title(label, move || {
            prompt.set(TextBuffer::new(instruction));
            include.set(has_context);
        })
        .enabled(enabled),
    )
}

fn error_label(message: String) -> AnyView {
    AnyView::new(Text::new(message).semantic_foreground(SemanticColorRef::error()))
}

impl View for AiDialog {
    fn body(&self, _cx: &Scope) -> AnyView {
        if !self.model.ai.open.get() {
            return AnyView::new(EmptyView);
        }
        let settings = self.model.ai.settings_open.get();
        let (mut rows, action) = if settings {
            self.settings()
        } else {
            self.writer()
        };
        let error = self.model.ai.error.get();
        if !error.is_empty() {
            rows.push(error_label(error));
        }
        let close = self.model.clone();
        let dismiss = self.model.clone();
        let binding = Binding::new(
            || true,
            move |shown: bool| {
                if !shown {
                    dismiss.close_ai();
                }
            },
        );
        let footer = hstack(
            8.0,
            Align::Center,
            vec![
                AnyView::new(Button::title("关闭", move || close.close_ai())),
                spacer(),
                action,
            ],
        );
        let dialog = Dialog::new(
            binding,
            if settings {
                "AI 模型设置"
            } else {
                "AI 助手"
            },
            vstack(9.0, Align::Stretch, rows),
        )
        .footer(footer)
        .configure(|options| {
            options.modal.geometry.width = 600.0;
            options.modal.geometry.height = if settings { 440.0 } else { 650.0 };
        });
        AnyView::new(AnyView::new(dialog).padding(4.0))
    }
}
