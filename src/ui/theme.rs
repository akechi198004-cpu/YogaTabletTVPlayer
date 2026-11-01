//! 铁灰主题。颜色取自 Android 版的 res/values/colors.xml。
//!
//! 低配平板上去掉动画、阴影和复杂渐变，只保留必要的圆角与描边。

use eframe::egui::epaint::Shadow;
use eframe::egui::{self, Color32, FontFamily, FontId, Rounding, Stroke, TextStyle, Vec2};

pub const BG_ROOT: Color32 = Color32::from_rgb(0x18, 0x1A, 0x1D);
pub const BG_PANEL: Color32 = Color32::from_rgb(0x1B, 0x1E, 0x23);
pub const BG_PANEL_ALT: Color32 = Color32::from_rgb(0x14, 0x17, 0x1A);
pub const BG_ELEVATED: Color32 = Color32::from_rgb(0x24, 0x29, 0x30);
pub const BG_HOVER: Color32 = Color32::from_rgb(0x2D, 0x33, 0x3B);
pub const BG_ACTIVE: Color32 = Color32::from_rgb(0x33, 0x3A, 0x44);
pub const BG_INPUT: Color32 = Color32::from_rgb(0x12, 0x15, 0x19);
pub const BG_SCREEN: Color32 = Color32::from_rgb(0x0A, 0x0C, 0x0E);
pub const LINE: Color32 = Color32::from_rgb(0x29, 0x2F, 0x38);
pub const LINE_STRONG: Color32 = Color32::from_rgb(0x34, 0x3B, 0x46);

pub const ACCENT: Color32 = Color32::from_rgb(0x38, 0xBD, 0xF8);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0x17, 0x3A, 0x4A);
pub const AMBER: Color32 = Color32::from_rgb(0xF5, 0x9E, 0x0B);
pub const EMERALD: Color32 = Color32::from_rgb(0x34, 0xD3, 0x99);
pub const ROSE: Color32 = Color32::from_rgb(0xF4, 0x3F, 0x5E);

pub const TEXT: Color32 = Color32::from_rgb(0xF8, 0xFA, 0xFC);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0x94, 0xA3, 0xB8);
pub const TEXT_TERTIARY: Color32 = Color32::from_rgb(0x64, 0x74, 0x8B);

pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.dark_mode = true;
    visuals.panel_fill = BG_ROOT;
    visuals.window_fill = BG_PANEL;
    visuals.window_stroke = Stroke::new(1.0, LINE_STRONG);
    visuals.window_rounding = Rounding::same(10.0);
    visuals.window_shadow = Shadow::NONE;
    visuals.popup_shadow = Shadow::NONE;
    visuals.extreme_bg_color = BG_INPUT;
    visuals.faint_bg_color = BG_ELEVATED;
    visuals.code_bg_color = BG_INPUT;
    visuals.hyperlink_color = ACCENT;
    visuals.warn_fg_color = AMBER;
    visuals.error_fg_color = ROSE;
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0, BG_ROOT);
    visuals.text_cursor = Stroke::new(1.5, ACCENT);

    visuals.widgets.noninteractive.bg_fill = BG_PANEL;
    visuals.widgets.noninteractive.weak_bg_fill = BG_PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_SECONDARY);
    visuals.widgets.noninteractive.rounding = Rounding::same(7.0);

    visuals.widgets.inactive.bg_fill = BG_ELEVATED;
    visuals.widgets.inactive.weak_bg_fill = BG_ELEVATED;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.inactive.rounding = Rounding::same(7.0);

    visuals.widgets.hovered.bg_fill = BG_HOVER;
    visuals.widgets.hovered.weak_bg_fill = BG_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, LINE_STRONG);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.hovered.rounding = Rounding::same(7.0);

    visuals.widgets.active.bg_fill = BG_ACTIVE;
    visuals.widgets.active.weak_bg_fill = BG_ACTIVE;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.active.rounding = Rounding::same(7.0);

    visuals.widgets.open.bg_fill = BG_ELEVATED;
    visuals.widgets.open.weak_bg_fill = BG_ELEVATED;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, LINE_STRONG);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.open.rounding = Rounding::same(7.0);

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.animation_time = 0.0;
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(12.0, 5.0);
    style.spacing.interact_size = Vec2::new(38.0, 32.0);
    style.spacing.scroll.floating = false;
    style.spacing.scroll.bar_width = 6.0;
    style.spacing.scroll.bar_inner_margin = 4.0;
    // 默认滚动条把手用前景色（接近白色），在铁灰主题里太刺眼。
    style.spacing.scroll.foreground_color = false;
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(14.0, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(13.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(11.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(20.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(12.0, FontFamily::Monospace),
    );
    ctx.set_style(style);
}
