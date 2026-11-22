//! Window shell: library sidebar · document list · editor.
//!
//! The three panes always occupy the same slots of one row, so changing the
//! layout (window width, focus mode, a hidden sidebar) never rebuilds the
//! editor. Narrow windows show the list or the editor, one at a time.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cove_ui::compose::FrameDimension;
use cove_ui::compose::adaptive::SurfaceEnvironment;
use cove_ui::compose::modifiers::layout::{FlexExt, LayoutFrameExt};
use cove_ui::compose::modifiers::style::{AppearanceExt, SemanticStyleViewExt};
use cove_ui::compose::primitives::overlays::ShortcutHost;
use cove_ui::compose::primitives::overlays::portal::LayerRoot;
use cove_ui::compose::primitives::text::{Divider, TextBuffer};
use cove_ui::compose::theme::SemanticColorRef;
use cove_ui::compose::theme::styles::circular_appearance::{CircularThemeHost, ThemeTransition};
use cove_ui::composition::{AnyView, EmptyView, Scope, View};
use cove_ui::core::ThreadPoster;
use cove_ui::core::scene::{Align, OpenedDocuments};
use cove_ui::watch::{DirectoryWatcher, WatchOptions};
use ruwriter_core::{Session, count};

use super::ai::AiPanel;
use super::editor::EditorPane;
use super::format_text::FormatTextDialog;
use super::history::HistoryDialog;
use super::info::InfoDialog;
use super::list::ListPane;
use super::outline::OutlinePane;
use super::palette::Palette;
use super::rename::RenameDialog;
use super::sidebar::LibrarySidebar;
use super::sync::SyncDialog;
use super::ui::{full_height, full_size, hstack, vstack};
use crate::menu_bar;
use crate::model::{Model, deliver_disk_change};
use crate::shortcuts;
use crate::theme::Mode;

pub const SIDEBAR_WIDTH: f32 = 216.0;
pub const LIST_WIDTH: f32 = 300.0;
pub const OUTLINE_WIDTH: f32 = 240.0;
pub const AI_PANEL_WIDTH: f32 = 416.0;
pub const AI_DOCK_MIN_WIDTH: f32 = 820.0;
/// Below this width one pane shows at a time.
pub const SINGLE_PANE_WIDTH: f32 = 640.0;
/// Below this width the sidebar folds into the list's title menu.
pub const SIDEBAR_MIN_WINDOW: f32 = 900.0;
/// Below this width the outline collapses so the page keeps a readable column.
pub const OUTLINE_MIN_WINDOW: f32 = 1180.0;

/// Which panes are on screen and how wide the editor is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub sidebar: bool,
    pub list: bool,
    pub editor: bool,
    pub outline: bool,
    pub ai_docked: bool,
    pub ai_full_width: bool,
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
        let ai_open = model.ai.open.get();
        let ai_docked = width >= AI_DOCK_MIN_WIDTH && ai_open;
        let ai_full_width = ai_open && !ai_docked;
        let sidebar = !ai_full_width
            && !single_pane
            && !options.focus
            && options.sidebar
            && width
                >= if ai_docked {
                    1330.0
                } else {
                    SIDEBAR_MIN_WINDOW
                };
        let list = !ai_full_width
            && !options.focus
            && (!single_pane || !showing_editor)
            && (!ai_docked || width >= 1100.0);
        let editor = !single_pane || showing_editor || options.focus;
        let outline = editor
            && !single_pane
            && !options.focus
            && options.outline
            && width >= OUTLINE_MIN_WINDOW
            && !ai_open;
        let mut editor_width = if ai_full_width { 0.0 } else { width };
        if sidebar {
            editor_width -= SIDEBAR_WIDTH;
        }
        if list && editor {
            editor_width -= LIST_WIDTH + 1.0;
        }
        if outline {
            editor_width -= OUTLINE_WIDTH + 1.0;
        }
        if ai_docked {
            editor_width -= AI_PANEL_WIDTH + 1.0;
        }
        Self {
            sidebar,
            list,
            editor,
            outline,
            ai_docked,
            ai_full_width,
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
            rename: cx.state(|| None),
            rename_text: cx.state(TextBuffer::empty),
            caret: cx.state(|| {
                cove_ui::compose::primitives::text::text_field::TextFieldCaretState::INITIAL
            }),
            navigation: cx.state(
                cove_ui::compose::primitives::text::text_field::TextFieldNavigationRequest::default,
            ),
            edit: cx.state(|| None),
            format_dialog: cx.state(|| None),
            find_open: cx.state(|| false),
            find_query: cx.state(cove_ui::compose::primitives::text::TextBuffer::empty),
            find_replacement: cx.state(cove_ui::compose::primitives::text::TextBuffer::empty),
            find_index: cx.state(|| 0),
            find_focus: cx.state(|| 0),
            info_document: cx.state(|| None),
            info_synopsis: cx.state(TextBuffer::empty),
            info_tags: cx.state(TextBuffer::empty),
            info_status: cx.state(ruwriter_core::DocStatus::default),
            settings_revision: cx.state(|| 0),
            pending_save: cx.state(|| None),
            save_revision: cx.state(|| 0),
            caret_revision: cx.state(|| 0),
            outline: cx.state(|| {
                let buffer = TextBuffer::new(&initial);
                Rc::new(crate::formatting::editor_headings(&initial, &buffer))
            }),
            history_open: cx.state(|| false),
            history_entries: cx.state(Vec::new),
            sync_open: cx.state(|| false),
            sync_login: cx.state(|| "GitHub CLI：等待检查".to_owned()),
            sync_checking: cx.state(|| false),
            sync_library: cx.state(|| None),
            sync_status: cx.state(String::new),
            sync_busy: cx.state(|| false),
            sync_failed: cx.state(|| false),
            sync_local: cx.state(|| Rc::new(Vec::new())),
            sync_remote: cx.state(|| None),
            sync_remote_loaded: cx.state(|| false),
            ai: crate::ai::AiState::new(cx, session.borrow().ai_preferences()),
        };

        // Documents the OS opens with Ruwriter (Finder "Open With", a file
        // association, command-line paths) land in the library once.
        let installed = cx.remember(|| Rc::new(Cell::new(false)));
        if !installed.replace(true) {
            model.install_disk_handler();
            model.install_sync_handler();
            model.restore_caret();
            let opener = model.clone();
            OpenedDocuments::set_handler(Some(Rc::new(move |paths: Vec<String>| {
                for path in paths {
                    opener.open_path(std::path::Path::new(&path));
                }
            })));
        }

        let _ = model.revision.get();

        // One watcher per library folder; opening another folder replaces it.
        let watcher = cx.remember(|| Rc::new(RefCell::new(None::<DirectoryWatcher>)));
        let library_root = session.borrow().library_root().to_path_buf();
        let stale = watcher
            .borrow()
            .as_ref()
            .is_none_or(|current| current.root() != library_root);
        if stale {
            let poster = ThreadPoster::current();
            *watcher.borrow_mut() =
                DirectoryWatcher::new(&library_root, WatchOptions::default(), move |paths| {
                    poster.post(move || deliver_disk_change(paths))
                })
                .ok();
        }

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
                .frame(
                    layout.ai_full_width.then_some(FrameDimension::Points(0.0)),
                    None,
                    Some(0.0),
                    None,
                    None,
                    None,
                )
                .clipped()
                .flex_with(
                    if layout.ai_full_width { 0.0 } else { 1.0 },
                    1.0,
                    Some(0.0),
                ),
            )
        } else {
            AnyView::new(EmptyView)
        };

        let outline_divider: AnyView = if layout.outline || layout.ai_docked {
            full_height(Divider::vertical())
        } else {
            AnyView::new(EmptyView)
        };

        let outline: AnyView = if layout.ai_docked || layout.ai_full_width {
            full_height(
                AnyView::new(AiPanel {
                    model: model.clone(),
                    docked: layout.ai_docked,
                    top_inset: layout.top_inset,
                    bottom_inset: layout.bottom_inset,
                })
                .frame_width(if layout.ai_full_width {
                    environment.viewport.width
                } else {
                    AI_PANEL_WIDTH
                })
                .semantic_background(SemanticColorRef::surface()),
            )
        } else if layout.outline {
            full_height(
                AnyView::new(OutlinePane {
                    model: model.clone(),
                    top_inset: layout.top_inset,
                    bottom_inset: layout.bottom_inset,
                })
                .frame_width(OUTLINE_WIDTH)
                .semantic_background(SemanticColorRef::background()),
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
            vec![sidebar, list, divider, editor, outline_divider, outline],
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
                AnyView::new(RenameDialog {
                    model: model.clone(),
                }),
                AnyView::new(FormatTextDialog {
                    model: model.clone(),
                }),
                AnyView::new(InfoDialog {
                    model: model.clone(),
                }),
                AnyView::new(HistoryDialog {
                    model: model.clone(),
                }),
                AnyView::new(SyncDialog {
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
