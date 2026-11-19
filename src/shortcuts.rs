//! Keyboard commands. Primary = ⌘ on Apple platforms, Ctrl elsewhere. On
//! macOS the native menu bar claims the same keys first; this table covers
//! every other platform and the in-window fallback. No command animates.

use crate::formatting::Command;
use crate::model::Model;
use cove_ui::composition::Scancode;
use cove_ui::platform::KeyEvent;
use ruwriter_core::Collection;

const BACKSLASH: u32 = 49;
// USB HID codes Cove's Scancode table does not name yet.
const G: u32 = 10;
const H: u32 = 11;
const I: u32 = 12;
const L: u32 = 15;

pub fn handle(event: &KeyEvent, model: &Model) -> bool {
    if event.is_repeat {
        return false;
    }
    let modifiers = event.modifiers;
    if event.scancode == Scancode::ESCAPE && !modifiers.has_gui() && !modifiers.has_ctrl() {
        return model.escape();
    }
    if !(modifiers.has_gui() || modifiers.has_ctrl()) {
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
        L if shift => model.toggle_outline(),
        Scancode::B if !shift => model.format(Command::Bold),
        I if !shift => model.format(Command::Italic),
        Scancode::X if shift => model.format(Command::Strikethrough),
        H if shift => model.format(Command::Heading),
        Scancode::DIGIT7 if shift => model.format(Command::NumberedList),
        Scancode::DIGIT8 if shift => model.format(Command::BulletList),
        Scancode::DIGIT9 if shift => model.format(Command::Quote),
        BACKSLASH => model.toggle_sidebar(),
        Scancode::EQUALS => model.adjust_font_size(1),
        Scancode::MINUS => model.adjust_font_size(-1),
        Scancode::DIGIT0 => model.reset_font_size(),
        Scancode::DIGIT1 => model.set_collection(Collection::All),
        Scancode::DIGIT2 => model.set_collection(Collection::Pinned),
        Scancode::DIGIT3 => model.set_collection(Collection::Recent),
        Scancode::DIGIT4 => model.set_collection(Collection::Trash),
        _ => return false,
    }
    true
}
