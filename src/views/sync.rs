//! Cloud backup: GitHub CLI login status, automatic private Gist, and the
//! manual sync button. Ruwriter never stores a token of its own.

use cove_ui::compose::modifiers::layout::{LayoutFrameExt, PaddingExt};
use cove_ui::compose::modifiers::style::SemanticStyleViewExt;
use cove_ui::compose::primitives::controls::Button;
use cove_ui::compose::primitives::overlays::Dialog;
use cove_ui::compose::primitives::text::Text;
use cove_ui::compose::theme::styles::buttons::{ButtonStyleViewExt, PrimaryButtonStyle};
use cove_ui::compose::theme::{SemanticColorRef, SemanticFontRef};
use cove_ui::composition::{AnyView, EmptyView, Scope, View};
use cove_ui::core::Binding;
use cove_ui::core::scene::Align;

use super::ui::{hstack, spacer, vstack};
use crate::model::Model;

pub struct SyncDialog {
    pub model: Model,
}

fn note(text: impl Into<String>, muted: bool) -> AnyView {
    let text = text.into();
    AnyView::new(
        Text::new(text)
            .line_limit(Some(4))
            .semantic_font(SemanticFontRef::caption())
            .semantic_foreground(if muted {
                SemanticColorRef::on_surface_muted()
            } else {
                SemanticColorRef::on_surface_variant()
            }),
    )
}

impl View for SyncDialog {
    fn body(&self, _cx: &Scope) -> AnyView {
        if !self.model.sync_open.get() {
            return AnyView::new(EmptyView);
        }
        let busy = self.model.sync_busy.get();
        let checking = self.model.sync_checking.get();
        let failed = self.model.sync_failed.get();
        let status = self.model.sync_status.get();
        let login = self.model.sync_login.get();
        let status_color = if failed {
            SemanticColorRef::error()
        } else {
            SemanticColorRef::on_surface_variant()
        };
        let status_view = AnyView::new(
            AnyView::new(
                Text::new(status)
                    .line_limit(Some(5))
                    .semantic_font(SemanticFontRef::body())
                    .semantic_foreground(status_color),
            )
            .frame_height(76.0),
        );

        let login_card = AnyView::new(
            AnyView::new(vstack(
                6.0,
                Align::Stretch,
                vec![
                    AnyView::new(
                        Text::new(login)
                            .line_limit(Some(2))
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::on_surface_variant()),
                    ),
                    note(
                        "登录由 GitHub CLI 管理。终端运行 gh auth login 即可；Ruwriter 不保存 Token，只在每次同步时读取一次，结束即丢弃。",
                        true,
                    ),
                ],
            ))
            .padding(12.0),
        );

        let rows = vstack(
            14.0,
            Align::Stretch,
            vec![
                note(
                    "备份到自己的私有 GitHub Gist。每篇文稿一个纯文本文件，文件夹、标题、标签等写在清单里；正文仍以本地文件为唯一真相，冲突时两份都保留。",
                    false,
                ),
                login_card,
                note(
                    "换电脑时，复制完整的资料库文件夹（含隐藏的 .ruwriter 目录），用同一个 GitHub 账号同步即可。",
                    true,
                ),
                status_view,
            ],
        );

        let close = self.model.clone();
        let check = self.model.clone();
        let sync = self.model.clone();
        let footer = hstack(
            8.0,
            Align::Center,
            vec![
                AnyView::new(Button::title("关闭", move || close.close_sync())),
                spacer(),
                AnyView::new(
                    Button::title("检查登录", move || check.check_sync_login())
                        .enabled(!checking && !busy),
                ),
                AnyView::new(
                    AnyView::new(
                        Button::title(
                            if busy { "同步中…" } else { "立即同步" },
                            move || sync.sync_now(),
                        )
                        .enabled(!busy),
                    )
                    .button_style(PrimaryButtonStyle),
                ),
            ],
        );
        let closer = self.model.clone();
        let presented = Binding::new(
            || true,
            move |shown: bool| {
                if !shown {
                    closer.close_sync();
                }
            },
        );
        AnyView::new(
            AnyView::new(
                Dialog::new(presented, "云同步", rows)
                    .footer(footer)
                    .configure(|options| {
                        options.modal.fits_content_height = true;
                        options.modal.geometry.width = 560.0;
                    }),
            )
            .padding(4.0),
        )
    }
}
