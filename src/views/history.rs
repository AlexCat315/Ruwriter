//! Version history: plain-text snapshots of the open document. Restoring one
//! snapshots the text being replaced first, so the previous state is safe too.

use cove_ui::compose::modifiers::layout::{LayoutFrameExt, PaddingExt};
use cove_ui::compose::modifiers::style::SemanticStyleViewExt;
use cove_ui::compose::primitives::controls::Button;
use cove_ui::compose::primitives::overlays::Dialog;
use cove_ui::compose::primitives::text::Text;
use cove_ui::compose::theme::styles::buttons::ButtonStyleViewExt;
use cove_ui::compose::theme::{SemanticColorRef, SemanticFontRef};
use cove_ui::compose::{ScrollAxis, ScrollView};
use cove_ui::composition::{AnyView, EmptyView, Scope, View};
use cove_ui::core::Binding;
use cove_ui::core::scene::Align;

use super::ui::{RowButtonStyle, hstack, spacer, vstack};
use crate::dates;
use crate::model::Model;

pub struct HistoryDialog {
    pub model: Model,
}

fn size(chars: usize) -> String {
    format!("{chars} 字")
}

impl View for HistoryDialog {
    fn body(&self, _cx: &Scope) -> AnyView {
        if !self.model.history_open.get() {
            return AnyView::new(EmptyView);
        }
        let entries = self.model.history_entries.get();
        let content = if entries.is_empty() {
            AnyView::new(
                vstack(
                    6.0,
                    Align::Center,
                    vec![
                        AnyView::new(
                            Text::new("还没有历史版本")
                                .semantic_font(SemanticFontRef::body())
                                .semantic_foreground(SemanticColorRef::on_surface()),
                        ),
                        AnyView::new(
                            Text::new("写一会儿会自动留下快照，也可以现在手动保存一份")
                                .semantic_font(SemanticFontRef::caption())
                                .semantic_foreground(SemanticColorRef::on_surface_muted()),
                        ),
                    ],
                )
                .frame_height(180.0),
            )
        } else {
            let rows: Vec<AnyView> = entries
                .iter()
                .map(|snapshot| {
                    let model = self.model.clone();
                    let name = snapshot.name.clone();
                    let excerpt = if snapshot.excerpt.is_empty() {
                        "（空）".to_owned()
                    } else {
                        snapshot.excerpt.clone()
                    };
                    let label = vstack(
                        3.0,
                        Align::Stretch,
                        vec![
                            hstack(
                                10.0,
                                Align::Center,
                                vec![
                                    AnyView::new(
                                        Text::new(dates::stamp(snapshot.created_at))
                                            .semantic_font(SemanticFontRef::caption())
                                            .semantic_foreground(
                                                SemanticColorRef::on_surface_variant(),
                                            ),
                                    ),
                                    spacer(),
                                    AnyView::new(
                                        Text::new(size(snapshot.chars))
                                            .semantic_font(SemanticFontRef::caption())
                                            .semantic_foreground(
                                                SemanticColorRef::on_surface_muted(),
                                            ),
                                    ),
                                ],
                            ),
                            AnyView::new(
                                Text::new(excerpt)
                                    .truncated()
                                    .semantic_font(SemanticFontRef::body())
                                    .semantic_foreground(SemanticColorRef::on_surface()),
                            ),
                        ],
                    )
                    .fill_width();
                    AnyView::new(
                        AnyView::new(
                            Button::new(label, move || model.restore_snapshot(&name))
                                .accessibility_text(format!(
                                    "恢复到 {}",
                                    dates::stamp(snapshot.created_at)
                                ))
                                .tooltip("恢复到这个版本"),
                        )
                        .button_style(RowButtonStyle)
                        .padding_insets(cove_ui::compose::modifiers::layout::EdgeInsets {
                            top: 5.0,
                            leading: 8.0,
                            bottom: 5.0,
                            trailing: 8.0,
                        })
                        .fill_width(),
                    )
                })
                .collect();
            let height = (entries.len() as f32 * 76.0 + 8.0).clamp(70.0, 300.0);
            AnyView::new(
                ScrollView::new(ScrollAxis::Vertical, vstack(2.0, Align::Stretch, rows))
                    .frame_height(height),
            )
        };

        let create_model = self.model.clone();
        let close_model = self.model.clone();
        let footer = hstack(
            8.0,
            Align::Center,
            vec![
                AnyView::new(
                    Text::new("点击一条即可恢复；最近保留 30 份")
                        .semantic_font(SemanticFontRef::caption())
                        .semantic_foreground(SemanticColorRef::on_surface_muted()),
                ),
                spacer(),
                AnyView::new(Button::title("关闭", move || close_model.close_history())),
                AnyView::new(Button::title("立即创建快照", move || {
                    create_model.snapshot_now()
                })),
            ],
        );
        let closer = self.model.clone();
        let presented = Binding::new(
            || true,
            move |shown: bool| {
                if !shown {
                    closer.close_history();
                }
            },
        );
        AnyView::new(
            Dialog::new(presented, "历史版本", content)
                .footer(footer)
                .configure(|options| {
                    options.modal.fits_content_height = true;
                    options.modal.geometry.width = 620.0;
                }),
        )
    }
}
