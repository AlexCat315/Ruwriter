use crate::views::RootView;
use cove_ui::ApplicationLaunch;

pub fn application() -> ApplicationLaunch {
    ApplicationLaunch::new(RootView)
        .configure(|config| {
            config.window.title = "Ruwriter".into();
            config.window.width = 1280;
            config.window.height = 820;
            // The sidebar shows the platform material where one exists
            // (macOS); elsewhere the window stays opaque.
            config.window.backdrop = cove_ui::hosts::WindowBackdrop::Sidebar;
        })
        .diagnostics(cove_ui::hosts::desktop::diagnostics::devtools::factory())
}
