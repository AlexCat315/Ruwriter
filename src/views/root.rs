//! Window shell: library sidebar · document list · editor.
//!
//! The three panes always occupy the same slots of one row, so changing the
//! layout (window width, focus mode, a hidden sidebar) never rebuilds the
//! editor. Narrow windows show the list or the editor, one at a time.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cove_ui::compose::adaptive::SurfaceEnvironment;
use cove_ui::compose::modifiers::layout::{FlexExt, LayoutFrameExt};
use cove_ui::compose::modifiers::style::SemanticStyleViewExt;
use cove_ui::compose::primitives::overlays::ShortcutHost;
use cove_ui::compose::primitives::overlays::portal::LayerRoot;
use cove_ui::compose::primitives::text::{Divider, TextBuffer};
use cove_ui::compose::theme::SemanticColorRef;
use cove_ui::compose::theme::styles::circular_appearance::{CircularThemeHost, ThemeTransition};
use cove_ui::composition::{AnyView, EmptyView, Scope, View};
use cove_ui::core::scene::{Align, OpenedDocuments};
use ruwriter_core::{Session, count};

use super::editor::EditorPane;
use super::list::ListPane;
use super::palette::Palette;
use super::sidebar::LibrarySidebar;
use super::ui::{full_height, full_size, hstack, vstack};
use crate::menu_bar;
use crate::model::Model;
use crate::shortcuts;
use crate::theme::Mode;

pub const SIDEBAR_WIDTH: f32 = 216.0;
pub const LIST_WIDTH: f32 = 300.0;
/// Below this width one pane shows at a time.
pub const SINGLE_PANE_WIDTH: f32 = 640.0;
/// Below this width the sidebar folds into the list's title menu.
pub const SIDEBAR_MIN_WINDOW: f32 = 900.0;

/// Which panes are on screen and how wide the editor is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub sidebar: bool,
    pub list: bool,
    pub editor: bool,
    pub single_pane: bool,
    pub editor_width: f32,
    pub top_inset: f32,
    /// Home indicator or on-screen keyboard, whichever covers more.
    pub bottom_inset: f32,
}

impl Layout {
    pub fn resolve(width: f32, top_inset: f32, bottom_inset: f32, model: &Model) -> Self {
        let options = model.options();
        let single_pane = width < SINGLE_PANE_WIDTH;
        let showing_editor = model.showing_editor.get();
        let sidebar =
            !single_pane && !options.focus && options.sidebar && width >= SIDEBAR_MIN_WINDOW;
        let list = !options.focus && (!single_pane || !showing_editor);
        let editor = !single_pane || showing_editor || options.focus;
        let mut editor_width = width;
        if sidebar {
            editor_width -= SIDEBAR_WIDTH;
        }
        if list && editor {
            editor_width -= LIST_WIDTH + 1.0;
        }
        Self {
            sidebar,
            list,
            editor,
            single_pane,
            editor_width: editor_width.max(0.0),
            top_inset,
            bottom_inset,
        }
    }
}

pub struct RootView;

impl View for RootView {
    fn body(&self, cx: &Scope) -> AnyView {
        let session = cx.remember(|| Rc::new(RefCell::new(Session::load())));
        let initial = session.borrow().text();
        let model = Model {
            session: session.clone(),
            revision: cx.state(|| 0_u64),
            listing: cx.state(|| 0_u64),
            document: cx.state(|| 0_u64),
            buffer: cx.state(|| TextBuffer::new(&initial)),
            preview: cx.state(|| initial.clone()),
            stats: cx.state(|| count(&initial)),
            notice: cx.state(String::new),
            query: cx.state(TextBuffer::empty),
            palette: cx.state(|| false),
            showing_editor: cx.state(|| true),
        };

        // Documents the OS opens with Ruwriter (Finder "Open With", a file
        // association, command-line paths) land in the library once.
        let installed = cx.remember(|| Rc::new(Cell::new(false)));
        if !installed.replace(true) {
            let opener = model.clone();
            OpenedDocuments::set_handler(Some(Rc::new(move |paths: Vec<String>| {
                for path in paths {
                    opener.open_path(std::path::Path::new(&path));
                }
            })));
        }

        let _ = model.revision.get();
        let environment = cx.composition_value(&SurfaceEnvironment::key());
        let layout = Layout::resolve(
            environment.viewport.width,
            environment.safe_area.top,
            environment
                .safe_area
                .bottom
                .max(environment.keyboard.height),
            &model,
        );
        let settings = model.settings();
        let mode = Mode::resolve(settings.appearance, environment.system_appearance.is_dark());

        let menu_state = cx.remember(|| Rc::new(RefCell::new(None)));
        menu_bar::sync(&model, &menu_state);

        let sidebar: AnyView = if layout.sidebar {
            full_height(
                AnyView::new(LibrarySidebar {
                    model: model.clone(),
                    layout,
                })
                .frame_width(SIDEBAR_WIDTH),
            )
        } else {
            AnyView::new(EmptyView)
        };

        let list: AnyView = if layout.list {
            let pane = AnyView::new(ListPane {
                model: model.clone(),
                layout,
            })
            .semantic_background(SemanticColorRef::background());
            let pane = if layout.editor {
                AnyView::new(pane.frame_width(LIST_WIDTH))
            } else {
                AnyView::new(pane.flex_with(1.0, 1.0, Some(0.0)))
            };
            full_height(pane)
        } else {
            AnyView::new(EmptyView)
        };

        let divider: AnyView = if layout.list && layout.editor {
            full_height(Divider::vertical())
        } else {
            AnyView::new(EmptyView)
        };

        let editor: AnyView = if layout.editor {
            full_height(
                AnyView::new(EditorPane {
                    model: model.clone(),
                    layout,
                    mode,
                })
                .semantic_background(SemanticColorRef::background())
                .flex_with(1.0, 1.0, Some(0.0)),
            )
        } else {
            AnyView::new(EmptyView)
        };

        let shortcut_model = model.clone();
        let shortcut_host =
            ShortcutHost::new(move |event| shortcuts::handle(event, &shortcut_model));

        let shell = full_size(hstack(
            0.0,
            Align::Stretch,
            vec![sidebar, list, divider, editor],
        ));
        let content = full_size(vstack(
            0.0,
            Align::Stretch,
            vec![
                AnyView::new(shell),
                AnyView::new(shortcut_host),
                AnyView::new(Palette {
                    model: model.clone(),
                }),
            ],
        ));
        let root = LayerRoot::new(content);
        full_size(CircularThemeHost::new(
            mode.theme(),
            mode,
            ThemeTransition::circular(),
            AnyView::new(root),
        ))
    }
}
