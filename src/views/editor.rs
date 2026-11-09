//! The writing surface: a quiet toolbar, the editor as a centered readable
//! column that scrolls across the whole pane, an optional side-by-side
//! preview, and a one-line count at the bottom.

use std::rc::Rc;

use cove_ui::compose::modifiers::layout::{FlexExt, LayoutFrameExt, PaddingExt};
use cove_ui::compose::modifiers::style::SemanticStyleViewExt;
use cove_ui::compose::primitives::controls::{ButtonIcon, ButtonIconSource, MenuButton};
use cove_ui::compose::primitives::media::ui_icon_catalog as icons;
use cove_ui::compose::primitives::overlays::{MenuEntry, MenuItem, MenuItemRole};
use cove_ui::compose::primitives::text::text_field::{
    TextFieldCodeEditing, TextFieldContentPadding, TextFieldDecoration,
};
use cove_ui::compose::primitives::text::{
    Divider, MarkdownView, Text, TextField, TextFieldAxis, TextFieldEvents, TextFieldLayout,
};
use cove_ui::compose::theme::styles::components::text_field_style::AnyTextFieldStyle;
use cove_ui::compose::theme::{SemanticColorRef, SemanticFontRef};
use cove_ui::compose::{FrameDimension, ScrollAxis, ScrollView};
use cove_ui::composition::{AnyView, BoxView, EmptyView, LayoutDebugExt, Scope, View, ViewExt};
use cove_ui::core::scene::{Align, FlexDirection, Justify, Wrap};

use super::root::Layout;
use super::ui::{ProseFieldStyle, full_size, hstack, icon_button, spacer, vstack};
use crate::markdown_syntax;
use crate::model::Model;
use crate::theme::{Mode, SyntaxColors, editor_font, measure};

type Action = Rc<dyn Fn(&Model)>;

/// Narrowest side margin around the text column.
const MARGIN: f32 = 28.0;
/// Panes narrower than this show the preview instead of beside the editor.
const SPLIT_WIDTH: f32 = 880.0;

pub struct EditorPane {
    pub model: Model,
    pub layout: Layout,
    pub mode: Mode,
}

impl View for EditorPane {
    fn body(&self, _cx: &Scope) -> AnyView {
        let _ = self.model.revision.get();
        let options = self.model.options();
        let font_size = self.model.settings().font_size;
        let split = options.preview && self.layout.editor_width >= SPLIT_WIDTH;
        let pane_width = if split {
            (self.layout.editor_width - 1.0) / 2.0
        } else {
            self.layout.editor_width
        };
        let column = measure(font_size);
        let horizontal = ((pane_width - column) / 2.0).max(MARGIN);

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

        full_size(vstack(
            0.0,
            Align::Stretch,
            vec![
                self.toolbar(),
                AnyView::new(body.flex_with(1.0, 1.0, Some(0.0))),
                AnyView::new(StatusLine {
                    model: self.model.clone(),
                    bottom_inset: self.layout.bottom_inset,
                }),
            ],
        ))
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
        let mut trailing = vec![
            icon_button(
                icons::layout::columns_2(),
                "预览",
                options.preview,
                move || preview.toggle_preview(),
            ),
            icon_button(
                icons::action::focus(),
                "专注模式",
                options.focus,
                move || focus.toggle_focus(),
            ),
        ];
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
}

fn document_menu(model: &Model) -> Vec<MenuEntry> {
    let pinned = model.active_document().is_some_and(|d| d.pinned);
    let actions: [(&str, Action); 7] = [
        ("新建文稿", Rc::new(Model::create)),
        ("打开文件…", Rc::new(Model::open_picker)),
        ("导出 Markdown…", Rc::new(Model::export)),
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
    let shortcuts = ["⌘N", "⌘O", "⇧⌘E", "", "⌘=", "⌘-", "⌘K"];
    let mut entries: Vec<MenuEntry> = Vec::new();
    for (index, ((title, action), shortcut)) in actions.into_iter().zip(shortcuts).enumerate() {
        let target = model.clone();
        let mut item = MenuItem::new(index as u64 + 1, title, move || action(&target));
        if !shortcut.is_empty() {
            item = item.shortcut(shortcut);
        }
        entries.push(MenuEntry::Item(item));
        if matches!(index, 2 | 3 | 5) {
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
        let change = self.model.clone();
        let blur = self.model.clone();
        let colors = SyntaxColors::for_theme(&self.mode.theme());
        let field = TextField::new("开始写作…", self.model.buffer.binding())
            .layout(TextFieldLayout {
                axis: TextFieldAxis::Vertical,
                max_visible_lines: 400,
                wraps_lines: true,
                ..Default::default()
            })
            .decoration(TextFieldDecoration {
                content_padding: Some(TextFieldContentPadding::new(self.horizontal, 24.0)),
                ..Default::default()
            })
            .code_editing(TextFieldCodeEditing {
                syntax_color_at_utf8_offset: Some(markdown_syntax::highlighter(colors)),
                ..Default::default()
            })
            .style(AnyTextFieldStyle::new(ProseFieldStyle))
            .events(TextFieldEvents {
                on_change: Some(Rc::new(move |buffer| change.on_change(buffer))),
                on_blur: Some(Rc::new(move || blur.save())),
                ..Default::default()
            });
        AnyView::new(
            full_size(
                AnyView::new(field)
                    .semantic_font(editor_font(self.font_size))
                    .debug_name("editor"),
            )
            .id(("editor", document)),
        )
    }
}

struct PreviewPane {
    model: Model,
    column: f32,
}

impl View for PreviewPane {
    fn body(&self, _cx: &Scope) -> AnyView {
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

/// Word count and save problems; nothing else competes with the text.
struct StatusLine {
    model: Model,
    bottom_inset: f32,
}

impl View for StatusLine {
    fn body(&self, _cx: &Scope) -> AnyView {
        let stats = self.model.stats.get();
        let notice = self.model.notice.get();
        let text = if notice.is_empty() {
            let minutes = if stats.reading_minutes == 0 {
                String::new()
            } else {
                format!(" · 约 {} 分钟", stats.reading_minutes)
            };
            Text::new(format!("{} 字{minutes}", stats.characters))
                .semantic_foreground(SemanticColorRef::on_surface_muted())
        } else {
            Text::new(notice).semantic_foreground(SemanticColorRef::error())
        };
        AnyView::new(
            hstack(
                0.0,
                Align::Center,
                vec![
                    spacer(),
                    AnyView::new(text.semantic_font(SemanticFontRef::caption())),
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
