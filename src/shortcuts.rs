//! Keyboard commands. Primary = ⌘ on Apple platforms, Ctrl elsewhere. On
//! macOS the native menu bar claims the same keys first; this table covers
//! every other platform and the in-window fallback. No command animates.

use crate::model::Model;
use cove_ui::composition::Scancode;
use cove_ui::platform::KeyEvent;
use ruwriter_core::Collection;

const BACKSLASH: u32 = 49;

pub fn handle(event: &KeyEvent, model: &Model) -> bool {
    if event.is_repeat {
        return false;
    }
    let modifiers = event.modifiers;
    if event.scancode == Scancode::ESCAPE && !modifiers.has_gui() && !modifiers.has_ctrl() {
        return model.leave_focus();
    }
    if !(modifiers.has_gui() || modifiers.has_ctrl()) {
        return false;
    }
    let shift = modifiers.has_shift();
    match event.scancode {
        Scancode::N if !shift => model.create(),
        Scancode::O if !shift => model.open_picker(),
        Scancode::S if !shift => model.save(),
        Scancode::E if shift => model.export(),
        Scancode::P if !shift => model.toggle_preview(),
        Scancode::F if shift => model.toggle_focus(),
        Scancode::K if !shift => model.toggle_palette(),
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
