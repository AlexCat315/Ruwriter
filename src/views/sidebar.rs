//! Library navigation: the collections, the library's folder tree (drag a
//! folder onto, before or after another to move it), and the appearance
//! switch.

use std::collections::HashSet;
use std::rc::Rc;

use cove_ui::compose::modifiers::layout::{FlexExt, LayoutFrameExt, PaddingExt};
use cove_ui::compose::modifiers::style::SemanticStyleViewExt;
use cove_ui::compose::primitives::controls::{Button, ButtonIcon, ButtonIconSource};
use cove_ui::compose::primitives::data::lists::tree::{
    TreeDragOperations, TreeDropPosition, TreeEvents, TreeLayout, TreeOptions, TreeSelection,
    TreeSlots,
};
use cove_ui::compose::primitives::data::lists::{RowState, Selection, Tree};
use cove_ui::compose::primitives::layout::sidebar::{
    Sidebar, SidebarItem, SidebarItemStyle, SidebarPresentation, SidebarSection,
};
use cove_ui::compose::primitives::media::Icon;
use cove_ui::compose::primitives::media::ui_icon_catalog as icons;
use cove_ui::compose::primitives::overlays::{
    ContextMenuExt, MenuEntry, MenuItem, MenuItemRole,
};
use cove_ui::compose::primitives::text::Text;
use cove_ui::compose::theme::styles::buttons::ButtonStyleViewExt;
use cove_ui::compose::theme::{SemanticColorRef, SemanticFontRef};
use cove_ui::compose::FrameDimension;
use cove_ui::composition::{AnyView, LayoutDebugExt, Scope, View};
use cove_ui::core::Binding;
use cove_ui::core::scene::Align;
use cove_ui::imaging::BundleImageResource;
use ruwriter_core::library::{name_of, parent_of};
use ruwriter_core::{AppearancePreference, Collection, Library};

use super::root::Layout;
use super::ui::{SidebarTreeRowStyle, hstack, icon_button, spacer, vstack};
use cove_ui::compose::theme::styles::components::tree_row_style::TreeRowStyleViewExt;
use crate::model::{Model, RenameTarget};

pub fn collection_icon(collection: Collection) -> BundleImageResource {
    match collection {
        Collection::All => icons::file::file_text(),
        Collection::Pinned => icons::shape::pin(),
        Collection::Recent => icons::data::clock(),
        Collection::Trash => icons::action::trash(),
    }
}

pub fn appearance_label(appearance: AppearancePreference) -> (&'static str, BundleImageResource) {
    match appearance {
        AppearancePreference::System => ("跟随系统", icons::media::monitor()),
        AppearancePreference::Light => ("浅色", icons::device::sun()),
        AppearancePreference::Dark => ("深色", icons::device::moon()),
    }
}

/// Row height shared by the collections and the folder tree.
const ROW: f32 = 30.0;

pub struct LibrarySidebar {
    pub model: Model,
    pub layout: Layout,
}

impl View for LibrarySidebar {
    fn body(&self, _cx: &Scope) -> AnyView {
        let _ = self.model.revision.get();
        let _ = self.model.listing.get();
        let header = vstack(0.0, Align::Stretch, Vec::new()).frame_height(self.layout.top_inset + 10.0);
        vstack(
            0.0,
            Align::Stretch,
            vec![
                AnyView::new(header),
                collections(&self.model),
                folders_header(&self.model),
                AnyView::new(
                    AnyView::new(FolderTree {
                        model: self.model.clone(),
                    })
                    .flex_with(1.0, 1.0, Some(0.0)),
                ),
                appearance_row(&self.model),
            ],
        )
    }
}

/// 全部文稿 · 置顶 · 最近 7 天 · 废纸篓. Nothing is selected here while a
/// folder is.
fn collections(model: &Model) -> AnyView {
    let counts = model
        .read(|session| Collection::ALL.map(|collection| session.count(collection)))
        .unwrap_or([0; 4]);
    let items = Collection::ALL
        .iter()
        .zip(counts)
        .map(|(collection, count)| {
            let item = SidebarItem::new(*collection, collection.title())
                .icon(collection_icon(*collection))
                .debug_name(format!("collection-{collection:?}"));
            if count > 0 {
                item.badge(count.to_string())
            } else {
                item
            }
        })
        .collect();
    let sections = vec![SidebarSection::new("library", items).title("资料库")];
    let get = model.clone();
    let set = model.clone();
    let selection = Binding::new(
        move || {
            let settings = get.settings();
            settings.folder.is_none().then_some(settings.collection)
        },
        move |value: Option<Collection>| {
            if let Some(collection) = value {
                let settings = set.settings();
                if settings.folder.is_some() || collection != settings.collection {
                    set.set_collection(collection);
                }
            }
        },
    );
    let sidebar = Sidebar::try_new(selection, sections)
        .expect("collections are unique")
        .adapts(false);
    // The sidebar grows to fill its parent; a fixed box keeps it to its rows
    // so the folder tree can take the rest.
    let height = ROW * 4.0 + 36.0;
    AnyView::new(
        vstack(
            0.0,
            Align::Stretch,
            vec![AnyView::new(AnyView::new(sidebar).flex_with(1.0, 1.0, Some(0.0)))],
        )
        .frame_height(height),
    )
}

fn folders_header(model: &Model) -> AnyView {
    let create = model.clone();
    let open = model.clone();
    AnyView::new(
        hstack(
            2.0,
            Align::Center,
            vec![
                AnyView::new(
                    AnyView::new(
                        Text::new(model.library_name())
                            .truncated()
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::on_surface_muted()),
                    )
                    .flex_with(1.0, 1.0, Some(0.0)),
                ),
                icon_button(icons::file::folder_open(), "打开文件夹…", false, move || {
                    open.open_library_picker()
                }),
                icon_button(icons::file::folder_plus(), "新建文件夹", false, move || {
                    create.new_folder("")
                }),
            ],
        )
        .padding_edges(14.0, 2.0)
        .debug_name("folders-header"),
    )
}

/// One folder in the tree, with its children built from the library.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FolderNode {
    id: String,
    name: String,
    count: usize,
    children: Vec<FolderNode>,
}

fn folder_nodes(library: &Library, parent: &str) -> Vec<FolderNode> {
    library
        .children(parent)
        .into_iter()
        .map(|folder| FolderNode {
            count: library.folder_count(&folder.id),
            children: folder_nodes(library, &folder.id),
            id: folder.id,
            name: folder.name,
        })
        .collect()
}

struct FolderTree {
    model: Model,
}

impl View for FolderTree {
    fn body(&self, _cx: &Scope) -> AnyView {
        let _ = self.model.revision.get();
        let _ = self.model.listing.get();
        let model = &self.model;
        let roots = model
            .read(|session| folder_nodes(&session.library, ""))
            .unwrap_or_default();
        if roots.is_empty() {
            return AnyView::new(
                AnyView::new(
                    Text::new("新建文件夹来整理文稿")
                        .semantic_font(SemanticFontRef::caption())
                        .semantic_foreground(SemanticColorRef::on_surface_muted()),
                )
                .padding_edges(16.0, 6.0),
            );
        }

        let get = model.clone();
        let set = model.clone();
        let selection = Binding::new(
            move || get.settings().folder.filter(|folder| !folder.is_empty()),
            move |value: Option<String>| {
                if let Some(folder) = value
                    && set.settings().folder.as_deref() != Some(folder.as_str())
                {
                    set.select_folder(&folder);
                }
            },
        );
        // Read live: the tree asks again after every toggle.
        let current = model.clone();
        let expand = model.clone();
        let expanded = Binding::new(
            move || {
                current
                    .read(|session| {
                        session
                            .library
                            .state
                            .expanded
                            .iter()
                            .cloned()
                            .collect::<HashSet<String>>()
                    })
                    .unwrap_or_default()
            },
            move |set: HashSet<String>| expand.set_expanded(set),
        );

        let drop_model = model.clone();
        let drag = TreeDragOperations {
            can_drop: Some(Rc::new(|source: &FolderNode, target: &FolderNode, position| {
                let parent = match position {
                    TreeDropPosition::Inside => target.id.as_str(),
                    _ => parent_of(&target.id),
                };
                source.id != target.id && !(!parent.is_empty() && Library::is_within(parent, &source.id))
            })),
            on_drop: Some(Rc::new(move |source: FolderNode, target: FolderNode, position| {
                match position {
                    TreeDropPosition::Inside => drop_model.move_folder(&source.id, &target.id, None),
                    TreeDropPosition::Before => drop_model.move_folder(
                        &source.id,
                        parent_of(&target.id),
                        Some(name_of(&target.id)),
                    ),
                    TreeDropPosition::After => {
                        let parent = parent_of(&target.id).to_owned();
                        let next = drop_model
                            .read(|session| {
                                let siblings = session.library.children(&parent);
                                let index = siblings.iter().position(|f| f.id == target.id)?;
                                siblings
                                    .iter()
                                    .skip(index + 1)
                                    .find(|f| f.id != source.id)
                                    .map(|f| f.name.clone())
                            })
                            .flatten();
                        drop_model.move_folder(&source.id, &parent, next.as_deref());
                    }
                }
            })),
            ..TreeDragOperations::default()
        };

        let icon: Rc<dyn Fn(&FolderNode) -> AnyView> = Rc::new(|_node: &FolderNode| {
            AnyView::new(Icon::with_semantic(
                icons::file::folder(),
                14.0,
                SemanticColorRef::accent(),
            ))
        });
        let options = TreeOptions {
            selection: TreeSelection {
                ids: Selection::single(selection),
                expanded: Some(expanded),
                ..TreeSelection::default()
            },
            layout: TreeLayout {
                row_height: ROW,
                leading_padding: 10.0,
                trailing_padding: 10.0,
                ..TreeLayout::default()
            },
            slots: TreeSlots {
                icon: Some(icon),
                ..TreeSlots::default()
            },
            events: TreeEvents::default(),
            drag,
            ..TreeOptions::default()
        };
        let row_model = model.clone();
        let tree = Tree::new(
            roots,
            |node: &FolderNode| node.id.clone(),
            |node: &FolderNode| node.children.clone(),
            move |node: &FolderNode, _state: RowState| folder_row(&row_model, node),
        )
        .options(options);
        AnyView::new(
            AnyView::new(tree)
                .tree_row_style(SidebarTreeRowStyle)
                .padding_edges(6.0, 0.0)
                .debug_name("folder-tree"),
        )
    }
}

fn folder_row(model: &Model, node: &FolderNode) -> AnyView {
    let mut children = vec![AnyView::new(
        AnyView::new(Text::new(node.name.clone()).truncated()).flex_with(1.0, 1.0, Some(0.0)),
    )];
    if node.count > 0 {
        children.push(AnyView::new(
            Text::new(node.count.to_string())
                .semantic_font(SemanticFontRef::caption())
                .semantic_foreground(SemanticColorRef::on_surface_muted()),
        ));
    }
    let label = AnyView::new(hstack(6.0, Align::Center, children).frame(
        Some(FrameDimension::Percent(100.0)),
        None,
        None,
        None,
        None,
        None,
    ));
    AnyView::new(label.context_menu(folder_menu(model, &node.id)))
}

fn folder_menu(model: &Model, id: &str) -> Vec<MenuEntry> {
    let new_document = model.clone();
    let new_folder = model.clone();
    let rename = model.clone();
    let trash = model.clone();
    let (a, b, c, d) = (id.to_owned(), id.to_owned(), id.to_owned(), id.to_owned());
    vec![
        MenuEntry::Item(MenuItem::new(1, "新建文稿", move || {
            new_document.select_folder(&a);
            new_document.create();
        })),
        MenuEntry::Item(MenuItem::new(2, "新建子文件夹", move || {
            new_folder.new_folder(&b)
        })),
        MenuEntry::separator(3),
        MenuEntry::Item(MenuItem::new(4, "重命名…", move || {
            rename.begin_rename(RenameTarget::Folder(c.clone()))
        })),
        MenuEntry::separator(5),
        MenuEntry::Item(
            MenuItem::new(6, "移到废纸篓", move || trash.trash_folder(&d))
                .role(MenuItemRole::Destructive),
        ),
    ]
}

fn appearance_row(model: &Model) -> AnyView {
    let (title, icon) = appearance_label(model.settings().appearance);
    let label = hstack(
        10.0,
        Align::Center,
        vec![
            AnyView::new(ButtonIcon::new(
                ButtonIconSource::resource(icon),
                Some(16.0),
                None,
            )),
            AnyView::new(
                Text::new(format!("外观：{title}"))
                    .truncated()
                    .semantic_font(SemanticFontRef::label())
                    .semantic_foreground(SemanticColorRef::on_surface_variant()),
            ),
            spacer(),
        ],
    );
    let cycle = model.clone();
    AnyView::new(
        AnyView::new(
            Button::new(label.padding_edges(10.0, 0.0), move || cycle.cycle_appearance())
                .accessibility_text("切换外观"),
        )
        .button_style(SidebarItemStyle {
            minimum_height: ROW,
            presentation: SidebarPresentation::Expanded,
        })
        .padding_edges(8.0, 8.0),
    )
}
