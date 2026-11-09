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

use crate::model::Model;

/// Everything a menu title or check mark depends on.
type MenuKey = (bool, bool, bool, AppearancePreference, Collection, bool);

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
    let (preview, focus, sidebar, appearance, collection, pinned) = key;
    let file = NativeMenu::new(
        "文件",
        vec![
            item(action(model, "新建文稿", Model::create), Some(primary("n"))),
            item(
                action(model, "打开…", Model::open_picker),
                Some(primary("o")),
            ),
            NativeMenuItem::Separator,
            item(action(model, "保存", Model::save), Some(primary("s"))),
            item(
                action(model, "导出 Markdown…", Model::export),
                Some(primary_shift("e")),
            ),
            NativeMenuItem::Separator,
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
                action(model, "侧栏", Model::toggle_sidebar).with_selected(sidebar && !focus),
                Some(primary("\\")),
            ),
            item(
                action(model, "预览", Model::toggle_preview).with_selected(preview),
                Some(primary("p")),
            ),
            item(
                action(model, "专注模式", Model::toggle_focus).with_selected(focus),
                Some(primary_shift("f")),
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

    let mut bar = NativeMenuBar::new("Ruwriter", Vec::new(), vec![file, view, library]);
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
