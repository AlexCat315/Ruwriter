use crate::actions::Actions;
use cove_ui::composition::Scancode;
use cove_ui::platform::KeyEvent;

/// Primary = ⌘ on Apple platforms, Ctrl elsewhere. No animation or transition
/// is attached to any of these commands.
pub fn handle(event: &KeyEvent, actions: &Actions) -> bool {
    if event.is_repeat || !(event.modifiers.has_gui() || event.modifiers.has_ctrl()) {
        return false;
    }
    match event.scancode {
        Scancode::N => actions.create(),
        Scancode::O => actions.open_picker(),
        Scancode::S => actions.save(),
        Scancode::P if !event.modifiers.has_shift() => actions.toggle_preview(),
        Scancode::F if event.modifiers.has_shift() => actions.toggle_focus(),
        _ => return false,
    }
    true
}
