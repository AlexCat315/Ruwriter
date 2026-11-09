use crate::views::RootView;
use cove_ui::ApplicationLaunch;

pub fn application() -> ApplicationLaunch {
    ApplicationLaunch::new(RootView)
        .configure(|config| {
            config.window.title = "Ruwriter".into();
            config.window.width = 1280;
            config.window.height = 800;
        })
        .diagnostics(cove_ui::hosts::desktop::diagnostics::devtools::factory())
}
