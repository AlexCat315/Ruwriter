//! Library navigation: the collections and the appearance switch.

use cove_ui::compose::modifiers::layout::{FlexExt, LayoutFrameExt, PaddingExt};
use cove_ui::compose::modifiers::style::SemanticStyleViewExt;
use cove_ui::compose::primitives::controls::{Button, ButtonIcon, ButtonIconSource};
use cove_ui::compose::primitives::layout::sidebar::{
    Sidebar, SidebarItem, SidebarItemStyle, SidebarPresentation, SidebarSection,
};
use cove_ui::compose::primitives::media::ui_icon_catalog as icons;
use cove_ui::compose::primitives::text::Text;
use cove_ui::compose::theme::styles::buttons::ButtonStyleViewExt;
use cove_ui::compose::theme::{SemanticColorRef, SemanticFontRef};
use cove_ui::composition::{AnyView, Scope, View};
use cove_ui::core::Binding;
use cove_ui::core::scene::Align;
use cove_ui::imaging::BundleImageResource;
use ruwriter_core::{AppearancePreference, Collection};

use super::root::Layout;
use super::ui::{hstack, vstack};
use crate::model::Model;

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

pub struct LibrarySidebar {
    pub model: Model,
    pub layout: Layout,
}

impl View for LibrarySidebar {
    fn body(&self, _cx: &Scope) -> AnyView {
        let _ = self.model.revision.get();
        let _ = self.model.listing.get();
        let model = &self.model;
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
            move || Some(get.settings().collection),
            move |value: Option<Collection>| {
                if let Some(collection) = value
                    && collection != set.settings().collection
                {
                    set.set_collection(collection);
                }
            },
        );

        // Clears the window controls that sit over the sidebar's top edge.
        let header =
            vstack(0.0, Align::Stretch, Vec::new()).frame_height(self.layout.top_inset + 10.0);
        let sidebar = Sidebar::try_new(selection, sections)
            .expect("collections are unique")
            .adapts(false)
            .header(header)
            .footer(appearance_row(model));
        AnyView::new(AnyView::new(sidebar).flex_with(1.0, 1.0, Some(0.0)))
    }
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
        ],
    );
    let cycle = model.clone();
    AnyView::new(
        AnyView::new(
            Button::new(label.padding_edges(10.0, 0.0), move || {
                cycle.cycle_appearance()
            })
            .accessibility_text("切换外观"),
        )
        .button_style(SidebarItemStyle {
            minimum_height: 30.0,
            presentation: SidebarPresentation::Expanded,
        })
        .padding_edges(0.0, 8.0),
    )
}
