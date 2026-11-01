//! 频道列表行与分类胶囊。全部手绘，避免每行创建大量子控件，
//! 这样即使自定义源有几千个频道也能流畅滚动。

use eframe::egui::{self, Align2, Color32, FontId, Rect, Response, Rounding, Sense, Stroke, Vec2};

use super::theme;
use crate::models::{Channel, LatencyState};

pub const ROW_HEIGHT: f32 = 42.0;
const STAR_WIDTH: f32 = 36.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct RowAction {
    pub play: bool,
    pub toggle_favorite: bool,
}

pub fn draw_channel_row(
    ui: &mut egui::Ui,
    channel: &Channel,
    index: usize,
    current: bool,
) -> RowAction {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), Sense::click());

    let hovered = response.hovered();
    let background = if current {
        theme::BG_ELEVATED
    } else if hovered {
        theme::BG_HOVER
    } else {
        Color32::TRANSPARENT
    };
    if background != Color32::TRANSPARENT {
        ui.painter().rect_filled(
            rect.shrink2(Vec2::new(1.0, 1.0)),
            Rounding::same(7.0),
            background,
        );
    }

    if current {
        let bar = Rect::from_min_size(
            egui::pos2(rect.left() + 6.0, rect.center().y - 9.0),
            Vec2::new(3.0, 18.0),
        );
        ui.painter()
            .rect_filled(bar, Rounding::same(2.0), theme::ACCENT);
    }

    let painter = ui.painter();
    let index_font = FontId::proportional(10.0);
    painter.text(
        egui::pos2(rect.left() + 14.0, rect.center().y),
        Align2::RIGHT_CENTER,
        format!("{:02}", index + 1),
        index_font.clone(),
        theme::TEXT_TERTIARY,
    );

    let star_center = egui::pos2(rect.right() - 18.0, rect.center().y);
    let star_rect = Rect::from_center_size(star_center, Vec2::new(STAR_WIDTH, ROW_HEIGHT));
    let star_color = if channel.favorite {
        theme::AMBER
    } else if hovered {
        theme::TEXT_SECONDARY
    } else {
        theme::TEXT_TERTIARY
    };
    painter.text(
        star_center,
        Align2::CENTER_CENTER,
        if channel.favorite { "★" } else { "☆" },
        FontId::proportional(15.0),
        star_color,
    );

    let mut right = star_rect.left() - 6.0;

    let latency_text = match channel.latency {
        LatencyState::Ok(ms) => Some(format!("{ms}ms")),
        LatencyState::Failed => Some("超时".to_owned()),
        LatencyState::Unknown => None,
    };
    if let Some(text) = latency_text {
        let color = latency_color(channel.latency);
        let font = FontId::proportional(9.5);
        let size = text_size(ui, &text, &font);
        let badge = Rect::from_min_size(
            egui::pos2(right - size.x, rect.center().y - 9.0),
            Vec2::new(size.x, 18.0),
        );
        painter.text(badge.center(), Align2::CENTER_CENTER, text, font, color);
        right = badge.left() - 6.0;
    }

    if !channel.resolution.is_empty() {
        let font = FontId::proportional(9.0);
        let text = channel.resolution.as_str();
        let text_width = text_size(ui, text, &font).x;
        let badge_width = text_width + 10.0;
        if right - badge_width > rect.left() + 80.0 {
            let badge = Rect::from_min_size(
                egui::pos2(right - badge_width, rect.center().y - 9.0),
                Vec2::new(badge_width, 18.0),
            );
            painter.rect_filled(badge, Rounding::same(4.0), theme::BG_ELEVATED);
            painter.text(
                badge.center(),
                Align2::CENTER_CENTER,
                text,
                font,
                theme::TEXT_SECONDARY,
            );
            right = badge.left() - 6.0;
        }
    }

    let name_left = rect.left() + 40.0;
    let max_name_width = (right - name_left).max(0.0);
    let name = fit_text(
        ui,
        &channel.name,
        FontId::proportional(13.5),
        max_name_width,
    );
    painter.text(
        egui::pos2(name_left, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(13.5),
        if current { theme::ACCENT } else { theme::TEXT },
    );

    let pointer = response.interact_pointer_pos();
    let pointer_on_star = pointer
        .map(|position| star_rect.contains(position))
        .unwrap_or(false);

    let mut action = RowAction::default();
    if response.clicked() {
        if pointer_on_star {
            action.toggle_favorite = true;
        } else {
            action.play = true;
        }
    }
    if response.secondary_clicked() || response.long_touched() {
        action.toggle_favorite = true;
    }

    let tooltip = if channel.lines.len() > 1 {
        format!(
            "{} · {} · {} 条线路\n点击播放，右侧星标收藏",
            channel.name,
            channel.group,
            channel.lines.len()
        )
    } else {
        format!(
            "{} · {}\n点击播放，右侧星标收藏",
            channel.name, channel.group
        )
    };
    response.on_hover_text(tooltip);

    action
}

pub fn draw_chip(ui: &mut egui::Ui, label: &str, selected: bool) -> Response {
    let text_color = if selected {
        theme::BG_ROOT
    } else {
        theme::TEXT_SECONDARY
    };
    let fill = if selected {
        theme::ACCENT
    } else {
        theme::BG_ELEVATED
    };
    ui.add(
        egui::Button::new(egui::RichText::new(label).size(12.0).color(text_color))
            .min_size(Vec2::new(0.0, 28.0))
            .fill(fill)
            .stroke(Stroke::new(
                1.0,
                if selected { theme::ACCENT } else { theme::LINE },
            ))
            .rounding(Rounding::same(14.0)),
    )
}

pub fn latency_color(latency: LatencyState) -> Color32 {
    match latency {
        LatencyState::Ok(ms) if ms <= 500 => theme::EMERALD,
        LatencyState::Ok(ms) if ms <= 1200 => theme::AMBER,
        LatencyState::Ok(_) => theme::ROSE,
        LatencyState::Failed => theme::ROSE,
        LatencyState::Unknown => theme::TEXT_TERTIARY,
    }
}

fn text_size(ui: &egui::Ui, text: &str, font: &FontId) -> Vec2 {
    ui.fonts(|fonts| {
        fonts
            .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
            .size()
    })
}

/// 单行省略。频道名通常很短，O(n²) 的逐字测量只发生在超长名称上。
fn fit_text(ui: &egui::Ui, text: &str, font: FontId, max_width: f32) -> String {
    if max_width <= 8.0 {
        return String::new();
    }
    if text_size(ui, text, &font).x <= max_width {
        return text.to_owned();
    }

    let ellipsis = "…";
    let ellipsis_width = text_size(ui, ellipsis, &font).x;
    let mut result = String::new();
    for ch in text.chars() {
        let mut candidate = result.clone();
        candidate.push(ch);
        if text_size(ui, &candidate, &font).x + ellipsis_width > max_width {
            break;
        }
        result.push(ch);
    }
    result.push_str(ellipsis);
    result
}
