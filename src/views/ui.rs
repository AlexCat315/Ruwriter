//! Small layout helpers and the custom styles Ruwriter draws with.

use cove_ui::compose::modifiers::layout::{FlexExt, LayoutFrameExt, PaddingExt};
use cove_ui::compose::modifiers::style::{AppearanceExt, SemanticStyleViewExt};
use cove_ui::compose::primitives::controls::{Button, ButtonIconSource};
use cove_ui::compose::theme::SemanticColorRef;
use cove_ui::compose::theme::styles::buttons::{
    ButtonStyle, ButtonStyleConfiguration, ButtonStyleViewExt, GhostButtonStyle,
};
use cove_ui::compose::theme::styles::components::text_field_style::{
    TextFieldStyle, TextFieldStyleConfiguration,
};
use cove_ui::compose::{FrameDimension, Spacer};
use cove_ui::composition::{AnyView, BoxView, TupleView};
use cove_ui::core::scene::{Align, FlexDirection, Justify, Wrap};
use cove_ui::imaging::BundleImageResource;

pub fn hstack(spacing: f32, align: Align, children: Vec<AnyView>) -> AnyView {
    AnyView::new(BoxView::new(
        FlexDirection::Row,
        align,
        Justify::FlexStart,
        Wrap::NoWrap,
        spacing,
        AnyView::new(TupleView::new(children)),
    ))
}

pub fn vstack(spacing: f32, align: Align, children: Vec<AnyView>) -> AnyView {
    AnyView::new(BoxView::new(
        FlexDirection::Column,
        align,
        Justify::FlexStart,
        Wrap::NoWrap,
        spacing,
        AnyView::new(TupleView::new(children)),
    ))
}

pub fn spacer() -> AnyView {
    AnyView::new(Spacer::new(0.0))
}

pub fn full_size(view: impl cove_ui::composition::View + 'static) -> AnyView {
    AnyView::new(AnyView::new(view).frame(
        Some(FrameDimension::Percent(100.0)),
        Some(FrameDimension::Percent(100.0)),
        None,
        None,
        None,
        None,
    ))
}

pub fn full_height(view: impl cove_ui::composition::View + 'static) -> AnyView {
    AnyView::new(AnyView::new(view).frame(
        None,
        Some(FrameDimension::Percent(100.0)),
        None,
        None,
        None,
        None,
    ))
}

/// Borderless icon control with a tooltip and an accessible name.
pub fn icon_button(
    icon: BundleImageResource,
    label: &str,
    selected: bool,
    action: impl Fn() + 'static,
) -> AnyView {
    AnyView::new(
        AnyView::new(
            Button::icon(ButtonIconSource::resource(icon), 16.0, action)
                .selected(selected)
                .tooltip(label)
                .accessibility_text(label),
        )
        .button_style(ToolButtonStyle),
    )
}

/// Toolbar glyph: a ghost button whose on state is a soft accent wash.
#[derive(Clone, Copy, Debug, Default)]
pub struct ToolButtonStyle;

impl ButtonStyle for ToolButtonStyle {
    fn make_body(&self, configuration: ButtonStyleConfiguration) -> AnyView {
        let selected = configuration.is_selected;
        let ghost = GhostButtonStyle.make_body(configuration);
        if selected {
            AnyView::new(
                ghost
                    .semantic_foreground(SemanticColorRef::accent())
                    .semantic_background(SemanticColorRef::accent_muted())
                    .corner_radius(7.0),
            )
        } else {
            ghost
        }
    }
}

/// A document row: full-width, rounded, filled when selected or pressed.
/// Hover and press are drawn at once; nothing here animates.
#[derive(Clone, Copy, Debug, Default)]
pub struct RowButtonStyle;

impl ButtonStyle for RowButtonStyle {
    fn make_body(&self, configuration: ButtonStyleConfiguration) -> AnyView {
        let fill = if configuration.is_selected {
            SemanticColorRef::accent_muted()
        } else if configuration.is_pressed {
            SemanticColorRef::state_layer_pressed()
        } else if configuration.is_hovered {
            SemanticColorRef::state_layer_hover()
        } else {
            SemanticColorRef::clear()
        };
        let body = configuration
            .label
            .padding_edges(12.0, 10.0)
            .semantic_background(fill)
            .corner_radius(8.0);
        if configuration.is_focused {
            AnyView::new(body.semantic_border(SemanticColorRef::focus_ring(), 2.0))
        } else {
            AnyView::new(body)
        }
    }
}

/// The editor draws no chrome: the page is the window.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProseFieldStyle;

impl TextFieldStyle for ProseFieldStyle {
    fn make_body(&self, configuration: TextFieldStyleConfiguration) -> AnyView {
        configuration.content
    }
}

/// Folder tree rows that look like the sidebar's own rows: the same neutral
/// selection fill and radius, a hover wash, full-width bands, and no change
/// of ink when selected.
#[derive(Clone, Copy, Debug, Default)]
pub struct SidebarTreeRowStyle;

impl cove_ui::compose::theme::styles::components::tree_row_style::TreeRowStyle
    for SidebarTreeRowStyle
{
    fn make_body(
        &self,
        configuration: cove_ui::compose::theme::styles::components::tree_row_style::TreeRowStyleConfiguration,
    ) -> AnyView {
        use cove_ui::compose::theme::styles::components::selection_appearance::{
            SelectionAppearance, SelectionRole,
        };
        let appearance = SelectionAppearance::resolve(SelectionRole::Sidebar, &configuration.theme);
        let fill = if configuration.is_selected {
            appearance.fill
        } else if configuration.is_hovered || configuration.is_search_hit {
            configuration.theme.colors.state_layer_hover
        } else {
            cove_ui::core::Color::CLEAR
        };
        let row = hstack(
            0.0,
            Align::Center,
            vec![
                AnyView::new(
                    AnyView::new(configuration.content.clone())
                        .flex_with(1.0, 1.0, Some(0.0)),
                ),
            ],
        )
        .padding_edges(8.0, 0.0)
        .frame(
            Some(FrameDimension::Percent(100.0)),
            Some(FrameDimension::Percent(100.0)),
            None,
            None,
            None,
            None,
        )
        .background_color(fill)
        .corner_radius(appearance.corner_radius);
        let row = AnyView::new(AnyView::new(row).flex_with(1.0, 1.0, Some(0.0)));
        if configuration.is_enabled {
            row
        } else {
            AnyView::new(row.opacity(0.55))
        }
    }
}
