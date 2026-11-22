//! ⌘K: every command and every document, searchable from the keyboard.

use std::rc::Rc;

use cove_ui::compose::primitives::media::ui_icon_catalog as icons;
use cove_ui::compose::primitives::overlays::{CommandPalette, PaletteCommand};
use cove_ui::composition::{AnyView, EmptyView, Scope, View};
use cove_ui::imaging::BundleImageResource;
use ruwriter_core::{AppearancePreference, Collection, SortOrder};

use super::sidebar::{appearance_label, collection_icon};
use crate::formatting::Command;
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
        let _ = self.model.buffer.get();
        let model = &self.model;
        let options = model.options();
        let syntax = model.settings().markdown_syntax;
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
                "导入到资料库…",
                "文稿",
                icons::file::folder_open(),
                Rc::new(Model::import_picker),
            )
            .keywords(["import", "daoru", "txt", "md"]),
            command(
                "AI 助手…",
                "写作",
                icons::status::sparkles(),
                Rc::new(Model::open_ai),
            )
            .keywords(["agent", "ai", "润色", "摘要", "提纲"]),
            command(
                "AI 续写…",
                "写作",
                icons::status::sparkles(),
                Rc::new(Model::open_ai_continuation),
            )
            .keywords(["继续写", "接着写", "xuxie", "continue"]),
            command(
                "云同步…",
                "文稿",
                icons::device::cloud_upload(),
                Rc::new(Model::open_sync),
            )
            .keywords(["sync", "gist", "cloud", "beifen", "yun", "tongbu"]),
            command(
                "文稿信息…",
                "文稿",
                icons::file::file_text(),
                Rc::new(Model::edit_active_info),
            )
            .keywords([
                "synopsis", "status", "tag", "tags", "gaojian", "zhuyao", "biaoti",
            ]),
            command(
                "保存快照",
                "文稿",
                icons::action::pencil(),
                Rc::new(Model::snapshot_now),
            )
            .keywords(["snapshot", "history", "version", "kuaizhao", "lishi"]),
            command(
                "历史版本…",
                "文稿",
                icons::data::clock(),
                Rc::new(Model::open_history),
            )
            .keywords([
                "history", "version", "snapshot", "lishi", "huifu", "kuaizhao",
            ]),
            command(
                if model.settings().writing_mode == ruwriter_core::WritingMode::Novel {
                    "新建文稿使用 Markdown"
                } else {
                    "新建文稿使用 TXT"
                },
                "写作模式",
                icons::text::type_(),
                Rc::new(Model::toggle_writing_mode),
            )
            .keywords(["novel", "markdown", "xiaoshuo", "moshi", "mode"]),
            command(
                if model.paragraph_indent() {
                    "关闭 TXT 自动显示段首缩进"
                } else {
                    "开启 TXT 自动显示段首缩进"
                },
                "排版",
                icons::text::type_(),
                Rc::new(Model::toggle_paragraph_indent),
            )
            .keywords(["indent", "suojin", "duanshou", "paragraph"]),
            command(
                "窄栏",
                "排版",
                icons::layout::columns_2(),
                Rc::new(|model: &Model| model.set_column_width(ruwriter_core::ColumnWidth::Narrow)),
            )
            .keywords(["width", "column", "lan", "zhaian", "banxin"]),
            command(
                "标准栏宽",
                "排版",
                icons::layout::columns_2(),
                Rc::new(|model: &Model| model.set_column_width(ruwriter_core::ColumnWidth::Normal)),
            )
            .keywords(["width", "column", "biaozhun", "banxin"]),
            command(
                "宽栏",
                "排版",
                icons::layout::columns_2(),
                Rc::new(|model: &Model| model.set_column_width(ruwriter_core::ColumnWidth::Wide)),
            )
            .keywords(["width", "column", "kuan", "kuanlan", "banxin"]),
            command(
                if model.line_spacing() == ruwriter_core::LineSpacing::Compact {
                    "标准行距"
                } else {
                    "紧凑行距"
                },
                "排版",
                icons::layout::columns_2(),
                Rc::new(|model: &Model| {
                    model.set_line_spacing(ruwriter_core::LineSpacing::Compact)
                }),
            )
            .keywords(["line", "spacing", "hangju", "jin cou"]),
            command(
                if model.line_spacing() == ruwriter_core::LineSpacing::Loose {
                    "标准行距"
                } else {
                    "宽松行距"
                },
                "排版",
                icons::layout::columns_2(),
                Rc::new(|model: &Model| model.set_line_spacing(ruwriter_core::LineSpacing::Loose)),
            )
            .keywords(["line", "spacing", "hangju", "kuansong"]),
            command(
                if model.typewriter_scrolling() {
                    "关闭打字机滚动"
                } else {
                    "开启打字机滚动"
                },
                "排版",
                icons::layout::columns_2(),
                Rc::new(Model::toggle_typewriter_scrolling),
            )
            .keywords([
                "typewriter",
                "scroll",
                "center",
                "daziji",
                "gundong",
                "juzhong",
            ]),
            command(
                if model.writing_mode() == ruwriter_core::WritingMode::Novel {
                    "导出文稿…"
                } else {
                    "导出 Markdown…"
                },
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
                "查找和替换…",
                "编辑",
                icons::action::search(),
                Rc::new(Model::open_find),
            )
            .keywords(["find", "search", "replace", "chazhao", "tihuan"]),
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
                if options.outline {
                    "隐藏大纲"
                } else {
                    "显示大纲"
                },
                "显示",
                icons::layout::panel_right(),
                Rc::new(Model::toggle_outline),
            )
            .keywords(["outline", "dagang", "structure"]),
            command(
                if syntax {
                    "关闭 Markdown 高亮（纯文本）"
                } else {
                    "开启 Markdown 高亮"
                },
                "显示",
                icons::editor::code(),
                Rc::new(Model::toggle_syntax),
            )
            .keywords(["syntax", "markdown", "plain", "gaoliang", "chunwenben"]),
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
        for sort in SortOrder::ALL {
            items.push(
                command(
                    &format!("排序：{}", sort.title()),
                    "显示",
                    icons::navigation::chevrons_up_down(),
                    Rc::new(move |model: &Model| model.set_sort(sort)),
                )
                .keywords(["sort", "paixu"]),
            );
        }
        let formats: [(&str, BundleImageResource, Command, [&str; 2]); 11] = [
            (
                "写入两格段首空格",
                icons::text::type_(),
                Command::IndentParagraphs,
                ["indent", "duanshou"],
            ),
            (
                "删除文件中的段首空格",
                icons::text::type_(),
                Command::UnindentParagraphs,
                ["unindent", "duanshou"],
            ),
            (
                "加粗",
                icons::text::bold(),
                Command::Bold,
                ["bold", "jiacu"],
            ),
            (
                "斜体",
                icons::text::italic(),
                Command::Italic,
                ["italic", "xieti"],
            ),
            (
                "删除线",
                icons::text::type_(),
                Command::Strikethrough,
                ["strikethrough", "shanchuxian"],
            ),
            (
                "行内代码",
                icons::editor::code(),
                Command::Code,
                ["code", "daima"],
            ),
            (
                "链接",
                icons::action::link(),
                Command::Link,
                ["link", "lianjie"],
            ),
            (
                "标题",
                icons::text::heading(),
                Command::Heading,
                ["heading", "biaoti"],
            ),
            (
                "无序列表",
                icons::text::list(),
                Command::BulletList,
                ["list", "liebiao"],
            ),
            (
                "有序列表",
                icons::text::list_ordered(),
                Command::NumberedList,
                ["numbered", "youxu"],
            ),
            (
                "引用",
                icons::shape::message_square(),
                Command::Quote,
                ["quote", "yinyong"],
            ),
        ];
        for (title, icon, format_command, keywords) in formats {
            if format_command.is_plain_text()
                != (model.writing_mode() == ruwriter_core::WritingMode::Novel)
            {
                continue;
            }
            items.push(
                command(
                    title,
                    "格式",
                    icon,
                    Rc::new(move |model: &Model| model.format(format_command)),
                )
                .keywords(keywords),
            );
        }
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
        items.push(
            command(
                "新建文件夹",
                "资料库",
                icons::file::folder_plus(),
                Rc::new(Model::new_folder_here),
            )
            .keywords(["folder", "new folder", "wenjianjia"]),
        );
        items.push(
            command(
                "打开文件夹作为资料库…",
                "资料库",
                icons::file::folder_open(),
                Rc::new(Model::open_library_picker),
            )
            .keywords(["vault", "library", "open folder", "ziliaoku"]),
        );
        let folders = model
            .read(|session| session.library.folders.clone())
            .unwrap_or_default();
        for folder in folders {
            let id = folder.id.clone();
            let path = folder.id.replace('/', " / ");
            items.push(
                command(
                    &format!("文件夹：{}", folder.name),
                    "跳转到文件夹",
                    icons::file::folder(),
                    Rc::new(move |model: &Model| model.select_folder(&id)),
                )
                .id(format!("folder-{}", folder.id))
                .subtitle(path),
            );
        }
        for heading in model.outline() {
            let offset = heading.offset;
            let title = heading.title;
            let level = heading.level;
            items.push(
                command(
                    &title,
                    "跳转到大纲",
                    icons::text::heading(),
                    Rc::new(move |model: &Model| model.jump_to_heading(offset)),
                )
                .id(format!("outline-{offset}"))
                .subtitle(format!("H{level} · 第 {} 字", offset + 1)),
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
