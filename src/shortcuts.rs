//! Keyboard commands. Primary = ⌘ on Apple platforms, Ctrl elsewhere. On
//! macOS the native menu bar claims the same keys first; this table covers
//! every other platform and the in-window fallback. No command animates.

use crate::formatting::Command;
use std::cell::RefCell;
use std::rc::Rc;

use crate::model::Model;
use cove_ui::compose::primitives::text::text_field::FieldState;
use cove_ui::compose::primitives::text::text_field::text_field_layout_engine::FIELD_STATE_KEY;
use cove_ui::composition::Scancode;
use cove_ui::core::scene::FocusChainHolder;
use cove_ui::platform::{KeyEvent, KeyModifiers};
use ruwriter_core::Collection;

const BACKSLASH: u32 = 49;
// USB HID codes Cove's Scancode table does not name yet.
const G: u32 = 10;
const H: u32 = 11;
const I: u32 = 12;

pub fn has_primary(modifiers: KeyModifiers) -> bool {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        modifiers.has_gui() && !modifiers.has_ctrl() && !modifiers.has_alt()
    } else {
        modifiers.has_ctrl() && !modifiers.has_gui() && !modifiers.has_alt()
    }
}

pub fn has_composition() -> bool {
    FocusChainHolder::current_focused()
        .and_then(|node| node.attachment::<Rc<RefCell<FieldState>>>(FIELD_STATE_KEY))
        .is_some_and(|state| state.borrow().composition.is_active())
}

pub fn handle(event: &KeyEvent, model: &Model) -> bool {
    if event.is_repeat {
        return false;
    }
    let modifiers = event.modifiers;
    if event.scancode == Scancode::ESCAPE && modifiers.is_empty() {
        return !has_composition() && model.escape();
    }
    if !has_primary(modifiers) {
        return false;
    }
    let shift = modifiers.has_shift();
    match event.scancode {
        Scancode::N if !shift => model.create(),
        Scancode::N => model.new_folder_here(),
        Scancode::O if shift => model.open_library_picker(),
        Scancode::O if !shift => model.open_picker(),
        Scancode::S if shift => model.snapshot_now(),
        Scancode::S if !shift => model.save(),
        Scancode::E if shift => model.export(),
        Scancode::E => model.format(Command::Code),
        Scancode::P if !shift => model.toggle_preview(),
        Scancode::F if shift => model.toggle_focus(),
        Scancode::F => model.open_find(),
        G if !shift => model.find_step(1),
        G => model.find_step(-1),
        Scancode::K if shift => model.format(Command::Link),
        Scancode::K => model.toggle_palette(),
        Scancode::L if shift => model.toggle_outline(),
        Scancode::B if !shift => model.format(Command::Bold),
        I if shift => model.import_picker(),
        I if !shift => model.format(Command::Italic),
        Scancode::X if shift => model.format(Command::Strikethrough),
        H if shift => model.format(Command::Heading),
        Scancode::DIGIT7 if shift => model.format(Command::NumberedList),
        Scancode::DIGIT8 if shift => model.format(Command::BulletList),
        Scancode::DIGIT9 if shift => model.format(Command::Quote),
        BACKSLASH if !shift => model.toggle_sidebar(),
        Scancode::EQUALS => model.adjust_font_size(1),
        Scancode::MINUS if !shift => model.adjust_font_size(-1),
        Scancode::DIGIT0 if !shift => model.reset_font_size(),
        Scancode::DIGIT1 if !shift => model.set_collection(Collection::All),
        Scancode::DIGIT2 if !shift => model.set_collection(Collection::Pinned),
        Scancode::DIGIT3 if !shift => model.set_collection(Collection::Recent),
        Scancode::DIGIT4 if !shift => model.set_collection(Collection::Trash),
        _ => return false,
    }
    true
}
