//! Visual identity, runtime palettes and shared card styles.

use std::sync::atomic::{AtomicU8, Ordering};

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Theme, Visuals};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

impl ThemeMode {
    pub const ALL: [Self; 2] = [Self::Dark, Self::Light];

    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "Crustom Dark",
            Self::Light => "Crustom Light",
        }
    }

    pub fn setting_value(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    pub fn from_setting(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("light") => Self::Light,
            _ => Self::Dark,
        }
    }
}

struct Palette {
    bg: Color32,
    sidebar: Color32,
    card: Color32,
    card_raised: Color32,
    card_hover: Color32,
    field: Color32,
    border: Color32,
    border_soft: Color32,
    text: Color32,
    muted: Color32,
    faint: Color32,
    accent: Color32,
    accent_hover: Color32,
    accent_dark: Color32,
    accent_soft: Color32,
    ink: Color32,
    green: Color32,
    amber: Color32,
    red: Color32,
}

const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x0B, 0x0F, 0x14),
    sidebar: Color32::from_rgb(0x0D, 0x12, 0x19),
    card: Color32::from_rgb(0x13, 0x1A, 0x23),
    card_raised: Color32::from_rgb(0x17, 0x20, 0x2B),
    card_hover: Color32::from_rgb(0x1B, 0x25, 0x31),
    field: Color32::from_rgb(0x0A, 0x0E, 0x13),
    border: Color32::from_rgb(0x22, 0x2D, 0x3B),
    border_soft: Color32::from_rgb(0x1A, 0x23, 0x2E),
    text: Color32::from_rgb(0xE8, 0xEF, 0xF7),
    muted: Color32::from_rgb(0x8C, 0x9B, 0xB0),
    faint: Color32::from_rgb(0x5A, 0x68, 0x7A),
    accent: Color32::from_rgb(0x4C, 0x8D, 0xFF),
    accent_hover: Color32::from_rgb(0x6B, 0xA2, 0xFF),
    accent_dark: Color32::from_rgb(0x33, 0x72, 0xE6),
    accent_soft: Color32::from_rgb(0x1B, 0x2B, 0x45),
    ink: Color32::from_rgb(0x06, 0x0C, 0x15),
    green: Color32::from_rgb(0x34, 0xD3, 0x99),
    amber: Color32::from_rgb(0xF5, 0xB5, 0x4B),
    red: Color32::from_rgb(0xF8, 0x71, 0x71),
};

const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xF3, 0xF6, 0xFA),
    sidebar: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    card: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    card_raised: Color32::from_rgb(0xF5, 0xF8, 0xFC),
    card_hover: Color32::from_rgb(0xEA, 0xF0, 0xF8),
    field: Color32::from_rgb(0xF8, 0xFA, 0xFD),
    border: Color32::from_rgb(0xD5, 0xDF, 0xEB),
    border_soft: Color32::from_rgb(0xE6, 0xEC, 0xF4),
    text: Color32::from_rgb(0x17, 0x24, 0x33),
    muted: Color32::from_rgb(0x5B, 0x6C, 0x80),
    faint: Color32::from_rgb(0x87, 0x95, 0xA7),
    accent: Color32::from_rgb(0x2F, 0x6F, 0xED),
    accent_hover: Color32::from_rgb(0x4A, 0x84, 0xF2),
    accent_dark: Color32::from_rgb(0x1F, 0x58, 0xC4),
    accent_soft: Color32::from_rgb(0xE3, 0xEC, 0xFF),
    ink: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    green: Color32::from_rgb(0x0E, 0x8F, 0x6B),
    amber: Color32::from_rgb(0xB4, 0x73, 0x14),
    red: Color32::from_rgb(0xD1, 0x43, 0x43),
};

static MODE: AtomicU8 = AtomicU8::new(0);

fn palette() -> &'static Palette {
    if MODE.load(Ordering::Relaxed) == 1 {
        &LIGHT
    } else {
        &DARK
    }
}

pub fn bg() -> Color32 {
    palette().bg
}
pub fn sidebar() -> Color32 {
    palette().sidebar
}
pub fn card() -> Color32 {
    palette().card
}
pub fn card_raised() -> Color32 {
    palette().card_raised
}
pub fn card_hover() -> Color32 {
    palette().card_hover
}
pub fn field() -> Color32 {
    palette().field
}
pub fn border() -> Color32 {
    palette().border
}
pub fn border_soft() -> Color32 {
    palette().border_soft
}
pub fn text() -> Color32 {
    palette().text
}
pub fn muted() -> Color32 {
    palette().muted
}
pub fn faint() -> Color32 {
    palette().faint
}
pub fn accent() -> Color32 {
    palette().accent
}
pub fn accent_hover() -> Color32 {
    palette().accent_hover
}
pub fn accent_dark() -> Color32 {
    palette().accent_dark
}
pub fn accent_soft() -> Color32 {
    palette().accent_soft
}
pub fn ink() -> Color32 {
    palette().ink
}
pub fn green() -> Color32 {
    palette().green
}
pub fn amber() -> Color32 {
    palette().amber
}
pub fn red() -> Color32 {
    palette().red
}

/// Apply the saved theme to the whole app. This can be called again to switch themes.
pub fn apply(ctx: &egui::Context, mode: ThemeMode) {
    MODE.store(
        match mode {
            ThemeMode::Dark => 0,
            ThemeMode::Light => 1,
        },
        Ordering::Relaxed,
    );

    let theme = match mode {
        ThemeMode::Dark => Theme::Dark,
        ThemeMode::Light => Theme::Light,
    };
    ctx.set_theme(theme);
    ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(match mode {
        ThemeMode::Dark => egui::SystemTheme::Dark,
        ThemeMode::Light => egui::SystemTheme::Light,
    }));

    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(14.0, 8.0);
        style.spacing.interact_size.y = 34.0;
        style.spacing.scroll.bar_width = 8.0;
        style.animation_time = 0.14;

        style.text_styles.insert(
            TextStyle::Heading,
            FontId::new(24.0, FontFamily::Proportional),
        );
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(14.0, FontFamily::Proportional));
        style.text_styles.insert(
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Small,
            FontId::new(11.5, FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(12.5, FontFamily::Monospace),
        );
    });

    let visuals = build_visuals(mode);
    ctx.style_mut_of(theme, |style| {
        style.visuals = visuals;
    });
}

fn build_visuals(mode: ThemeMode) -> Visuals {
    let mut visuals = match mode {
        ThemeMode::Dark => Visuals::dark(),
        ThemeMode::Light => Visuals::light(),
    };

    visuals.panel_fill = bg();
    visuals.window_fill = card();
    visuals.extreme_bg_color = field();
    visuals.faint_bg_color = card_raised();
    visuals.override_text_color = Some(text());
    visuals.window_stroke = Stroke::new(1.0, border());
    visuals.selection.bg_fill = accent().gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, accent());
    visuals.hyperlink_color = accent();
    visuals.warn_fg_color = amber();
    visuals.error_fg_color = red();

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = card();
    widgets.noninteractive.weak_bg_fill = card();
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, border());
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, text());

    widgets.inactive.bg_fill = card_raised();
    widgets.inactive.weak_bg_fill = card_raised();
    widgets.inactive.bg_stroke = Stroke::new(1.0, border());
    widgets.inactive.fg_stroke = Stroke::new(1.0, text());

    widgets.hovered.bg_fill = card_hover();
    widgets.hovered.weak_bg_fill = card_hover();
    widgets.hovered.bg_stroke = Stroke::new(1.0, accent().gamma_multiply(0.65));
    widgets.hovered.fg_stroke = Stroke::new(1.0, text());

    widgets.active.bg_fill = accent_soft();
    widgets.active.weak_bg_fill = accent_soft();
    widgets.active.bg_stroke = Stroke::new(1.0, accent());
    widgets.active.fg_stroke = Stroke::new(1.0, text());

    widgets.open.bg_fill = card_raised();
    widgets.open.weak_bg_fill = card_raised();
    widgets.open.bg_stroke = Stroke::new(1.0, border());
    widgets.open.fg_stroke = Stroke::new(1.0, text());

    for widget in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(8);
    }

    visuals
}

/// Standard content card: panel fill, hairline border, generous padding.
pub fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(card())
        .stroke(Stroke::new(1.0, border()))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(egui::Margin::same(20))
}

/// A lighter inset card used inside larger panels.
pub fn inset_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(card_raised())
        .stroke(Stroke::new(1.0, border_soft()))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::same(14))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_theme_preferences() {
        assert_eq!(ThemeMode::from_setting(Some("light")), ThemeMode::Light);
        assert_eq!(ThemeMode::from_setting(Some("dark")), ThemeMode::Dark);
        assert_eq!(ThemeMode::from_setting(None), ThemeMode::Dark);
        assert_eq!(ThemeMode::Light.setting_value(), "light");
    }

    #[test]
    fn applies_both_palettes() {
        let ctx = egui::Context::default();
        apply(&ctx, ThemeMode::Light);
        assert_eq!(bg(), LIGHT.bg);
        assert_eq!(text(), LIGHT.text);

        apply(&ctx, ThemeMode::Dark);
        assert_eq!(bg(), DARK.bg);
        assert_eq!(text(), DARK.text);
    }
}
