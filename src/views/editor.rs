//! The writing surface: a quiet toolbar, the editor as a centered readable
//! column that scrolls across the whole pane, an optional side-by-side
//! preview, and a one-line count at the bottom.

use std::rc::Rc;

use cove_ui::compose::modifiers::layout::{FlexExt, LayoutFrameExt, PaddingExt};
use cove_ui::compose::modifiers::style::SemanticStyleViewExt;
use cove_ui::compose::primitives::controls::{Button, ButtonIcon, ButtonIconSource, MenuButton};
use cove_ui::compose::primitives::media::ui_icon_catalog as icons;
use cove_ui::compose::primitives::overlays::{MenuEntry, MenuItem, MenuItemRole};
use cove_ui::compose::primitives::text::text_field::{
    TextFieldCodeEditing, TextFieldContentPadding, TextFieldDecoration, TextFieldNavigationRequest,
};
use cove_ui::compose::primitives::text::{
    Divider, MarkdownView, Text, TextField, TextFieldAxis, TextFieldBehavior, TextFieldEvents,
    TextFieldIndentSkip, TextFieldLayout,
};
use cove_ui::compose::theme::styles::buttons::{ButtonStyleViewExt, GhostButtonStyle};
use cove_ui::compose::theme::styles::components::text_field_style::AnyTextFieldStyle;
use cove_ui::compose::theme::{SemanticColorRef, SemanticFontRef};
use cove_ui::compose::{FrameDimension, ScrollAxis, ScrollView};
use cove_ui::composition::{AnyView, BoxView, EmptyView, LayoutDebugExt, Scope, View, ViewExt};
use cove_ui::core::scene::{Align, FlexDirection, Justify, Wrap};
use cove_ui::imaging::BundleImageResource;

use super::root::Layout;
use super::ui::{ProseFieldStyle, full_size, hstack, icon_button, spacer, vstack};
use crate::formatting::{Command, skip_prose_indent};
use crate::markdown_syntax;
use crate::model::Model;
use crate::theme::{Mode, SyntaxColors, editor_font, measure, novel_font};

type Action = Rc<dyn Fn(&Model)>;

/// Narrowest side margin around the text column.
const MARGIN: f32 = 28.0;
/// Panes narrower than this show the preview instead of beside the editor.
const SPLIT_WIDTH: f32 = 880.0;
/// Below this width the formatting buttons fold into one menu.
const FORMAT_BAR_WIDTH: f32 = 620.0;

pub struct EditorPane {
    pub model: Model,
    pub layout: Layout,
    pub mode: Mode,
}

impl View for EditorPane {
    fn body(&self, _cx: &Scope) -> AnyView {
        let _ = self.model.revision.get();
        let _ = self.model.settings_revision.get();
        let options = self.model.options();
        let settings = self.model.settings();
        let font_size = settings.font_size;
        let split = options.preview && self.layout.editor_width >= SPLIT_WIDTH;
        let pane_width = if split {
            (self.layout.editor_width - 1.0) / 2.0
        } else {
            self.layout.editor_width
        };
        let column = measure(font_size, settings.column_width.glyphs());
        let margin = (pane_width * 0.055).clamp(12.0, MARGIN);
        let horizontal = ((pane_width - column) / 2.0).max(margin);

        let editor = AnyView::new(EditorField {
            model: self.model.clone(),
            mode: self.mode,
            font_size,
            horizontal,
        });
        let preview = || {
            AnyView::new(PreviewPane {
                model: self.model.clone(),
                column,
                horizontal,
            })
        };
        let body: AnyView = if split {
            hstack(
                0.0,
                Align::Stretch,
                vec![
                    AnyView::new(editor.flex_with(1.0, 1.0, Some(0.0))),
                    AnyView::new(Divider::vertical()),
                    AnyView::new(preview().flex_with(1.0, 1.0, Some(0.0))),
                ],
            )
        } else if options.preview {
            hstack(
                0.0,
                Align::Stretch,
                vec![
                    AnyView::new(EmptyView),
                    AnyView::new(EmptyView),
                    AnyView::new(preview().flex_with(1.0, 1.0, Some(0.0))),
                ],
            )
        } else {
            hstack(
                0.0,
                Align::Stretch,
                vec![
                    AnyView::new(editor.flex_with(1.0, 1.0, Some(0.0))),
                    AnyView::new(EmptyView),
                    AnyView::new(EmptyView),
                ],
            )
        };

        let mut children = vec![self.toolbar()];
        if self.model.find_open.get() {
            children.push(AnyView::new(FindBar {
                model: self.model.clone(),
            }));
        }
        children.push(AnyView::new(body.flex_with(1.0, 1.0, Some(0.0))));
        children.push(AnyView::new(StatusLine {
            model: self.model.clone(),
            bottom_inset: self.layout.bottom_inset,
        }));
        full_size(vstack(0.0, Align::Stretch, children))
    }
}

impl EditorPane {
    fn toolbar(&self) -> AnyView {
        let model = &self.model;
        let options = model.options();
        let mut leading: Vec<AnyView> = Vec::new();
        if self.layout.single_pane && !options.focus {
            let back = model.clone();
            leading.push(icon_button(
                icons::navigation::chevron_left(),
                "文稿列表",
                false,
                move || back.show_list(),
            ));
        } else if !self.layout.single_pane {
            let sidebar = model.clone();
            leading.push(icon_button(
                icons::layout::panel_left(),
                "显示或隐藏侧栏",
                options.sidebar && !options.focus,
                move || sidebar.toggle_sidebar(),
            ));
        }

        let preview = model.clone();
        let focus = model.clone();
        let outline = model.clone();
        let mut trailing = vec![];
        if !self.layout.single_pane {
            trailing.push(icon_button(
                icons::layout::panel_right(),
                "显示或隐藏大纲",
                options.outline && self.layout.outline,
                move || outline.toggle_outline(),
            ));
        }
        trailing.extend([
            icon_button(
                icons::layout::columns_2(),
                if options.preview && self.layout.editor_width < SPLIT_WIDTH {
                    "返回编辑"
                } else {
                    "预览"
                },
                options.preview,
                move || preview.toggle_preview(),
            ),
            icon_button(
                icons::action::focus(),
                "专注模式",
                options.focus,
                move || focus.toggle_focus(),
            ),
        ]);
        let ai = model.clone();
        trailing.push(icon_button(
            icons::status::sparkles(),
            "AI 助手",
            model.ai.open.get(),
            move || ai.toggle_ai(),
        ));
        trailing.push(AnyView::new(
            MenuButton::new(
                document_menu(model),
                ButtonIcon::new(
                    ButtonIconSource::resource(icons::action::more_horizontal()),
                    Some(16.0),
                    None,
                ),
            )
            .ghost()
            .accessibility_label("更多")
            .tooltip("更多"),
        ));

        let mut children = leading;
        if !options.focus {
            children.extend(self.formatting_controls());
        }
        children.push(spacer());
        children.extend(trailing);
        AnyView::new(hstack(4.0, Align::Center, children).padding_insets(
            cove_ui::compose::modifiers::layout::EdgeInsets {
                top: self.layout.top_inset + 8.0,
                leading: 10.0,
                bottom: 4.0,
                trailing: 10.0,
            },
        ))
    }

    /// Document cleanup and Markdown markup use separate controls.
    fn formatting_controls(&self) -> Vec<AnyView> {
        let model = &self.model;
        let formatter = model.clone();
        let mut controls = vec![AnyView::new(
            Button::title("格式化", move || formatter.open_text_formatter())
                .accessibility_text("格式化文本")
                .tooltip("格式化文本")
                .button_style(GhostButtonStyle),
        )];
        if model.writing_mode() == ruwriter_core::WritingMode::Novel {
            return controls;
        }
        if self.layout.editor_width < FORMAT_BAR_WIDTH {
            controls.push(AnyView::new(
                MenuButton::new(
                    format_menu(model),
                    ButtonIcon::new(
                        ButtonIconSource::resource(icons::text::type_()),
                        Some(16.0),
                        None,
                    ),
                )
                .ghost()
                .accessibility_label("Markdown 样式")
                .tooltip("Markdown 样式"),
            ));
            return controls;
        }
        let buttons: [(BundleImageResource, &str, Command); 8] = [
            (icons::text::bold(), "加粗 ⌘B", Command::Bold),
            (icons::text::italic(), "斜体 ⌘I", Command::Italic),
            (icons::text::heading(), "标题 ⇧⌘H", Command::Heading),
            (icons::text::list(), "无序列表 ⇧⌘8", Command::BulletList),
            (
                icons::text::list_ordered(),
                "有序列表 ⇧⌘7",
                Command::NumberedList,
            ),
            (icons::shape::message_square(), "引用 ⇧⌘9", Command::Quote),
            (icons::editor::code(), "行内代码 ⌘E", Command::Code),
            (icons::action::link(), "链接 ⇧⌘K", Command::Link),
        ];
        controls.extend(buttons.into_iter().map(|(icon, label, command)| {
            let target = model.clone();
            icon_button(icon, label, false, move || target.format(command))
        }));
        controls
    }
}

/// Every formatting command with its shortcut, for the narrow-pane menu.
fn format_menu(model: &Model) -> Vec<MenuEntry> {
    let actions: [(&str, &str, Command); 9] = [
        ("加粗", "⌘B", Command::Bold),
        ("斜体", "⌘I", Command::Italic),
        ("删除线", "⇧⌘X", Command::Strikethrough),
        ("行内代码", "⌘E", Command::Code),
        ("链接", "⇧⌘K", Command::Link),
        ("标题", "⇧⌘H", Command::Heading),
        ("无序列表", "⇧⌘8", Command::BulletList),
        ("有序列表", "⇧⌘7", Command::NumberedList),
        ("引用", "⇧⌘9", Command::Quote),
    ];
    let mut entries = Vec::new();
    for (index, (title, shortcut, command)) in actions.into_iter().enumerate() {
        let target = model.clone();
        entries.push(MenuEntry::Item(
            MenuItem::new(index as u64 + 1, title, move || target.format(command))
                .shortcut(shortcut),
        ));
        if index == 4 {
            entries.push(MenuEntry::separator(50));
        }
    }
    entries
}

fn document_menu(model: &Model) -> Vec<MenuEntry> {
    let pinned = model.active_document().is_some_and(|d| d.pinned);
    let actions: [(&str, Action); 8] = [
        ("新建文稿", Rc::new(Model::create)),
        ("打开文件…", Rc::new(Model::open_picker)),
        ("导入到资料库…", Rc::new(Model::import_picker)),
        ("导出文稿…", Rc::new(Model::export)),
        (
            if pinned { "取消置顶" } else { "置顶" },
            Rc::new(move |model: &Model| {
                if let Some(id) = model.active_document().map(|d| d.id) {
                    model.set_pinned(&id, !pinned);
                }
            }),
        ),
        (
            "放大文字",
            Rc::new(|model: &Model| model.adjust_font_size(1)),
        ),
        (
            "缩小文字",
            Rc::new(|model: &Model| model.adjust_font_size(-1)),
        ),
        ("命令面板", Rc::new(Model::toggle_palette)),
    ];
    let shortcuts = ["⌘N", "⌘O", "⇧⌘I", "⇧⌘E", "", "⌘=", "⌘-", "⌘K"];
    let mut entries: Vec<MenuEntry> = Vec::new();
    for (index, ((title, action), shortcut)) in actions.into_iter().zip(shortcuts).enumerate() {
        let target = model.clone();
        let mut item = MenuItem::new(index as u64 + 1, title, move || action(&target));
        if !shortcut.is_empty() {
            item = item.shortcut(shortcut);
        }
        entries.push(MenuEntry::Item(item));
        if index == 6 {
            let toggle = model.clone();
            entries.push(MenuEntry::Item(
                MenuItem::new(61, "打字机滚动", move || {
                    toggle.toggle_typewriter_scrolling()
                })
                .selected(model.typewriter_scrolling()),
            ));
        }
        if matches!(index, 3 | 4 | 6) {
            entries.push(MenuEntry::separator(100 + index as u64));
        }
    }
    let trash = model.clone();
    entries.push(MenuEntry::Item(
        MenuItem::new(200, "移到废纸篓", move || trash.trash_active())
            .role(MenuItemRole::Destructive),
    ));
    entries
}

/// The text field, rebuilt per document so caret, scroll and undo history
/// start fresh, but never per keystroke.
struct EditorField {
    model: Model,
    mode: Mode,
    font_size: u8,
    horizontal: f32,
}

impl View for EditorField {
    fn body(&self, _cx: &Scope) -> AnyView {
        let document = self.model.document.get();
        let navigation = self.model.navigation.get();
        let edit = self.model.edit.get();
        let change = self.model.clone();
        let blur = self.model.clone();
        let caret = self.model.clone();
        let keys = self.model.clone();
        let colors = SyntaxColors::for_theme(&self.mode.theme());
        let settings = self.model.settings();
        let novel = self.model.writing_mode() == ruwriter_core::WritingMode::Novel;
        let mut code = TextFieldCodeEditing {
            occurrences: self.find_occurrences(),
            ..Default::default()
        };
        if settings.markdown_syntax && !novel {
            code.syntax_color_at_utf8_offset = Some(markdown_syntax::highlighter(colors));
        }
        let mut field = TextField::new("开始写作…", self.model.buffer.binding())
            .layout(TextFieldLayout {
                axis: TextFieldAxis::Vertical,
                max_visible_lines: 6,
                wraps_lines: true,
                first_line_indent: if novel && settings.paragraph_indent {
                    f32::from(self.font_size) * 2.0
                } else {
                    0.0
                },
                first_line_indent_skip: if novel {
                    TextFieldIndentSkip::new(skip_prose_indent)
                } else {
                    TextFieldIndentSkip::default()
                },
                typewriter_scrolling: settings.typewriter_scrolling,
                ..Default::default()
            })
            .decoration(TextFieldDecoration {
                content_padding: Some(TextFieldContentPadding::new(self.horizontal, 24.0)),
                ..Default::default()
            })
            .code_editing(code)
            .style(AnyTextFieldStyle::new(ProseFieldStyle))
            .events(TextFieldEvents {
                on_change: Some(Rc::new(move |buffer| change.on_change(buffer))),
                on_blur: Some(Rc::new(move || blur.save())),
                on_caret_change: Some(Rc::new(move |state| caret.on_caret(state))),
                on_key_down: Some(Rc::new(move |event| keys.on_editor_key(event))),
                ..Default::default()
            });
        field.navigation = navigation;
        field.edit_request = edit;
        AnyView::new(
            full_size(
                AnyView::new(field)
                    .semantic_font(if novel {
                        novel_font(self.font_size, settings.line_spacing.factor())
                    } else {
                        editor_font(self.font_size)
                    })
                    .debug_name("editor"),
            )
            .id(("editor", document)),
        )
    }
}

impl EditorField {
    /// While the find bar is open, every match carries the same highlight the
    /// editor uses for repeated symbols.
    fn find_occurrences(&self) -> Vec<std::ops::Range<usize>> {
        if !self.model.find_open.get() {
            return Vec::new();
        }
        let _ = self.model.find_query.get();
        let buffer = self.model.buffer.get();
        self.model
            .find_matches()
            .into_iter()
            .map(|(low, high)| {
                buffer.utf8_offset_for_character_index(low)
                    ..buffer.utf8_offset_for_character_index(high)
            })
            .collect()
    }
}

/// Find and replace over the open document, above the writing surface.
struct FindBar {
    model: Model,
}

impl View for FindBar {
    fn body(&self, _cx: &Scope) -> AnyView {
        let model = &self.model;
        let _ = model.buffer.get();
        let query = model.find_query.get();
        let matches = model.find_matches();
        let count = matches.len();
        let index = model.find_index.get().min(count.saturating_sub(1));
        let label = if query.string_value().is_empty() {
            String::new()
        } else if count == 0 {
            "无结果".to_owned()
        } else {
            format!("{}/{}", index + 1, count)
        };
        let typing = model.clone();
        let submit = model.clone();
        let previous = model.clone();
        let next = model.clone();
        let replace = model.clone();
        let all = model.clone();
        let close = model.clone();
        let mut query_field =
            TextField::new("查找", model.find_query.binding()).events(TextFieldEvents {
                on_change: Some(Rc::new(move |buffer| typing.set_find_query(buffer))),
                on_submit: Some(Rc::new(move || submit.find_step(1))),
                ..Default::default()
            });
        query_field.navigation = TextFieldNavigationRequest {
            focus_request_id: Some(model.find_focus.get()),
            ..Default::default()
        };
        let query_field = query_field.frame(
            Some(FrameDimension::Points(200.0)),
            None,
            None,
            None,
            None,
            None,
        );
        let replacement_field = TextField::new("替换", model.find_replacement.binding()).frame(
            Some(FrameDimension::Points(160.0)),
            None,
            None,
            None,
            None,
            None,
        );
        AnyView::new(
            hstack(
                6.0,
                Align::Center,
                vec![
                    AnyView::new(query_field),
                    AnyView::new(
                        Text::new(label)
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::on_surface_muted()),
                    ),
                    icon_button(
                        icons::navigation::chevron_up(),
                        "上一个匹配",
                        false,
                        move || previous.find_step(-1),
                    ),
                    icon_button(
                        icons::navigation::chevron_down(),
                        "下一个匹配",
                        false,
                        move || next.find_step(1),
                    ),
                    AnyView::new(replacement_field),
                    AnyView::new(Button::title("替换", move || replace.replace_one())),
                    AnyView::new(Button::title("全部替换", move || all.replace_all())),
                    spacer(),
                    icon_button(icons::action::x(), "关闭查找", false, move || {
                        close.close_find()
                    }),
                ],
            )
            .padding_insets(cove_ui::compose::modifiers::layout::EdgeInsets {
                top: 2.0,
                leading: 10.0,
                bottom: 6.0,
                trailing: 10.0,
            }),
        )
    }
}

struct PreviewPane {
    model: Model,
    column: f32,
    horizontal: f32,
}

impl View for PreviewPane {
    fn body(&self, _cx: &Scope) -> AnyView {
        if self.model.writing_mode() == ruwriter_core::WritingMode::Novel {
            let settings = self.model.settings();
            return AnyView::new(
                full_size(AnyView::new(
                    TextField::new("纯文本预览", self.model.buffer.binding())
                        .style(AnyTextFieldStyle::new(ProseFieldStyle))
                        .layout(TextFieldLayout {
                            axis: TextFieldAxis::Vertical,
                            first_line_indent: if settings.paragraph_indent {
                                f32::from(settings.font_size) * 2.0
                            } else {
                                0.0
                            },
                            first_line_indent_skip: TextFieldIndentSkip::new(skip_prose_indent),
                            ..Default::default()
                        })
                        .decoration(TextFieldDecoration {
                            content_padding: Some(TextFieldContentPadding::new(
                                self.horizontal,
                                24.0,
                            )),
                            ..Default::default()
                        })
                        .behavior(TextFieldBehavior {
                            read_only: true,
                            ..Default::default()
                        })
                        .semantic_font(novel_font(
                            settings.font_size,
                            settings.line_spacing.factor(),
                        )),
                ))
                .id(("text-preview", self.model.document.get())),
            );
        }
        let text = self.model.preview.get();
        let page = AnyView::new(MarkdownView::new(text)).frame(
            Some(FrameDimension::Percent(100.0)),
            None,
            None,
            None,
            Some(self.column),
            None,
        );
        let centered = AnyView::new(BoxView::new(
            FlexDirection::Column,
            Align::Center,
            Justify::FlexStart,
            Wrap::NoWrap,
            0.0,
            AnyView::new(page),
        ))
        .padding_edges(MARGIN, 24.0);
        AnyView::new(ScrollView::new(ScrollAxis::Vertical, centered))
    }
}

/// Word count, selection size and save problems; nothing else competes
/// with the text.
struct StatusLine {
    model: Model,
    bottom_inset: f32,
}

impl View for StatusLine {
    fn body(&self, _cx: &Scope) -> AnyView {
        let stats = self.model.stats.get();
        let notice = self.model.notice.get();
        let caret = self.model.caret.get();
        let content: AnyView = if !notice.is_empty() {
            AnyView::new(Text::new(notice).semantic_foreground(SemanticColorRef::error()))
        } else {
            let mut parts: Vec<AnyView> = Vec::new();
            if let Some((low, high)) = caret.selected_range {
                let buffer = self.model.buffer.get_untracked();
                let len = buffer.character_count();
                let (low, high) = (low.min(len), high.min(len));
                if high > low {
                    let selected = high - low;
                    parts.push(AnyView::new(
                        Text::new(format!("已选 {selected} 字 · "))
                            .semantic_foreground(SemanticColorRef::accent()),
                    ));
                }
            }
            let plain = self.model.writing_mode() == ruwriter_core::WritingMode::Novel;
            let format = if plain { "TXT" } else { "Markdown" };
            let mut summary = format!("{format} · {} 字", stats.characters);
            // Chinese prose is measured in characters; the word split only
            // helps Latin text, so novel mode leaves it out.
            if stats.words != stats.characters && !plain {
                summary.push_str(&format!(" · {} 词", stats.words));
            }
            if stats.reading_minutes > 0 {
                summary.push_str(&format!(" · 约 {} 分钟", stats.reading_minutes));
            }
            parts.push(AnyView::new(
                Text::new(summary).semantic_foreground(SemanticColorRef::on_surface_muted()),
            ));
            AnyView::new(hstack(0.0, Align::Center, parts))
        };
        AnyView::new(
            hstack(
                0.0,
                Align::Center,
                vec![
                    spacer(),
                    AnyView::new(content.semantic_font(SemanticFontRef::caption())),
                ],
            )
            .padding_insets(cove_ui::compose::modifiers::layout::EdgeInsets {
                top: 8.0,
                leading: 16.0,
                bottom: 8.0 + self.bottom_inset,
                trailing: 16.0,
            }),
        )
    }
}
