//! Editing a document's author metadata: a synopsis of your own, where the
//! document is in the writing process, and tags. All of it lives in the
//! library state; the Markdown file is never rewritten.

use std::rc::Rc;

use cove_ui::compose::modifiers::layout::LayoutFrameExt;
use cove_ui::compose::modifiers::style::SemanticStyleViewExt;
use cove_ui::compose::primitives::controls::{Button, ButtonIcon, ButtonIconSource, MenuButton};
use cove_ui::compose::primitives::media::ui_icon_catalog as icons;
use cove_ui::compose::primitives::overlays::{Dialog, MenuEntry, MenuItem};
use cove_ui::compose::primitives::text::{
    Text, TextField, TextFieldAxis, TextFieldEvents, TextFieldLayout,
};
use cove_ui::compose::theme::styles::buttons::{ButtonStyleViewExt, PrimaryButtonStyle};
use cove_ui::compose::theme::{SemanticColorRef, SemanticFontRef};
use cove_ui::composition::{AnyView, EmptyView, Scope, View};
use cove_ui::core::Binding;
use cove_ui::core::scene::Align;
use ruwriter_core::DocStatus;

use super::ui::{hstack, spacer, vstack};
use crate::model::Model;

pub struct InfoDialog {
    pub model: Model,
}

impl InfoDialog {
    fn status_menu(&self) -> Vec<MenuEntry> {
        let current = self.model.info_status.get();
        DocStatus::ALL
            .iter()
            .enumerate()
            .map(|(index, status)| {
                let choose = self.model.clone();
                let status = *status;
                MenuEntry::Item(
                    MenuItem::new(index as u64 + 1, status.title(), move || {
                        choose.set_info_status(status)
                    })
                    .selected(status == current),
                )
            })
            .collect()
    }
}

impl View for InfoDialog {
    fn body(&self, _cx: &Scope) -> AnyView {
        if self.model.info_document.get().is_none() {
            return AnyView::new(EmptyView);
        }
        let _ = self.model.info_status.get();

        let synopsis = TextField::new(
            "给自己的摘要，留空则显示正文开头",
            self.model.info_synopsis.binding(),
        )
        .layout(TextFieldLayout {
            axis: TextFieldAxis::Vertical,
            max_visible_lines: 4,
            wraps_lines: true,
            ..Default::default()
        })
        .frame_height(84.0);

        let submit = self.model.clone();
        let tags = TextField::new(
            "用逗号分隔，例如：悬疑, 第一幕",
            self.model.info_tags.binding(),
        )
        .events(TextFieldEvents {
            on_submit: Some(Rc::new(move || submit.commit_info())),
            ..Default::default()
        });

        let status_label = hstack(
            4.0,
            Align::Center,
            vec![
                AnyView::new(Text::new(self.model.info_status.get().title())),
                AnyView::new(ButtonIcon::new(
                    ButtonIconSource::resource(icons::navigation::chevron_down()),
                    Some(12.0),
                    None,
                )),
            ],
        );
        let status = AnyView::new(hstack(
            0.0,
            Align::Center,
            vec![
                AnyView::new(MenuButton::new(self.status_menu(), status_label).width(120.0)),
                spacer(),
            ],
        ));

        let rows = vstack(
            14.0,
            Align::Stretch,
            vec![
                labeled("摘要", AnyView::new(synopsis)),
                labeled("状态", status),
                labeled("标签", AnyView::new(tags)),
            ],
        );

        let confirm = self.model.clone();
        let dismiss = self.model.clone();
        let footer = hstack(
            8.0,
            Align::Center,
            vec![
                AnyView::new(
                    Text::new("元数据保存在资料库状态里，不写入 Markdown 文件")
                        .semantic_font(SemanticFontRef::caption())
                        .semantic_foreground(SemanticColorRef::on_surface_muted()),
                ),
                spacer(),
                AnyView::new(Button::title("取消", move || dismiss.cancel_info())),
                AnyView::new(
                    AnyView::new(Button::title("保存", move || confirm.commit_info()))
                        .button_style(PrimaryButtonStyle),
                ),
            ],
        );
        let closer = self.model.clone();
        let presented = Binding::new(
            || true,
            move |shown: bool| {
                if !shown {
                    closer.cancel_info();
                }
            },
        );
        AnyView::new(
            Dialog::new(presented, "文稿信息", rows)
                .footer(footer)
                .configure(|options| {
                    options.modal.fits_content_height = true;
                    options.modal.geometry.width = 460.0;
                }),
        )
    }
}

fn labeled(title: &str, field: AnyView) -> AnyView {
    AnyView::new(vstack(
        6.0,
        Align::Stretch,
        vec![
            AnyView::new(
                Text::new(title)
                    .semantic_font(SemanticFontRef::caption())
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
            ),
            field,
        ],
    ))
}
