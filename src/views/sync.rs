use cove_ui::compose::modifiers::layout::{FlexExt, LayoutFrameExt};
use cove_ui::compose::modifiers::style::{AppearanceExt, SemanticStyleViewExt};
use cove_ui::compose::primitives::controls::{Button, ButtonRole};
use cove_ui::compose::primitives::overlays::Dialog;
use cove_ui::compose::primitives::text::{Text, TextBuffer, TextField, TextFieldBehavior};
use cove_ui::compose::theme::styles::buttons::{
    ButtonStyleViewExt, GhostButtonStyle, PrimaryButtonStyle, ToggleButtonStyle,
};
use cove_ui::compose::theme::{SemanticColorRef, SemanticFontRef};
use cove_ui::compose::{ScrollAxis, ScrollView};
use cove_ui::composition::{AnyView, EmptyView, Scope, View, ViewExt};
use cove_ui::core::Binding;
use cove_ui::core::scene::Align;
use ruwriter_core::sync::SyncDocument;
use std::collections::BTreeMap;
use std::{cell::RefCell, rc::Rc};

use super::ui::{hstack, spacer, vstack};
use crate::model::Model;

pub struct SyncDialog {
    pub model: Model,
}

fn note(text: impl Into<String>) -> AnyView {
    AnyView::new(
        Text::new(text.into())
            .line_limit(Some(3))
            .semantic_font(SemanticFontRef::caption())
            .semantic_foreground(SemanticColorRef::on_surface_muted()),
    )
}

fn title(document: &SyncDocument) -> &str {
    if document.title.trim().is_empty() {
        document.path.rsplit('/').next().unwrap_or(&document.path)
    } else {
        &document.title
    }
}

impl View for SyncDialog {
    fn body(&self, cx: &Scope) -> AnyView {
        if !self.model.sync_open.get() {
            return AnyView::new(EmptyView);
        }
        let cloud = cx.state(|| false);
        let selected = cx.state(|| None::<String>);
        let query = cx.state(TextBuffer::empty);
        let show_deleted = cx.state(|| false);
        let confirm = cx.state(|| None::<SyncDocument>);
        let preview_cache = cx.remember(|| Rc::new(RefCell::new(None::<(String, TextBuffer)>)));
        let busy = self.model.sync_busy.get();
        let on_cloud = cloud.get();
        let local = self.model.sync_local.get();
        let remote = self.model.sync_remote.get();
        let cloud_loaded = self.model.sync_remote_loaded.get();
        let cloud_documents = remote
            .as_ref()
            .map_or(&[][..], |value| value.documents.as_slice());
        let documents = if on_cloud {
            cloud_documents
        } else {
            local.as_slice()
        };
        let other = if on_cloud {
            local.as_slice()
        } else {
            cloud_documents
        };
        let counterpart: BTreeMap<_, _> = other.iter().map(|doc| (doc.uid.as_str(), doc)).collect();
        let search = query.get().string_value().trim().to_lowercase();
        let deleted = show_deleted.get();
        let mut visible: Vec<_> = documents
            .iter()
            .filter(|document| {
                (deleted || !document.deleted)
                    && (search.is_empty()
                        || title(document).to_lowercase().contains(&search)
                        || document.path.to_lowercase().contains(&search))
            })
            .collect();
        visible.sort_by(|a, b| a.deleted.cmp(&b.deleted).then_with(|| a.path.cmp(&b.path)));
        let selection = selected.get();
        let current = visible
            .iter()
            .find(|doc| selection.as_ref() == Some(&doc.uid))
            .copied();

        let switch_cloud = self.model.clone();
        let refresh = self.model.clone();
        let toolbar = hstack(
            6.0,
            Align::Center,
            vec![
                AnyView::new(
                    Button::title("本地", move || {
                        cloud.set(false);
                        confirm.set(None);
                    })
                    .selected(!on_cloud)
                    .button_style(ToggleButtonStyle::new(48.0, 30.0)),
                ),
                AnyView::new(
                    Button::title("云端", move || {
                        cloud.set(true);
                        confirm.set(None);
                        if !switch_cloud.sync_remote_loaded.get_untracked() {
                            switch_cloud.refresh_sync_remote();
                        }
                    })
                    .selected(on_cloud)
                    .button_style(ToggleButtonStyle::new(48.0, 30.0)),
                ),
                spacer(),
                AnyView::new(
                    Button::title(
                        if deleted {
                            "隐藏已删除"
                        } else {
                            "显示已删除"
                        },
                        move || {
                            show_deleted.update(|value| *value = !*value);
                            confirm.set(None);
                        },
                    )
                    .button_style(GhostButtonStyle),
                ),
                AnyView::new(
                    Button::title("刷新清单", move || {
                        confirm.set(None);
                        if cloud.get_untracked() {
                            refresh.refresh_sync_remote();
                        } else {
                            refresh.refresh_sync_local();
                        }
                    })
                    .enabled(!busy),
                ),
            ],
        );
        let mut entries = Vec::new();
        for document in &visible {
            let uid = document.uid.clone();
            let active = selection.as_ref() == Some(&uid);
            let name = if document.deleted {
                format!("{} · 已删除", title(document))
            } else {
                title(document).to_owned()
            };
            let label = hstack(
                0.0,
                Align::Center,
                vec![
                    AnyView::new(
                        Text::new(name.clone())
                            .truncated()
                            .flex_with(0.0, 1.0, None),
                    ),
                    spacer(),
                ],
            )
            .frame_width(194.0);
            entries.push(AnyView::new(
                AnyView::new(
                    Button::new(label, move || {
                        selected.set(Some(uid.clone()));
                        confirm.set(None);
                    })
                    .selected(active)
                    .accessibility_text(name)
                    .tooltip(document.path.clone())
                    .button_style(GhostButtonStyle),
                )
                .semantic_background(SemanticColorRef::accent_muted().opacity(if active {
                    1.0
                } else {
                    0.0
                }))
                .corner_radius(8.0)
                .id(("sync-document", document.uid.clone())),
            ));
        }
        if entries.is_empty() {
            entries.push(note(if on_cloud && !cloud_loaded {
                "刷新清单以查看云端文稿"
            } else if on_cloud && remote.is_none() {
                "这个资料库尚无云端备份"
            } else if search.is_empty() {
                "没有可显示的文稿"
            } else {
                "没有匹配的文稿"
            }));
        }
        let list = vstack(
            8.0,
            Align::Stretch,
            vec![
                AnyView::new(TextField::new("搜索标题或路径", query.binding())),
                note(format!("{} 篇文稿", visible.len())),
                AnyView::new(
                    ScrollView::new(ScrollAxis::Vertical, vstack(2.0, Align::Stretch, entries))
                        .flex_with(1.0, 1.0, Some(0.0)),
                ),
            ],
        )
        .frame_width(218.0);

        let mut details = Vec::new();
        if let Some(document) = current {
            details.push(AnyView::new(
                Text::new(title(document))
                    .truncated()
                    .semantic_font(SemanticFontRef::headline()),
            ));
            details.push(note(&document.path));
            let status = if document.deleted {
                if on_cloud {
                    "已从云端清单删除"
                } else {
                    "在本地回收站，可从侧栏恢复"
                }
            } else if on_cloud || cloud_loaded {
                match counterpart.get(document.uid.as_str()) {
                    Some(other) if document.same_version(other) => "本地和云端一致",
                    Some(other) if other.deleted => "另一端已删除",
                    Some(_) => "本地和云端版本不同",
                    None if on_cloud => "仅云端有此文稿",
                    None => "仅本地有此文稿",
                }
            } else {
                ""
            };
            if !status.is_empty() {
                details.push(note(status));
            }
            if !document.deleted {
                let mut cache = preview_cache.borrow_mut();
                if cache
                    .as_ref()
                    .is_none_or(|(hash, _)| hash != &document.hash)
                {
                    *cache = Some((document.hash.clone(), TextBuffer::new(&document.content)));
                }
                let preview = cache
                    .as_ref()
                    .map(|(_, buffer)| buffer.clone())
                    .unwrap_or_default();
                details.push(AnyView::new(
                    TextField::new("空白文稿", Binding::new(move || preview.clone(), |_| {}))
                        .multiline()
                        .behavior(TextFieldBehavior {
                            read_only: true,
                            ..Default::default()
                        })
                        .id(("sync-preview", on_cloud, document.uid.clone()))
                        .flex_with(1.0, 1.0, Some(0.0)),
                ));
                if let Some(expected) = confirm.get().filter(|entry| entry.same_version(document)) {
                    details.push(note(if on_cloud {
                        "删除云端版本后，下一次同步会将与该版本一致的本地文稿移入回收站。"
                    } else {
                        "文稿会移入本地回收站。同步时会将这次删除传到云端。"
                    }));
                    let model = self.model.clone();
                    let gist_id = remote.as_ref().map(|value| value.gist_id.clone());
                    details.push(hstack(
                        6.0,
                        Align::Center,
                        vec![
                            spacer(),
                            AnyView::new(Button::title("取消", move || confirm.set(None))),
                            AnyView::new(
                                Button::title("确认删除", move || {
                                    confirm.set(None);
                                    if on_cloud {
                                        if let Some(id) = &gist_id {
                                            model.delete_sync_remote(id.clone(), expected.clone());
                                        }
                                    } else {
                                        model.delete_sync_local(&expected);
                                    }
                                })
                                .role(ButtonRole::Destructive)
                                .enabled(!busy),
                            ),
                        ],
                    ));
                } else {
                    let expected = SyncDocument {
                        content: String::new(),
                        ..document.clone()
                    };
                    details.push(hstack(
                        0.0,
                        Align::Center,
                        vec![
                            spacer(),
                            AnyView::new(
                                Button::title(
                                    if on_cloud {
                                        "删除云端版本"
                                    } else {
                                        "移入本地回收站"
                                    },
                                    move || {
                                        confirm.set(Some(expected.clone()));
                                    },
                                )
                                .enabled(!busy)
                                .button_style(GhostButtonStyle),
                            ),
                        ],
                    ));
                }
            }
        } else {
            details.push(note("选择文稿以预览正文"));
        }
        let status = Text::new(self.model.sync_status.get())
            .line_limit(Some(3))
            .semantic_font(SemanticFontRef::caption())
            .semantic_foreground(if self.model.sync_failed.get() {
                SemanticColorRef::error()
            } else {
                SemanticColorRef::on_surface_variant()
            });
        let rows = vstack(
            9.0,
            Align::Stretch,
            vec![
                toolbar,
                AnyView::new(
                    hstack(
                        14.0,
                        Align::Stretch,
                        vec![
                            AnyView::new(list),
                            AnyView::new(vstack(6.0, Align::Stretch, details).flex_with(
                                1.0,
                                1.0,
                                Some(0.0),
                            )),
                        ],
                    )
                    .flex_with(1.0, 1.0, Some(0.0)),
                ),
                AnyView::new(status),
            ],
        )
        .frame_height(450.0);

        let close = self.model.clone();
        let check = self.model.clone();
        let sync = self.model.clone();
        let footer = hstack(
            8.0,
            Align::Center,
            vec![
                note(self.model.sync_login.get()),
                spacer(),
                AnyView::new(
                    Button::title("检查登录", move || check.check_sync_login())
                        .enabled(!self.model.sync_checking.get() && !busy),
                ),
                AnyView::new(Button::title("关闭", move || close.close_sync())),
                AnyView::new(
                    Button::title("立即同步", move || {
                        confirm.set(None);
                        sync.sync_now();
                    })
                    .enabled(!busy)
                    .button_style(PrimaryButtonStyle),
                ),
            ],
        );
        let close = self.model.clone();
        AnyView::new(
            Dialog::new(
                Binding::new(
                    || true,
                    move |open| {
                        if !open {
                            close.close_sync();
                        }
                    },
                ),
                "云同步",
                rows,
            )
            .footer(footer)
            .configure(|options| {
                options.modal.fits_content_height = true;
                options.modal.geometry.width = 820.0;
            }),
        )
    }
}
