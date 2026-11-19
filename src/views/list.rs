//! Document list: collection title, search, and one row per document with
//! its title, a two-line excerpt and when it was last edited.

use cove_ui::compose::modifiers::layout::{FlexExt, LayoutFrameExt, PaddingExt};
use cove_ui::compose::modifiers::style::SemanticStyleViewExt;
use cove_ui::compose::primitives::controls::fields::SearchField;
use cove_ui::compose::primitives::controls::{Button, ButtonIcon, ButtonIconSource, MenuButton};
use cove_ui::compose::primitives::media::Icon;
use cove_ui::compose::primitives::media::ui_icon_catalog as icons;
use cove_ui::compose::primitives::overlays::{
    ContextMenuExt, MenuEntry, MenuItem, MenuItemRole, MenuSubmenu,
};
use cove_ui::compose::primitives::text::Text;
use cove_ui::compose::theme::styles::buttons::ButtonStyleViewExt;
use cove_ui::compose::theme::{SemanticColorRef, SemanticFontRef};
use cove_ui::compose::{FrameDimension, ScrollAxis, ScrollView};
use cove_ui::composition::{AnyView, Scope, View, ViewExt};
use cove_ui::core::scene::Align;
use ruwriter_core::{Collection, Document, SortOrder};

use super::root::Layout;
use super::sidebar::appearance_label;
use super::ui::{RowButtonStyle, hstack, icon_button, spacer, vstack};
use crate::dates::relative;
use crate::model::{Model, RenameTarget};

pub struct ListPane {
    pub model: Model,
    pub layout: Layout,
}

impl View for ListPane {
    fn body(&self, _cx: &Scope) -> AnyView {
        let _ = self.model.revision.get();
        let header = self.header();
        let search = AnyView::new(
            AnyView::new(
                SearchField::new(self.model.query.binding())
                    .placeholder("搜索")
                    .clears_on_escape(true),
            )
            .padding_insets(cove_ui::compose::modifiers::layout::EdgeInsets {
                top: 0.0,
                leading: 12.0,
                bottom: 8.0,
                trailing: 12.0,
            }),
        );
        let rows = AnyView::new(
            AnyView::new(DocumentRows {
                model: self.model.clone(),
                bottom_inset: self.layout.bottom_inset,
            })
            .flex_with(1.0, 1.0, Some(0.0)),
        );
        vstack(0.0, Align::Stretch, vec![header, search, rows])
    }
}

impl ListPane {
    fn header(&self) -> AnyView {
        let model = &self.model;
        let settings = model.settings();
        let collection = settings.collection;
        let in_trash = settings.folder.is_none() && collection == Collection::Trash;
        let (heading_title, count) = model
            .read(|session| match settings.folder.as_deref() {
                Some(folder) => (
                    session
                        .library
                        .folder(folder)
                        .map(|folder| folder.name.clone())
                        .unwrap_or_else(|| model.library_name()),
                    session.library.folder_count(folder),
                ),
                None => (collection.title().to_owned(), session.count(collection)),
            })
            .unwrap_or_default();
        let title = Text::new(heading_title)
            .truncated()
            .semantic_font(SemanticFontRef::headline());
        let subtitle = Text::new(if count == 0 {
            "空".to_owned()
        } else {
            format!("{count} 篇")
        })
        .semantic_font(SemanticFontRef::caption())
        .semantic_foreground(SemanticColorRef::on_surface_muted());
        let titles = vstack(
            1.0,
            Align::FlexStart,
            vec![AnyView::new(title), AnyView::new(subtitle)],
        );

        // Without the sidebar the title opens the collections.
        let heading: AnyView = if self.layout.sidebar {
            titles
        } else {
            let label = hstack(
                4.0,
                Align::Center,
                vec![
                    titles,
                    AnyView::new(ButtonIcon::new(
                        ButtonIconSource::resource(icons::navigation::chevron_down()),
                        Some(14.0),
                        None,
                    )),
                ],
            );
            AnyView::new(
                MenuButton::new(collections_menu(model), label)
                    .ghost()
                    .accessibility_label("资料库"),
            )
        };

        let action: AnyView = if in_trash {
            let empty = model.clone();
            AnyView::new(
                Button::title("清空", move || empty.empty_trash())
                    .enabled(count > 0)
                    .role(cove_ui::compose::primitives::controls::ButtonRole::Destructive),
            )
        } else {
            let create = model.clone();
            icon_button(icons::action::pencil(), "新建文稿", false, move || {
                create.create()
            })
        };
        let sort = AnyView::new(
            MenuButton::new(
                sort_menu(model),
                ButtonIcon::new(
                    ButtonIconSource::resource(icons::navigation::chevrons_up_down()),
                    Some(16.0),
                    None,
                ),
            )
            .ghost()
            .accessibility_label("排序")
            .tooltip("排序"),
        );

        AnyView::new(
            hstack(8.0, Align::Center, vec![heading, spacer(), sort, action]).padding_insets(
                cove_ui::compose::modifiers::layout::EdgeInsets {
                    top: self.layout.top_inset + 12.0,
                    // The collection menu's ghost chrome brings its own inset.
                    leading: if self.layout.sidebar { 18.0 } else { 8.0 },
                    bottom: 10.0,
                    trailing: 12.0,
                },
            ),
        )
    }
}

fn collections_menu(model: &Model) -> Vec<MenuEntry> {
    let current = model.settings().collection;
    let mut entries: Vec<MenuEntry> = Collection::ALL
        .iter()
        .enumerate()
        .map(|(index, collection)| {
            let choose = model.clone();
            let collection = *collection;
            MenuEntry::Item(
                MenuItem::new(index as u64 + 1, collection.title(), move || {
                    choose.set_collection(collection)
                })
                .selected(collection == current),
            )
        })
        .collect();
    let folders = model
        .read(|session| session.library.folders.clone())
        .unwrap_or_default();
    if !folders.is_empty() {
        entries.push(MenuEntry::separator(80));
        entries.push(MenuEntry::label(81, model.library_name()));
        let selected = model.settings().folder;
        for (index, folder) in folders.into_iter().enumerate() {
            let choose = model.clone();
            let indent = "\u{3000}".repeat(folder.depth);
            let is_selected = selected.as_deref() == Some(folder.id.as_str());
            entries.push(MenuEntry::Item(
                MenuItem::new(
                    1000 + index as u64,
                    format!("{indent}{}", folder.name),
                    move || choose.select_folder(&folder.id),
                )
                .selected(is_selected),
            ));
        }
    }
    entries.push(MenuEntry::separator(90));
    let (title, _) = appearance_label(model.settings().appearance);
    let cycle = model.clone();
    entries.push(MenuEntry::Item(MenuItem::new(
        91,
        format!("外观：{title}"),
        move || cycle.cycle_appearance(),
    )));
    entries
}

fn sort_menu(model: &Model) -> Vec<MenuEntry> {
    let current = model.settings().sort;
    SortOrder::ALL
        .iter()
        .enumerate()
        .map(|(index, order)| {
            let choose = model.clone();
            let order = *order;
            MenuEntry::Item(
                MenuItem::new(index as u64 + 1, order.title(), move || {
                    choose.set_sort(order)
                })
                .selected(order == current),
            )
        })
        .collect()
}

/// The rows alone, so typing refreshes them without rebuilding the header
/// and the search field.
struct DocumentRows {
    model: Model,
    bottom_inset: f32,
}

impl View for DocumentRows {
    fn body(&self, _cx: &Scope) -> AnyView {
        let _ = self.model.revision.get();
        let _ = self.model.listing.get();
        let query = self.model.query.get().string_value();
        let (documents, active, settings) = self
            .model
            .read(|session| {
                (
                    session.list(&query),
                    session.active_id(),
                    session.settings(),
                )
            })
            .unwrap_or_default();
        let collection = settings.collection;
        if documents.is_empty() {
            return empty_state(
                &self.model,
                collection,
                settings.folder.is_some(),
                !query.trim().is_empty(),
            );
        }
        let show_folder = settings.folder.is_none();
        let rows: Vec<AnyView> = documents
            .into_iter()
            .map(|document| {
                let selected = active.as_deref() == Some(document.id.as_str());
                row(&self.model, document, selected, collection, show_folder)
            })
            .collect();
        AnyView::new(ScrollView::new(
            ScrollAxis::Vertical,
            vstack(2.0, Align::Stretch, rows).padding_insets(
                cove_ui::compose::modifiers::layout::EdgeInsets {
                    top: 4.0,
                    leading: 8.0,
                    bottom: 12.0 + self.bottom_inset,
                    trailing: 8.0,
                },
            ),
        ))
    }
}

fn empty_state(model: &Model, collection: Collection, in_folder: bool, searching: bool) -> AnyView {
    let (icon, message) = if searching {
        (icons::action::search(), "没有匹配的文稿")
    } else if in_folder {
        (icons::file::folder_open(), "文件夹是空的")
    } else {
        match collection {
            Collection::Trash => (icons::action::trash(), "废纸篓是空的"),
            Collection::Pinned => (icons::shape::pin(), "右键文稿即可置顶"),
            _ => (icons::file::file_text(), "还没有文稿"),
        }
    };
    let mut children = vec![
        AnyView::new(Icon::with_semantic(
            icon,
            30.0,
            SemanticColorRef::on_surface_muted(),
        )),
        AnyView::new(
            Text::new(message)
                .semantic_font(SemanticFontRef::body())
                .semantic_foreground(SemanticColorRef::on_surface_muted()),
        ),
    ];
    if !searching && !matches!(collection, Collection::Trash | Collection::Pinned) {
        let create = model.clone();
        children.push(AnyView::new(Button::title("新建文稿", move || {
            create.create()
        })));
    }
    AnyView::new(
        AnyView::new(vstack(12.0, Align::Center, children))
            .frame(
                Some(FrameDimension::Percent(100.0)),
                None,
                None,
                None,
                None,
                None,
            )
            .padding_insets(cove_ui::compose::modifiers::layout::EdgeInsets {
                top: 64.0,
                leading: 18.0,
                bottom: 24.0,
                trailing: 18.0,
            }),
    )
}

fn row(
    model: &Model,
    document: Document,
    selected: bool,
    collection: Collection,
    show_folder: bool,
) -> AnyView {
    let mut title_line = vec![AnyView::new(
        AnyView::new(
            Text::new(document.display_title())
                .truncated()
                .semantic_font(SemanticFontRef::body_strong()),
        )
        .flex_with(1.0, 1.0, Some(0.0)),
    )];
    if document.pinned && collection != Collection::Trash {
        title_line.push(AnyView::new(ButtonIcon::new(
            ButtonIconSource::resource(icons::shape::pin()),
            Some(12.0),
            None,
        )));
    }
    let mut lines = vec![hstack(6.0, Align::Center, title_line)];
    if !document.summary().is_empty() {
        lines.push(AnyView::new(
            Text::new(document.summary().to_owned())
                .line_limit(Some(2))
                .semantic_font(SemanticFontRef::caption())
                .semantic_foreground(SemanticColorRef::on_surface_variant()),
        ));
    }
    let folder_name = if show_folder && !document.trashed {
        document.folder.as_deref().and_then(|id| {
            model
                .read(|session| session.library.folder(id).map(|folder| folder.name.clone()))
                .flatten()
        })
    } else {
        None
    };
    let mut meta = String::new();
    if !document.trashed {
        meta.push_str(document.status.title());
        if !document.tags.is_empty() {
            meta.push_str(" · ");
            for (index, tag) in document.tags.iter().enumerate() {
                if index > 0 {
                    meta.push(' ');
                }
                meta.push('#');
                meta.push_str(tag);
            }
        }
        meta.push_str(" · ");
    }
    if let Some(name) = folder_name {
        meta.push_str(&name);
        meta.push_str(" · ");
    }
    meta.push_str(&format!(
        "{} · {} 字",
        relative(document.updated_at),
        document.words
    ));
    lines.push(AnyView::new(
        Text::new(meta)
            .semantic_font(SemanticFontRef::caption())
            .semantic_foreground(SemanticColorRef::on_surface_muted()),
    ));
    let content = AnyView::new(vstack(4.0, Align::Stretch, lines).frame(
        Some(FrameDimension::Percent(100.0)),
        None,
        None,
        None,
        None,
        None,
    ));

    let id = document.id.clone();
    let open = model.clone();
    let button = AnyView::new(
        Button::new(content, move || open.select(&id))
            .selected(selected)
            .accessibility_text(document.display_title()),
    )
    .button_style(RowButtonStyle);
    let entries = row_menu(model, &document, collection);
    AnyView::new(AnyView::new(button.context_menu(entries)).id(format!("row-{}", document.id)))
}

fn row_menu(model: &Model, document: &Document, collection: Collection) -> Vec<MenuEntry> {
    let _ = collection;
    let id = document.id.clone();
    if document.trashed {
        let restore = model.clone();
        let purge = model.clone();
        let restore_id = id.clone();
        return vec![
            MenuEntry::Item(MenuItem::new(1, "恢复", move || {
                restore.restore(&restore_id)
            })),
            MenuEntry::separator(2),
            MenuEntry::Item(
                MenuItem::new(3, "永久删除", move || purge.purge(&id))
                    .role(MenuItemRole::Destructive),
            ),
        ];
    }
    let pin = model.clone();
    let info = model.clone();
    let open = model.clone();
    let export = model.clone();
    let trash = model.clone();
    let pinned = document.pinned;
    let pin_id = id.clone();
    let info_id = id.clone();
    let open_id = id.clone();
    let export_id = id.clone();
    let mut entries = vec![
        MenuEntry::Item(MenuItem::new(1, "打开", move || open.select(&open_id))),
        MenuEntry::Item(MenuItem::new(
            2,
            if pinned { "取消置顶" } else { "置顶" },
            move || pin.set_pinned(&pin_id, !pinned),
        )),
        MenuEntry::Item(MenuItem::new(9, "文稿信息…", move || {
            info.begin_edit_info(&info_id)
        })),
        MenuEntry::Item(MenuItem::new(3, "导出…", move || {
            export.select(&export_id);
            export.export();
        })),
        MenuEntry::separator(4),
    ];
    let rename = model.clone();
    let rename_id = id.clone();
    entries.push(MenuEntry::Item(MenuItem::new(
        6,
        "重命名…",
        move || rename.begin_rename(RenameTarget::Document(rename_id.clone())),
    )));
    if !document.external {
        entries.push(MenuEntry::Submenu(MenuSubmenu::new(
            7,
            "移动到",
            move_targets(model, &id, document.folder.as_deref()),
        )));
    }
    entries.push(MenuEntry::separator(8));
    entries.push(MenuEntry::Item(
        MenuItem::new(
            5,
            if document.external {
                "从列表中移除"
            } else {
                "移到废纸篓"
            },
            move || trash.trash(&id),
        )
        .role(MenuItemRole::Destructive),
    ));
    entries
}

/// The library folder and every folder in it, indented by depth; the
/// document's own folder is checked and does nothing.
fn move_targets(model: &Model, id: &str, current: Option<&str>) -> Vec<MenuEntry> {
    let folders = model
        .read(|session| session.library.folders.clone())
        .unwrap_or_default();
    let mut targets = vec![(String::new(), model.library_name(), 0)];
    targets.extend(folders.into_iter().map(|f| (f.id, f.name, f.depth + 1)));
    targets
        .into_iter()
        .enumerate()
        .map(|(index, (folder, name, depth))| {
            let mover = model.clone();
            let id = id.to_owned();
            let here = current == Some(folder.as_str());
            MenuEntry::Item(
                MenuItem::new(
                    100 + index as u64,
                    format!("{}{name}", "\u{3000}".repeat(depth)),
                    move || mover.move_document(&id, &folder),
                )
                .selected(here)
                .enabled(!here),
            )
        })
        .collect()
}
