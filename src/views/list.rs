//! Document list: collection title, search, and one row per document with
//! its title, a two-line excerpt and when it was last edited.

use cove_ui::compose::modifiers::layout::{FlexExt, LayoutFrameExt, PaddingExt};
use cove_ui::compose::modifiers::style::SemanticStyleViewExt;
use cove_ui::compose::primitives::controls::fields::SearchField;
use cove_ui::compose::primitives::controls::{Button, ButtonIcon, ButtonIconSource, MenuButton};
use cove_ui::compose::primitives::media::ui_icon_catalog as icons;
use cove_ui::compose::primitives::overlays::{ContextMenuExt, MenuEntry, MenuItem, MenuItemRole};
use cove_ui::compose::primitives::text::Text;
use cove_ui::compose::theme::styles::buttons::ButtonStyleViewExt;
use cove_ui::compose::theme::{SemanticColorRef, SemanticFontRef};
use cove_ui::compose::{FrameDimension, ScrollAxis, ScrollView};
use cove_ui::composition::{AnyView, Scope, View, ViewExt};
use cove_ui::core::scene::Align;
use ruwriter_core::{Collection, Document};

use super::root::Layout;
use super::sidebar::appearance_label;
use super::ui::{RowButtonStyle, hstack, icon_button, spacer, vstack};
use crate::dates::relative;
use crate::model::Model;

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
        let collection = model.settings().collection;
        let count = model.read(|session| session.count(collection)).unwrap_or(0);
        let title = Text::new(collection.title())
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

        let action: AnyView = if collection == Collection::Trash {
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

        AnyView::new(
            hstack(8.0, Align::Center, vec![heading, spacer(), action]).padding_insets(
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
        let (documents, active, collection) = self
            .model
            .read(|session| {
                (
                    session.list(&query),
                    session.active_id(),
                    session.settings().collection,
                )
            })
            .unwrap_or_default();
        if documents.is_empty() {
            return empty_state(collection, !query.trim().is_empty());
        }
        let rows: Vec<AnyView> = documents
            .into_iter()
            .map(|document| {
                let selected = active.as_deref() == Some(document.id.as_str());
                row(&self.model, document, selected, collection)
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

fn empty_state(collection: Collection, searching: bool) -> AnyView {
    let message = if searching {
        "没有匹配的文稿"
    } else {
        match collection {
            Collection::Trash => "废纸篓是空的",
            Collection::Pinned => "右键文稿即可置顶",
            _ => "还没有文稿",
        }
    };
    AnyView::new(
        AnyView::new(
            Text::new(message)
                .semantic_font(SemanticFontRef::caption())
                .semantic_foreground(SemanticColorRef::on_surface_muted()),
        )
        .padding_edges(18.0, 24.0),
    )
}

fn row(model: &Model, document: Document, selected: bool, collection: Collection) -> AnyView {
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
    if !document.excerpt.is_empty() {
        lines.push(AnyView::new(
            Text::new(document.excerpt.clone())
                .line_limit(Some(2))
                .semantic_font(SemanticFontRef::caption())
                .semantic_foreground(SemanticColorRef::on_surface_variant()),
        ));
    }
    let meta = format!("{} · {} 字", relative(document.updated_at), document.words);
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
    let id = document.id.clone();
    if collection == Collection::Trash {
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
    let open = model.clone();
    let export = model.clone();
    let trash = model.clone();
    let pinned = document.pinned;
    let pin_id = id.clone();
    let open_id = id.clone();
    let export_id = id.clone();
    vec![
        MenuEntry::Item(MenuItem::new(1, "打开", move || open.select(&open_id))),
        MenuEntry::Item(MenuItem::new(
            2,
            if pinned { "取消置顶" } else { "置顶" },
            move || pin.set_pinned(&pin_id, !pinned),
        )),
        MenuEntry::Item(MenuItem::new(3, "导出…", move || {
            export.select(&export_id);
            export.export();
        })),
        MenuEntry::separator(4),
        MenuEntry::Item(
            MenuItem::new(5, "移到废纸篓", move || trash.trash(&id))
                .role(MenuItemRole::Destructive),
        ),
    ]
}
