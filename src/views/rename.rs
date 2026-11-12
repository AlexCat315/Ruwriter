//! Naming a folder or a file: one field, Return confirms, Esc cancels.

use std::rc::Rc;

use cove_ui::compose::primitives::controls::Button;
use cove_ui::compose::primitives::overlays::Dialog;
use cove_ui::compose::primitives::text::text_field::TextFieldNavigationRequest;
use cove_ui::compose::primitives::text::{TextField, TextFieldEvents};
use cove_ui::compose::theme::styles::buttons::{ButtonStyleViewExt, PrimaryButtonStyle};
use cove_ui::composition::{AnyView, EmptyView, Scope, View};
use cove_ui::core::Binding;
use cove_ui::core::scene::Align;

use super::ui::{hstack, spacer};
use crate::model::{Model, RenameTarget};

pub struct RenameDialog {
    pub model: Model,
}

impl View for RenameDialog {
    fn body(&self, _cx: &Scope) -> AnyView {
        let Some(target) = self.model.rename.get() else {
            return AnyView::new(EmptyView);
        };
        let title = match target {
            RenameTarget::Folder(_) => "文件夹名称",
            RenameTarget::Document(_) => "文件名",
        };
        let submit = self.model.clone();
        let cancel = self.model.clone();
        let mut field = TextField::new("名称", self.model.rename_text.binding()).events(
            TextFieldEvents {
                on_submit: Some(Rc::new(move || submit.commit_rename())),
                on_cancel: Some(Rc::new(move || cancel.cancel_rename())),
                ..Default::default()
            },
        );
        field.navigation = TextFieldNavigationRequest {
            focus_request_id: Some(1),
            ..Default::default()
        };

        let confirm = self.model.clone();
        let dismiss = self.model.clone();
        let footer = hstack(
            8.0,
            Align::Center,
            vec![
                spacer(),
                AnyView::new(Button::title("取消", move || dismiss.cancel_rename())),
                AnyView::new(
                    AnyView::new(Button::title("确定", move || confirm.commit_rename()))
                        .button_style(PrimaryButtonStyle),
                ),
            ],
        );
        let closer = self.model.clone();
        let presented = Binding::new(
            || true,
            move |shown: bool| {
                if !shown {
                    closer.cancel_rename();
                }
            },
        );
        AnyView::new(
            Dialog::new(presented, title, field)
                .footer(footer)
                .configure(|options| {
                    options.modal.fits_content_height = true;
                    options.modal.geometry.width = 420.0;
                }),
        )
    }
}
