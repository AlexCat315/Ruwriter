//! The application menu bar. Installed where the platform has one (macOS);
//! the installer is a no-op elsewhere, where the toolbar, the ⌘K palette and
//! the shortcut table expose the same commands.
//!
//! AppKit reads check marks from the installed snapshot, so the bar is
//! reinstalled whenever a checked setting changes.

use std::cell::RefCell;
use std::rc::Rc;

use cove_ui::hosts::desktop::application::composition::AppDisplayHandleHolder;
use cove_ui::hosts::desktop::window::native_menu::{
    NativeApplicationMenuTitles, NativeMenu, NativeMenuAction, NativeMenuBar, NativeMenuItem,
    NativeMenuKeyModifiers,
};
use cove_ui::platform::NativeStandardMenuTitles;
use ruwriter_core::{AppearancePreference, Collection};

use crate::formatting::Command;
use crate::model::Model;

/// Everything a menu title or check mark depends on.
type MenuKey = (
    bool,
    bool,
    bool,
    AppearancePreference,
    Collection,
    bool,
    bool,
    ruwriter_core::WritingMode,
    bool,
    bool,
    ruwriter_core::LineSpacing,
    ruwriter_core::ColumnWidth,
);

pub fn sync(model: &Model, installed: &Rc<RefCell<Option<MenuKey>>>) {
    let options = model.options();
    let settings = model.settings();
    let pinned = model.active_document().is_some_and(|d| d.pinned);
    let key = (
        options.preview,
        options.focus,
        options.sidebar,
        settings.appearance,
        settings.collection,
        pinned,
        settings.markdown_syntax,
        settings.writing_mode,
        settings.paragraph_indent,
        settings.typewriter_scrolling,
        settings.line_spacing,
        settings.column_width,
    );
    if installed.borrow().as_ref() == Some(&key) {
        return;
    }
    let Some(handle) = AppDisplayHandleHolder::current() else {
        return;
    };
    handle.install_native_menu_bar(&build(model, key));
    *installed.borrow_mut() = Some(key);
}

fn action(model: &Model, title: &str, run: impl Fn(&Model) + 'static) -> NativeMenuAction {
    let target = model.clone();
    NativeMenuAction::new(title, move || run(&target))
}

fn primary(key: &str) -> (String, NativeMenuKeyModifiers) {
    (key.to_owned(), NativeMenuKeyModifiers::primary())
}

fn primary_shift(key: &str) -> (String, NativeMenuKeyModifiers) {
    (
        key.to_owned(),
        NativeMenuKeyModifiers::primary().union(NativeMenuKeyModifiers::SHIFT),
    )
}

fn item(
    action: NativeMenuAction,
    shortcut: Option<(String, NativeMenuKeyModifiers)>,
) -> NativeMenuItem {
    NativeMenuItem::Action(match shortcut {
        Some((key, modifiers)) => action.with_shortcut(key, modifiers),
        None => action,
    })
}

fn build(model: &Model, key: MenuKey) -> NativeMenuBar {
    let (
        preview,
        focus,
        sidebar,
        appearance,
        collection,
        pinned,
        syntax,
        writing,
        indent,
        typewriter,
        spacing,
        width,
    ) = key;
    let markdown = writing == ruwriter_core::WritingMode::Markdown;
    let file = NativeMenu::new(
        "文件",
        vec![
            item(action(model, "新建文稿", Model::create), Some(primary("n"))),
            item(
                action(model, "新建文件夹", Model::new_folder_here),
                Some(primary_shift("n")),
            ),
            item(
                action(model, "打开文件…", Model::open_picker),
                Some(primary("o")),
            ),
            item(
                action(model, "打开文件夹作为资料库…", Model::open_library_picker),
                Some(primary_shift("o")),
            ),
            NativeMenuItem::Separator,
            item(action(model, "保存", Model::save), Some(primary("s"))),
            item(
                action(model, "保存快照", Model::snapshot_now),
                Some(primary_shift("s")),
            ),
            item(action(model, "历史版本…", Model::open_history), None),
            item(
                action(
                    model,
                    if markdown {
                        "导出 Markdown…"
                    } else {
                        "导出文稿…"
                    },
                    Model::export,
                ),
                Some(primary_shift("e")),
            ),
            NativeMenuItem::Separator,
            item(action(model, "文稿信息…", Model::edit_active_info), None),
            item(
                action(
                    model,
                    if pinned { "取消置顶" } else { "置顶" },
                    move |model| {
                        if let Some(id) = model.active_document().map(|d| d.id) {
                            model.set_pinned(&id, !pinned);
                        }
                    },
                ),
                None,
            ),
            item(action(model, "移到废纸篓", Model::trash_active), None),
        ],
    );

    let format = NativeMenu::new(
        "格式",
        vec![
            item(
                action(model, "加粗", |model| model.format(Command::Bold)),
                Some(primary("b")),
            ),
            item(
                action(model, "斜体", |model| model.format(Command::Italic)),
                Some(primary("i")),
            ),
            item(
                action(model, "删除线", |model| {
                    model.format(Command::Strikethrough)
                }),
                Some(primary_shift("x")),
            ),
            item(
                action(model, "行内代码", |model| model.format(Command::Code)),
                Some(primary("e")),
            ),
            item(
                action(model, "链接", |model| model.format(Command::Link)),
                Some(primary_shift("k")),
            ),
            NativeMenuItem::Separator,
            item(
                action(model, "标题", |model| model.format(Command::Heading)),
                Some(primary_shift("h")),
            ),
            item(
                action(model, "无序列表", |model| {
                    model.format(Command::BulletList)
                }),
                Some(primary_shift("8")),
            ),
            item(
                action(model, "有序列表", |model| {
                    model.format(Command::NumberedList)
                }),
                Some(primary_shift("7")),
            ),
            item(
                action(model, "引用", |model| model.format(Command::Quote)),
                Some(primary_shift("9")),
            ),
        ],
    );

    let appearance_menu = NativeMenu::new(
        "外观",
        [
            (AppearancePreference::System, "跟随系统"),
            (AppearancePreference::Light, "浅色"),
            (AppearancePreference::Dark, "深色"),
        ]
        .into_iter()
        .map(|(choice, title)| {
            item(
                action(model, title, move |model| model.set_appearance(choice))
                    .with_selected(choice == appearance),
                None,
            )
        })
        .collect(),
    );
    let view = NativeMenu::new(
        "显示",
        vec![
            item(
                action(model, "查找和替换…", Model::open_find),
                Some(primary("f")),
            ),
            item(
                action(model, "下一个匹配", |model| model.find_step(1)),
                Some(primary("g")),
            ),
            item(
                action(model, "上一个匹配", |model| model.find_step(-1)),
                Some(primary_shift("g")),
            ),
            NativeMenuItem::Separator,
            item(
                action(model, "侧栏", Model::toggle_sidebar).with_selected(sidebar && !focus),
                Some(primary("\\")),
            ),
            item(
                action(model, "大纲", Model::toggle_outline),
                Some(primary_shift("l")),
            ),
            item(
                action(model, "预览", Model::toggle_preview).with_selected(preview),
                Some(primary("p")),
            ),
            item(
                action(model, "专注模式", Model::toggle_focus).with_selected(focus),
                Some(primary_shift("f")),
            ),
            item(
                action(model, "Markdown 高亮", Model::toggle_syntax).with_selected(syntax),
                None,
            ),
            item(
                action(model, "Markdown 模式", Model::toggle_writing_mode).with_selected(markdown),
                None,
            ),
            item(
                action(model, "段首缩进", Model::toggle_paragraph_indent).with_selected(indent),
                None,
            ),
            item(
                action(model, "打字机滚动", Model::toggle_typewriter_scrolling)
                    .with_selected(typewriter),
                None,
            ),
            NativeMenuItem::Separator,
            item(
                action(model, "放大文字", |model| model.adjust_font_size(1)),
                Some(primary("=")),
            ),
            item(
                action(model, "缩小文字", |model| model.adjust_font_size(-1)),
                Some(primary("-")),
            ),
            item(
                action(model, "实际大小", Model::reset_font_size),
                Some(primary("0")),
            ),
            NativeMenuItem::Separator,
            item(
                action(model, "紧凑行距", |model| {
                    model.set_line_spacing(ruwriter_core::LineSpacing::Compact)
                })
                .with_selected(
                    spacing == ruwriter_core::LineSpacing::Compact,
                ),
                None,
            ),
            item(
                action(model, "标准行距", |model| {
                    model.set_line_spacing(ruwriter_core::LineSpacing::Normal)
                })
                .with_selected(
                    spacing == ruwriter_core::LineSpacing::Normal,
                ),
                None,
            ),
            item(
                action(model, "宽松行距", |model| {
                    model.set_line_spacing(ruwriter_core::LineSpacing::Loose)
                })
                .with_selected(
                    spacing == ruwriter_core::LineSpacing::Loose,
                ),
                None,
            ),
            NativeMenuItem::Separator,
            item(
                action(model, "窄栏", |model| {
                    model.set_column_width(ruwriter_core::ColumnWidth::Narrow)
                })
                .with_selected(width == ruwriter_core::ColumnWidth::Narrow),
                None,
            ),
            item(
                action(model, "标准栏宽", |model| {
                    model.set_column_width(ruwriter_core::ColumnWidth::Normal)
                })
                .with_selected(width == ruwriter_core::ColumnWidth::Normal),
                None,
            ),
            item(
                action(model, "宽栏", |model| {
                    model.set_column_width(ruwriter_core::ColumnWidth::Wide)
                })
                .with_selected(width == ruwriter_core::ColumnWidth::Wide),
                None,
            ),
            NativeMenuItem::Separator,
            NativeMenuItem::Submenu(appearance_menu),
            NativeMenuItem::Separator,
            item(
                action(model, "命令面板…", Model::toggle_palette),
                Some(primary("k")),
            ),
        ],
    );
    let library = NativeMenu::new(
        "资料库",
        Collection::ALL
            .iter()
            .enumerate()
            .map(|(index, choice)| {
                let choice = *choice;
                item(
                    action(model, choice.title(), move |model| {
                        model.set_collection(choice)
                    })
                    .with_selected(choice == collection),
                    Some(primary(&(index + 1).to_string())),
                )
            })
            .collect(),
    );

    let mut bar = NativeMenuBar::new("Ruwriter", Vec::new(), vec![file, format, view, library]);
    bar.application_titles = NativeApplicationMenuTitles {
        services: "服务".to_owned(),
        hide: "隐藏 Ruwriter".to_owned(),
        hide_others: "隐藏其他".to_owned(),
        show_all: "全部显示".to_owned(),
        quit: "退出 Ruwriter".to_owned(),
    };
    bar.standard_titles(NativeStandardMenuTitles {
        edit: "编辑".to_owned(),
        undo: "撤销".to_owned(),
        redo: "重做".to_owned(),
        cut: "剪切".to_owned(),
        copy: "拷贝".to_owned(),
        paste: "粘贴".to_owned(),
        delete: "删除".to_owned(),
        select_all: "全选".to_owned(),
        window: "窗口".to_owned(),
        minimize: "最小化".to_owned(),
        zoom: "缩放".to_owned(),
        bring_all_to_front: "前置全部窗口".to_owned(),
    })
}
