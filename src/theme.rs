//! Ruwriter palette: warm paper in light mode, warm charcoal in dark mode,
//! one terracotta ink accent. Regions separate by fill, never by shadow.

use cove_ui::compose::theme::{SemanticFontRef, TextStyleToken, Theme};
use cove_ui::core::{Color, Font, FontWeight};
use ruwriter_core::AppearancePreference;

fn rgb(hex: u32) -> Color {
    Color::from_rgba8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 255)
}

fn rgba(hex: u32, alpha: u8) -> Color {
    Color::from_rgba8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, alpha)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Light,
    Dark,
}

impl Mode {
    /// Resolves the preference; `System` follows the host's live report and
    /// falls back to light where the platform reports none.
    pub fn resolve(preference: AppearancePreference, system_dark: Option<bool>) -> Self {
        match preference {
            AppearancePreference::Light => Self::Light,
            AppearancePreference::Dark => Self::Dark,
            AppearancePreference::System => {
                if system_dark.unwrap_or(false) {
                    Self::Dark
                } else {
                    Self::Light
                }
            }
        }
    }

    pub fn theme(self) -> Theme {
        match self {
            Self::Light => light(),
            Self::Dark => dark(),
        }
    }
}

fn light() -> Theme {
    let mut theme = Theme::default_light();
    let c = &mut theme.colors;
    c.background = rgb(0xFCFBF8);
    c.surface = rgb(0xFCFBF8);
    c.surface_variant = rgb(0xF3F1EC);
    c.surface_pressed = rgb(0xE6E2DA);
    c.surface_sunken = rgb(0xEFECE6);
    c.surface_raised = rgb(0xFFFFFF);
    c.surface_floating = rgb(0xFFFFFF);
    c.surface_overlay = rgb(0xFFFFFF);
    c.on_background = rgb(0x2A2723);
    c.on_surface = rgb(0x2A2723);
    c.on_surface_variant = rgb(0x4A4640);
    c.on_surface_muted = rgb(0x736D63);
    c.accent = rgb(0xB5532C);
    c.accent_hover = rgb(0xA54A26);
    c.accent_pressed = rgb(0x8F3F20);
    c.on_accent = rgb(0xFFFFFF);
    // Opaque: alpha washes blend in linear space and read far stronger on
    // dark fills than their nominal opacity.
    c.accent_muted = rgb(0xF4E8DF);
    c.state_layer_hover = rgba(0x3A2E20, 0x0A);
    c.state_layer_pressed = rgba(0x3A2E20, 0x14);
    c.state_layer_selected = rgba(0xB5532C, 0x1C);
    c.border = rgb(0xDDD8CE);
    c.border_strong = rgb(0x9A9386);
    c.divider = rgb(0xE9E5DD);
    c.focus_ring = rgba(0xB5532C, 0x80);
    c.selection = rgba(0xB5532C, 0x30);
    sync_inputs(&mut theme);
    theme
}

fn dark() -> Theme {
    let mut theme = Theme::default_dark();
    let c = &mut theme.colors;
    c.background = rgb(0x1D1C1A);
    c.surface = rgb(0x1D1C1A);
    c.surface_variant = rgb(0x252421);
    c.surface_pressed = rgb(0x34322E);
    c.surface_sunken = rgb(0x2A2926);
    c.surface_raised = rgb(0x2A2926);
    c.surface_floating = rgb(0x2C2B28);
    c.surface_overlay = rgb(0x2C2B28);
    c.on_background = rgb(0xE8E4DC);
    c.on_surface = rgb(0xE8E4DC);
    c.on_surface_variant = rgb(0xC9C3B8);
    c.on_surface_muted = rgb(0x9A9488);
    c.accent = rgb(0xE08A5B);
    c.accent_hover = rgb(0xE89A6E);
    c.accent_pressed = rgb(0xC9784D);
    c.on_accent = rgb(0x1D1C1A);
    c.accent_muted = rgb(0x3A2D25);
    c.state_layer_hover = rgba(0xFFF5E8, 0x06);
    c.state_layer_pressed = rgba(0xFFF5E8, 0x0C);
    c.state_layer_selected = rgba(0xE08A5B, 0x0E);
    c.border = rgb(0x3A3833);
    c.border_strong = rgb(0x6E685E);
    c.divider = rgb(0x312F2B);
    c.focus_ring = rgba(0xE08A5B, 0x90);
    c.selection = rgba(0xE08A5B, 0x45);
    sync_inputs(&mut theme);
    theme
}

/// `InputAppearance` is captured when a theme is built; keep it in step.
fn sync_inputs(theme: &mut Theme) {
    theme.inputs.background = theme.colors.surface_sunken;
    theme.inputs.border_color = theme.colors.border;
    theme.inputs.border_focused = theme.colors.accent;
}

/// Editor body: the chosen size with a generous 1.75 line box.
pub fn editor_font(size: u8) -> SemanticFontRef {
    let size = f32::from(size);
    SemanticFontRef::new(move |_| {
        TextStyleToken::new(
            Font::system(size, FontWeight::Regular),
            (size * 1.75).round(),
            0.0,
        )
    })
}

/// Readable column width for a body size: about forty full-width glyphs.
pub fn measure(size: u8) -> f32 {
    f32::from(size) * 40.0
}

/// Syntax colors for Markdown marks in the editor.
#[derive(Clone, Copy)]
pub struct SyntaxColors {
    pub heading: Color,
    pub mark: Color,
    pub code: Color,
    pub link: Color,
    pub quote: Color,
}

impl SyntaxColors {
    pub fn for_theme(theme: &Theme) -> Self {
        let c = &theme.colors;
        Self {
            heading: c.on_surface,
            mark: c.on_surface_muted.multiplied_alpha(0.7),
            code: c.accent_pressed,
            link: c.accent,
            quote: c.on_surface_variant,
        }
    }
}
