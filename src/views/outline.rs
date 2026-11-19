//! The outline pane: the heading tree of the open document, next to the
//! editor. Clicking a heading moves the caret there and returns focus to the
//! page. Collapsed when the window is narrow or focus mode is on.

use cove_ui::compose::modifiers::layout::{FlexExt, LayoutFrameExt, PaddingExt};
use cove_ui::compose::modifiers::style::SemanticStyleViewExt;
use cove_ui::compose::primitives::media::ui_icon_catalog as icons;
use cove_ui::compose::primitives::text::Text;
use cove_ui::compose::theme::styles::buttons::ButtonStyleViewExt;
use cove_ui::compose::theme::{SemanticColorRef, SemanticFontRef};
use cove_ui::compose::{FrameDimension, ScrollAxis, ScrollView};
use cove_ui::composition::{AnyView, Scope, View, ViewExt};
use cove_ui::core::scene::Align;

use super::ui::{RowButtonStyle, hstack, icon_button, spacer, vstack};
use crate::model::Model;

pub struct OutlinePane {
    pub model: Model,
    pub top_inset: f32,
    pub bottom_inset: f32,
}

impl View for OutlinePane {
    fn body(&self, _cx: &Scope) -> AnyView {
        let (headings, current) = self.model.outline_at_caret();
        let count = headings.len();

        let close = self.model.clone();
        let header = AnyView::new(
            hstack(
                6.0,
                Align::Center,
                vec![
                    AnyView::new(
                        Text::new("大纲")
                            .semantic_font(SemanticFontRef::body_strong())
                            .semantic_foreground(SemanticColorRef::on_surface()),
                    ),
                    AnyView::new(
                        Text::new(count.to_string())
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::on_surface_muted()),
                    ),
                    spacer(),
                    icon_button(
                        icons::navigation::chevron_right(),
                        "隐藏大纲",
                        false,
                        move || close.toggle_outline(),
                    ),
                ],
            )
            .padding_insets(cove_ui::compose::modifiers::layout::EdgeInsets {
                top: self.top_inset + 10.0,
                leading: 12.0,
                bottom: 8.0,
                trailing: 8.0,
            }),
        );

        let body = if headings.is_empty() {
            AnyView::new(
                vstack(
                    6.0,
                    Align::Center,
                    vec![
                        AnyView::new(
                            Text::new("还没有标题")
                                .semantic_font(SemanticFontRef::caption())
                                .semantic_foreground(SemanticColorRef::on_surface_muted()),
                        ),
                        AnyView::new(
                            Text::new(
                                if self.model.writing_mode() == ruwriter_core::WritingMode::Novel {
                                    "另起一行写「第一章 雨夜」"
                                } else {
                                    "用 # 或 ## 标记章节"
                                },
                            )
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::on_surface_muted()),
                        ),
                    ],
                )
                .padding_insets(cove_ui::compose::modifiers::layout::EdgeInsets {
                    top: 32.0,
                    leading: 16.0,
                    bottom: 16.0,
                    trailing: 16.0,
                }),
            )
        } else {
            let rows: Vec<AnyView> = headings
                .iter()
                .enumerate()
                .map(|(index, heading)| {
                    let level = heading.level as usize;
                    let title = heading.title.clone();
                    let offset = heading.offset;
                    let jump = self.model.clone();
                    let strong = level == 1;
                    let label = AnyView::new(
                        AnyView::new(
                            Text::new(title)
                                .truncated()
                                .semantic_font(if strong {
                                    SemanticFontRef::body_strong()
                                } else {
                                    SemanticFontRef::body()
                                })
                                .semantic_foreground(if current == Some(index) {
                                    SemanticColorRef::accent()
                                } else {
                                    SemanticColorRef::on_surface_variant()
                                }),
                        )
                        .padding_insets(cove_ui::compose::modifiers::layout::EdgeInsets {
                            top: 3.0,
                            leading: (level.saturating_sub(1)) as f32 * 14.0,
                            bottom: 3.0,
                            trailing: 0.0,
                        })
                        .frame(
                            Some(FrameDimension::Percent(100.0)),
                            None,
                            None,
                            None,
                            None,
                            None,
                        ),
                    );
                    AnyView::new(
                        AnyView::new(
                            AnyView::new(
                                cove_ui::compose::primitives::controls::Button::new(
                                    label,
                                    move || jump.jump_to_heading(offset),
                                )
                                .selected(current == Some(index))
                                .accessibility_text(format!("跳转到：{}", heading.title)),
                            )
                            .button_style(RowButtonStyle),
                        )
                        .id(format!("outline-{offset}")),
                    )
                })
                .collect();
            AnyView::new(
                ScrollView::new(
                    ScrollAxis::Vertical,
                    vstack(2.0, Align::Stretch, rows).padding_insets(
                        cove_ui::compose::modifiers::layout::EdgeInsets {
                            top: 2.0,
                            leading: 6.0,
                            bottom: 12.0 + self.bottom_inset,
                            trailing: 6.0,
                        },
                    ),
                )
                .flex_with(1.0, 1.0, Some(0.0)),
            )
        };

        AnyView::new(vstack(
            0.0,
            Align::Stretch,
            vec![header, AnyView::new(body.flex_with(1.0, 1.0, Some(0.0)))],
        ))
    }
}
