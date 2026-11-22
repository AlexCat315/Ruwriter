use std::rc::Rc;

use cove_ui::compose::modifiers::layout::{FlexExt, LayoutFrameExt, PaddingExt};
use cove_ui::compose::modifiers::style::{AppearanceExt, SemanticStyleViewExt};
use cove_ui::compose::primitives::controls::{Button, Toggle};
use cove_ui::compose::primitives::overlays::Dialog;
use cove_ui::compose::primitives::text::Text;
use cove_ui::compose::theme::styles::buttons::{
    ButtonStyleViewExt, PrimaryButtonStyle, ToggleButtonStyle,
};
use cove_ui::compose::theme::{SemanticColorRef, SemanticFontRef};
use cove_ui::compose::{ScrollAxis, ScrollView};
use cove_ui::composition::{AnyView, BoxView, EmptyView, Scope, TupleView, View};
use cove_ui::core::Binding;
use cove_ui::core::scene::{Align, FlexDirection, Justify, Wrap};
use ruwriter_core::WritingMode;

use super::ui::{hstack, spacer, vstack};
use crate::model::{FormatTarget, Model};
use crate::text_format::{self, BlankLines, FormatOptions, Indent};

pub struct FormatTextDialog {
    pub model: Model,
}

impl View for FormatTextDialog {
    fn body(&self, _cx: &Scope) -> AnyView {
        match self.model.format_dialog.get() {
            Some(target) => AnyView::new(FormatSettings {
                model: self.model.clone(),
                target,
            }),
            None => AnyView::new(EmptyView),
        }
    }
}

struct FormatSettings {
    model: Model,
    target: FormatTarget,
}

impl View for FormatSettings {
    fn body(&self, cx: &Scope) -> AnyView {
        let markdown = self.target.mode == WritingMode::Markdown;
        let selection = cx.state(|| self.target.selection.is_some());
        let settings = cx.state(|| FormatOptions::CLEAN);
        let error = cx.state(String::new);
        let options = settings.get();
        let selected_range = selection
            .get()
            .then(|| self.target.selection.clone())
            .flatten();
        let source = self.target.buffer.string_value();
        let plan = text_format::format(&source, selected_range, self.target.mode, options);
        let document_kind = if markdown { "Markdown" } else { "TXT" };
        let mut rows = vec![hstack(
            8.0,
            Align::Center,
            vec![
                caption(if selection.get() {
                    format!("{document_kind} · 处理选区覆盖的行")
                } else {
                    document_kind.to_owned()
                }),
                spacer(),
                choice(
                    "选区",
                    self.target.selection.is_some(),
                    selection.get(),
                    move || selection.set(true),
                ),
                choice("全文", true, !selection.get(), move || {
                    selection.set(false)
                }),
            ],
        )];

        if markdown {
            rows.push(caption(
                "保留 Markdown 的代码块、列表、引用、表格和强制换行。",
            ));
        } else {
            rows.push(hstack(
                8.0,
                Align::Center,
                vec![
                    caption("预设"),
                    choice(
                        "清理空白",
                        true,
                        options == FormatOptions::CLEAN,
                        move || settings.set(FormatOptions::CLEAN),
                    ),
                    choice(
                        "小说排版",
                        true,
                        options == FormatOptions::NOVEL,
                        move || settings.set(FormatOptions::NOVEL),
                    ),
                ],
            ));
            rows.push(option_group(
                "段首缩进",
                options.indent,
                [
                    ("保留缩进", Indent::Keep),
                    ("两格全角", Indent::TwoFullWidth),
                    ("顶格", Indent::Remove),
                ],
                move |indent| settings.update(|options| options.indent = indent),
            ));
            if self.model.paragraph_indent() {
                rows.push(caption(
                    "编辑器已开启显示缩进。下方预览按文件里的实际空格显示。",
                ));
            }
        }
        let mut blank_options = vec![
            ("保留空行", BlankLines::Keep),
            ("最多一行", BlankLines::Collapse),
        ];
        if !markdown {
            blank_options.extend([
                ("删除空行", BlankLines::Remove),
                ("段间一行", BlankLines::BetweenParagraphs),
            ]);
        }
        rows.push(option_group(
            "段间空行",
            options.blank_lines,
            blank_options,
            move |blank_lines| settings.update(|options| options.blank_lines = blank_lines),
        ));
        rows.push(wrap_options(vec![
            AnyView::new(Toggle::labeled(
                "清除行尾空格",
                Binding::new(
                    move || settings.get_untracked().trim_line_ends,
                    move |trim| settings.update(|options| options.trim_line_ends = trim),
                ),
            )),
            AnyView::new(Toggle::labeled(
                "清除文首文末空行",
                Binding::new(
                    move || settings.get_untracked().trim_outer_blank_lines,
                    move |trim| settings.update(|options| options.trim_outer_blank_lines = trim),
                ),
            )),
        ]));

        let status = match &plan {
            Ok(plan) => {
                rows.push(hstack(
                    10.0,
                    Align::Stretch,
                    vec![
                        preview("整理前", &plan.before_preview),
                        preview("整理后", &plan.after_preview),
                    ],
                ));
                rows.push(caption(if plan.changed_lines == 0 {
                    "预览当前范围。· 半角空格，□ 全角空格，⇥ 制表符。"
                } else {
                    "预览首处改动。· 半角空格，□ 全角空格，⇥ 制表符。"
                }));
                if plan.changed_lines == 0 {
                    "当前选项无需修改正文。".to_owned()
                } else {
                    format!("将整理 {} 行", plan.changed_lines)
                }
            }
            Err(_) => "这些改动会改变 Markdown 的解析结果。请调整选项。".to_owned(),
        };
        if plan.as_ref().is_ok_and(|plan| plan.kept_markdown_boundary) {
            rows.push(caption("已保留影响 Markdown 结构的文首文末空行。"));
        }
        let failure = error.get();
        if !failure.is_empty() {
            rows.push(AnyView::new(
                Text::new(failure).semantic_foreground(SemanticColorRef::error()),
            ));
        }

        let apply = self.model.clone();
        let target = self.target.clone();
        let plan = plan.ok().filter(|plan| plan.changed_lines > 0).map(Rc::new);
        let available = plan.is_some();
        let dialog = self.model.format_dialog;
        let footer = hstack(
            8.0,
            Align::Center,
            vec![
                caption(status),
                spacer(),
                AnyView::new(Button::title("取消", move || dialog.set(None))),
                AnyView::new(
                    Button::title("应用格式", move || {
                        if let Some(plan) = &plan
                            && let Err(message) = apply.apply_text_format(&target, plan)
                        {
                            error.set(message.to_owned());
                        }
                    })
                    .enabled(available)
                    .button_style(PrimaryButtonStyle),
                ),
            ],
        );
        let presented = Binding::new(
            || true,
            move |shown| {
                if !shown {
                    dialog.set(None);
                }
            },
        );
        AnyView::new(
            Dialog::new(presented, "格式化文本", vstack(12.0, Align::Stretch, rows))
                .footer(footer)
                .configure(|options| {
                    options.modal.fits_content_height = true;
                    options.modal.geometry.width = 640.0;
                }),
        )
    }
}

fn caption(text: impl Into<String>) -> AnyView {
    AnyView::new(
        Text::new(text.into())
            .semantic_font(SemanticFontRef::caption())
            .semantic_foreground(SemanticColorRef::on_surface_muted()),
    )
}

fn choice(title: &str, enabled: bool, selected: bool, action: impl Fn() + 'static) -> AnyView {
    AnyView::new(
        Button::title(title, action)
            .enabled(enabled)
            .selected(selected)
            .button_style(ToggleButtonStyle::default()),
    )
}

fn option_group<T: Copy + Eq + 'static>(
    title: &str,
    selected: T,
    options: impl IntoIterator<Item = (&'static str, T)>,
    apply: impl Fn(T) + 'static,
) -> AnyView {
    let apply = Rc::new(apply);
    let mut row = vec![caption(title)];
    row.extend(options.into_iter().map(|(label, value)| {
        let apply = apply.clone();
        choice(label, true, selected == value, move || apply(value))
    }));
    wrap_options(row)
}

fn wrap_options(controls: Vec<AnyView>) -> AnyView {
    AnyView::new(BoxView::new(
        FlexDirection::Row,
        Align::Center,
        Justify::FlexStart,
        Wrap::Wrap,
        8.0,
        AnyView::new(TupleView::new(controls)),
    ))
}

fn preview(title: &str, text: &str) -> AnyView {
    let text: String = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .map(|ch| match ch {
            ' ' => '·',
            '\u{3000}' => '□',
            '\t' => '⇥',
            ch if text_format::horizontal_space(ch) => '·',
            _ => ch,
        })
        .collect();
    let contents = if text.is_empty() {
        caption("（空）")
    } else {
        AnyView::new(Text::new(text))
    };
    AnyView::new(
        vstack(
            5.0,
            Align::Stretch,
            vec![
                caption(title),
                AnyView::new(
                    ScrollView::new(ScrollAxis::Vertical, contents.padding_edges(10.0, 8.0))
                        .frame_height(156.0)
                        .semantic_background(SemanticColorRef::surface_raised())
                        .semantic_border(SemanticColorRef::border(), 1.0)
                        .corner_radius(8.0),
                ),
            ],
        )
        .flex_with(1.0, 1.0, Some(0.0)),
    )
}
