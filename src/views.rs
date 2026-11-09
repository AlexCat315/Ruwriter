//! View tree. Components are split so that keystroke-driven signals only
//! recompose the panes that display them.

use std::cell::RefCell;
use std::rc::Rc;

use cove_ui::compose::modifiers::layout::{FlexExt, LayoutFrameExt, PaddingExt};
use cove_ui::compose::modifiers::style::SemanticStyleViewExt;
use cove_ui::compose::primitives::controls::Button;
use cove_ui::compose::primitives::layout::{SafeArea, SafeAreaEdges};
use cove_ui::compose::primitives::overlays::ShortcutHost;
use cove_ui::compose::primitives::text::{
    MarkdownView, Text, TextAlignment, TextBuffer, TextField, TextFieldAxis, TextFieldEvents,
    TextFieldLayout,
};
use cove_ui::compose::theme::styles::components::{AnyTextFieldStyle, EditorTextFieldStyle};
use cove_ui::compose::theme::{SemanticColorRef, SemanticFontRef};
use cove_ui::compose::{FrameDimension, ScrollAxis, ScrollView};
use cove_ui::composition::{AnyView, BoxView, EmptyView, Scope, TupleView, View, views};
use cove_ui::core::Signal;
use cove_ui::core::scene::{Align, FlexDirection, Justify, Wrap};
use ruwriter_core::{Session, count};

use crate::actions::Actions;
use crate::shortcuts;

fn stack(direction: FlexDirection, spacing: f32, content: impl View + 'static) -> AnyView {
    AnyView::new(BoxView::new(
        direction,
        Align::Stretch,
        Justify::FlexStart,
        Wrap::NoWrap,
        spacing,
        AnyView::new(content),
    ))
}

pub struct RootView;

impl View for RootView {
    fn body(&self, cx: &Scope) -> AnyView {
        let session = cx.remember(|| Rc::new(RefCell::new(Session::load())));
        let revision = cx.state(|| 0_u64);
        let initial = session.borrow().text();
        let buffer = cx.state(|| TextBuffer::new(&initial));
        let preview = cx.state(|| initial.clone());
        let stats = cx.state(|| count(&initial));
        let notice = cx.state(String::new);

        let actions = Actions {
            session: session.clone(),
            revision,
            buffer,
            preview,
            stats,
            notice,
        };
        let shortcut_actions = actions.clone();
        let shortcut_host =
            ShortcutHost::new(move |event| shortcuts::handle(event, &shortcut_actions));

        let root = stack(
            FlexDirection::Column,
            0.0,
            views!(
                AnyView::new(Toolbar {
                    actions: actions.clone()
                })
                .semantic_background(SemanticColorRef::surface())
                .padding_edges(18.0, 10.0),
                AnyView::new(Workspace {
                    actions: actions.clone()
                })
                .flex(1.0),
                AnyView::new(StatusBar {
                    actions: actions.clone()
                })
                .semantic_background(SemanticColorRef::surface())
                .padding_edges(18.0, 6.0),
                shortcut_host,
            ),
        )
        .semantic_background(SemanticColorRef::background())
        .frame(
            Some(FrameDimension::Percent(100.0)),
            Some(FrameDimension::Percent(100.0)),
            None,
            None,
            None,
            None,
        );
        AnyView::new(SafeArea::new(SafeAreaEdges::ALL, root))
    }
}

struct Toolbar {
    actions: Actions,
}

impl View for Toolbar {
    fn body(&self, _cx: &Scope) -> AnyView {
        let _ = self.actions.revision.get();
        let (title, options) = {
            let session = self.actions.session.borrow();
            (session.title(), session.options)
        };
        let create = self.actions.clone();
        let open = self.actions.clone();
        let save = self.actions.clone();
        let preview = self.actions.clone();
        let focus = self.actions.clone();

        stack(
            FlexDirection::Row,
            8.0,
            views!(
                Text::new(title).semantic_font(SemanticFontRef::body_strong()),
                AnyView::new(Text::new("")).flex(1.0),
                Button::title("新建", move || create.create()),
                Button::title("打开", move || open.open_picker()),
                Button::title("保存", move || save.save()),
                Button::title(
                    if options.preview { "写作" } else { "预览" },
                    move || preview.toggle_preview()
                ),
                Button::title(
                    if options.focus { "资料库" } else { "专注" },
                    move || focus.toggle_focus()
                ),
            ),
        )
    }
}

struct Workspace {
    actions: Actions,
}

impl View for Workspace {
    fn body(&self, _cx: &Scope) -> AnyView {
        let _ = self.actions.revision.get();
        let options = self.actions.session.borrow().options;
        let sidebar: AnyView = if options.focus {
            AnyView::new(AnyView::new(EmptyView).frame_width(0.0))
        } else {
            AnyView::new(
                AnyView::new(LibraryPane {
                    actions: self.actions.clone(),
                })
                .semantic_background(SemanticColorRef::surface())
                .frame_width(260.0),
            )
        };
        let editor = AnyView::new(EditorPane {
            actions: self.actions.clone(),
        })
        .flex(1.0);
        stack(FlexDirection::Row, 0.0, views!(sidebar, editor))
    }
}

struct LibraryPane {
    actions: Actions,
}

impl View for LibraryPane {
    fn body(&self, _cx: &Scope) -> AnyView {
        let _ = self.actions.revision.get();
        let (documents, active) = {
            let session = self.actions.session.borrow();
            (session.documents(), session.active_id())
        };
        let mut rows: Vec<AnyView> = Vec::with_capacity(documents.len());
        for document in documents {
            let id = document.id.clone();
            let actions = self.actions.clone();
            let selected = active.as_deref() == Some(id.as_str());
            let label = if selected {
                format!("• {}", document.display_title())
            } else {
                document.display_title().to_owned()
            };
            let label_view = AnyView::new(Text::new(label).alignment(TextAlignment::Leading))
                .frame(
                    Some(FrameDimension::Percent(100.0)),
                    None,
                    None,
                    None,
                    None,
                    None,
                );
            rows.push(AnyView::new(Button::new(label_view, move || {
                actions.select(&id)
            })));
        }
        AnyView::new(
            AnyView::new(ScrollView::new(
                ScrollAxis::Vertical,
                stack(FlexDirection::Column, 4.0, TupleView::new(rows)),
            ))
            .padding(12.0),
        )
    }
}

struct EditorPane {
    actions: Actions,
}

impl View for EditorPane {
    fn body(&self, _cx: &Scope) -> AnyView {
        let _ = self.actions.revision.get();
        let preview = self.actions.session.borrow().options.preview;
        let content: AnyView = if preview {
            AnyView::new(MarkdownPane {
                text: self.actions.preview,
            })
        } else {
            AnyView::new(EditorField {
                actions: self.actions.clone(),
            })
        };
        AnyView::new(content.padding_edges(32.0, 24.0))
    }
}

struct EditorField {
    actions: Actions,
}

impl View for EditorField {
    fn body(&self, _cx: &Scope) -> AnyView {
        let change = self.actions.clone();
        let blur = self.actions.clone();
        let field = TextField::new("开始写作", self.actions.buffer.binding())
            .layout(TextFieldLayout {
                axis: TextFieldAxis::Vertical,
                max_visible_lines: 32,
                wraps_lines: true,
                ..Default::default()
            })
            .style(AnyTextFieldStyle::new(EditorTextFieldStyle))
            .events(TextFieldEvents {
                on_change: Some(Rc::new(move |buffer| change.on_change(buffer))),
                on_blur: Some(Rc::new(move || blur.save())),
                ..Default::default()
            });
        AnyView::new(
            AnyView::new(field)
                .semantic_font(SemanticFontRef::body())
                .frame(
                    None,
                    Some(FrameDimension::Percent(100.0)),
                    None,
                    None,
                    None,
                    None,
                ),
        )
    }
}

struct MarkdownPane {
    text: Signal<String>,
}

impl View for MarkdownPane {
    fn body(&self, _cx: &Scope) -> AnyView {
        AnyView::new(ScrollView::new(
            ScrollAxis::Vertical,
            MarkdownView::new(self.text.get()),
        ))
    }
}

struct StatusBar {
    actions: Actions,
}

impl View for StatusBar {
    fn body(&self, _cx: &Scope) -> AnyView {
        let _ = self.actions.revision.get();
        let stats = self.actions.stats.get();
        let notice = self.actions.notice.get();
        let label = if notice.is_empty() {
            format!(
                "{} 字 · {} 词 · {} 行 · 约 {} 分钟",
                stats.characters, stats.words, stats.lines, stats.reading_minutes
            )
        } else {
            notice
        };
        stack(
            FlexDirection::Row,
            0.0,
            Text::new(label).semantic_font(SemanticFontRef::caption()),
        )
    }
}
