//! ⌘K: every command and every document, searchable from the keyboard.

use std::rc::Rc;

use cove_ui::compose::primitives::media::ui_icon_catalog as icons;
use cove_ui::compose::primitives::overlays::{CommandPalette, PaletteCommand};
use cove_ui::composition::{AnyView, EmptyView, Scope, View};
use cove_ui::imaging::BundleImageResource;
use ruwriter_core::{AppearancePreference, Collection};

use super::sidebar::{appearance_label, collection_icon};
use crate::model::Model;

pub struct Palette {
    pub model: Model,
}

impl View for Palette {
    fn body(&self, _cx: &Scope) -> AnyView {
        if !self.model.palette.get() {
            return AnyView::new(EmptyView);
        }
        let _ = self.model.revision.get();
        let model = &self.model;
        let options = model.options();
        let command =
            |title: &str, group: &str, icon: BundleImageResource, action: Rc<dyn Fn(&Model)>| {
                let target = model.clone();
                PaletteCommand::new(title, move || action(&target))
                    .group(group)
                    .icon(icon)
            };

        let mut items = vec![
            command(
                "新建文稿",
                "文稿",
                icons::action::pencil(),
                Rc::new(Model::create),
            )
            .keywords(["new", "xinjian"]),
            command(
                "打开文件…",
                "文稿",
                icons::file::folder_open(),
                Rc::new(Model::open_picker),
            )
            .keywords(["open", "dakai"]),
            command(
                "导出 Markdown…",
                "文稿",
                icons::action::share(),
                Rc::new(Model::export),
            )
            .keywords(["export", "save as", "daochu"]),
            command(
                "移到废纸篓",
                "文稿",
                icons::action::trash(),
                Rc::new(Model::trash_active),
            )
            .keywords(["delete", "trash", "shanchu"]),
            command(
                if options.preview {
                    "关闭预览"
                } else {
                    "预览"
                },
                "显示",
                icons::layout::columns_2(),
                Rc::new(Model::toggle_preview),
            )
            .keywords(["preview", "yulan"]),
            command(
                if options.focus {
                    "退出专注模式"
                } else {
                    "专注模式"
                },
                "显示",
                icons::action::focus(),
                Rc::new(Model::toggle_focus),
            )
            .keywords(["focus", "zen", "zhuanzhu"]),
            command(
                "显示或隐藏侧栏",
                "显示",
                icons::layout::panel_left(),
                Rc::new(Model::toggle_sidebar),
            )
            .keywords(["sidebar", "cebian"]),
            command(
                "放大文字",
                "显示",
                icons::action::zoom_in(),
                Rc::new(|model: &Model| model.adjust_font_size(1)),
            )
            .keywords(["bigger", "font"]),
            command(
                "缩小文字",
                "显示",
                icons::action::zoom_out(),
                Rc::new(|model: &Model| model.adjust_font_size(-1)),
            )
            .keywords(["smaller", "font"]),
        ];
        for appearance in [
            AppearancePreference::System,
            AppearancePreference::Light,
            AppearancePreference::Dark,
        ] {
            let (title, icon) = appearance_label(appearance);
            items.push(
                command(
                    &format!("外观：{title}"),
                    "外观",
                    icon,
                    Rc::new(move |model: &Model| model.set_appearance(appearance)),
                )
                .keywords(["theme", "appearance", "dark", "light", "waiguan"]),
            );
        }
        for collection in Collection::ALL {
            items.push(command(
                collection.title(),
                "资料库",
                collection_icon(collection),
                Rc::new(move |model: &Model| model.set_collection(collection)),
            ));
        }
        let documents = model
            .read(|session| session.documents())
            .unwrap_or_default();
        for document in documents {
            let id = document.id.clone();
            items.push(
                command(
                    document.display_title(),
                    "跳转到文稿",
                    icons::file::file_text(),
                    Rc::new(move |model: &Model| model.select(&id)),
                )
                .id(format!("doc-{}", document.id))
                .subtitle(document.excerpt.chars().take(40).collect::<String>()),
            );
        }

        AnyView::new(
            CommandPalette::new(model.palette.binding(), items)
                .placeholder("搜索命令或文稿…")
                .empty_state("没有结果", "换个关键词试试"),
        )
    }
}
