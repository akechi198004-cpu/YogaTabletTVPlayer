//! 界面绘制。布局尽量模拟 Android 版：
//! 顶部标题栏、左侧/上方播放区、右侧/下方频道列表、底部状态条。
//!
//! 视频解码在外部 VLC 里，所以播放区是一块“正在播放”信息面板，而不是
//! egui 自绘视频。这样低配平板只需要承担一个很轻的 UI 进程。

use eframe::egui::{
    self, Align, Align2, Color32, FontId, Layout, Margin, Rect, RichText, Rounding, Sense, Stroke,
    Vec2,
};

use crate::app::{SettingsAction, TouchIptvApp};
use crate::models::LatencyState;
use crate::ui::channel_list::{self, RowAction};
use crate::ui::theme;

impl TouchIptvApp {
    pub(crate) fn draw(&mut self, ctx: &egui::Context) {
        self.draw_header(ctx);
        self.draw_notice(ctx);

        let wide = ctx.screen_rect().width() >= 900.0;
        if wide {
            egui::SidePanel::right("channel_panel")
                .resizable(true)
                .default_width(334.0)
                .width_range(280.0..=440.0)
                .frame(
                    egui::Frame::none()
                        .fill(theme::BG_PANEL)
                        .inner_margin(Margin::symmetric(10.0, 0.0)),
                )
                .show(ctx, |ui| self.draw_channel_panel(ui, ctx));
        } else {
            let max_channel = (ctx.screen_rect().height() - 360.0).max(220.0);
            let height = (ctx.screen_rect().height() * 0.52)
                .clamp(220.0, 560.0)
                .min(max_channel);
            egui::TopBottomPanel::bottom("channel_panel_narrow")
                .resizable(true)
                .default_height(height)
                .height_range(220.0..=600.0)
                .frame(
                    egui::Frame::none()
                        .fill(theme::BG_PANEL)
                        .inner_margin(Margin::symmetric(10.0, 0.0)),
                )
                .show(ctx, |ui| self.draw_channel_panel(ui, ctx));
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(theme::BG_ROOT)
                    .inner_margin(Margin::same(14.0)),
            )
            .show(ctx, |ui| self.draw_player_panel(ui, ctx));

        self.draw_settings(ctx);
    }

    // ------------------------------------------------------------------ 顶栏

    fn draw_header(&mut self, ctx: &egui::Context) {
        let narrow = ctx.screen_rect().width() < 720.0;
        egui::TopBottomPanel::top("header")
            .exact_height(48.0)
            .frame(
                egui::Frame::none()
                    .fill(theme::BG_PANEL)
                    .inner_margin(Margin::symmetric(12.0, 0.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    paint_logo(ui);
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("TouchIPTV")
                            .size(16.0)
                            .strong()
                            .color(theme::TEXT),
                    );
                    if !narrow {
                        ui.add_space(4.0);
                        badge(
                            ui,
                            "极速直播 · 360-720P",
                            theme::TEXT_SECONDARY,
                            theme::BG_ELEVATED,
                        );
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(bar_button("设置")).clicked() {
                            self.open_settings();
                        }
                        if self.batch.is_some() {
                            if ui.add(bar_button("停止测速")).clicked() {
                                self.cancel_batch();
                            }
                        } else if ui.add(bar_button("批量测速")).clicked() {
                            self.start_batch_test(ctx);
                        }
                        if !narrow {
                            ui.label(
                                RichText::new(self.header_status())
                                    .size(11.0)
                                    .color(theme::TEXT_SECONDARY),
                            );
                        }
                    });
                });
            });
    }

    fn draw_notice(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("notice")
            .exact_height(26.0)
            .frame(
                egui::Frame::none()
                    .fill(theme::BG_PANEL_ALT)
                    .inner_margin(Margin::symmetric(12.0, 0.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new(self.notice.as_str()).size(11.0).color(
                        if self.notice_error {
                            theme::ROSE
                        } else {
                            theme::TEXT_SECONDARY
                        },
                    ));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(state) = &self.playback {
                            ui.label(
                                RichText::new(format!("VLC · {}s", state.elapsed_secs()))
                                    .size(10.0)
                                    .color(theme::TEXT_TERTIARY),
                            );
                        } else if self.pending_preflight.is_some() {
                            ui.label(
                                RichText::new("正在预检")
                                    .size(10.0)
                                    .color(theme::TEXT_TERTIARY),
                            );
                        }
                    });
                });
            });
    }

    // ------------------------------------------------------------------ 播放区

    fn draw_player_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let viewport_height = ui.available_height();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.draw_now_heading(ui);
                ui.add_space(8.0);
                self.draw_stage(ui, viewport_height);
                ui.add_space(8.0);
                self.draw_player_controls(ui, ctx);
                ui.add_space(6.0);
                self.draw_player_hints(ui);
            });
    }

    fn draw_now_heading(&mut self, ui: &mut egui::Ui) {
        let channel = self.playing_channel().or_else(|| self.selected_channel());
        let playing = self.playback.is_some();
        let line = self
            .playback
            .as_ref()
            .map(|state| state.line_index)
            .unwrap_or(0);

        ui.horizontal_wrapped(|ui| {
            if playing {
                ui.label(RichText::new("●").size(10.0).color(theme::ROSE));
            }
            match channel {
                Some(channel) => {
                    ui.label(
                        RichText::new(channel.name.as_str())
                            .size(22.0)
                            .strong()
                            .color(theme::TEXT),
                    );
                    badge(
                        ui,
                        channel.group.as_str(),
                        theme::TEXT_SECONDARY,
                        theme::BG_ELEVATED,
                    );
                    if !channel.resolution.is_empty() {
                        badge(
                            ui,
                            channel.resolution.as_str(),
                            theme::TEXT_SECONDARY,
                            theme::BG_ELEVATED,
                        );
                    }
                    if channel.line_count() > 1 {
                        badge(
                            ui,
                            &format!("线路 {}/{}", line + 1, channel.line_count()),
                            theme::ACCENT,
                            theme::ACCENT_SOFT,
                        );
                    }
                    if channel.latency != LatencyState::Unknown {
                        badge(
                            ui,
                            &latency_text(channel.latency),
                            channel_list::latency_color(channel.latency),
                            theme::BG_ELEVATED,
                        );
                    }
                }
                None => {
                    ui.label(
                        RichText::new("TouchIPTV 直播")
                            .size(22.0)
                            .strong()
                            .color(theme::TEXT),
                    );
                    badge(
                        ui,
                        "外部 VLC 播放",
                        theme::TEXT_SECONDARY,
                        theme::BG_ELEVATED,
                    );
                }
            }
        });
    }

    fn draw_stage(&mut self, ui: &mut egui::Ui, viewport_height: f32) {
        let width = ui.available_width();
        let height = (width * 0.5)
            .clamp(120.0, 420.0)
            .min((viewport_height - 170.0).max(110.0));
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        let painter = ui.painter();

        painter.rect_filled(rect, Rounding::same(12.0), theme::BG_SCREEN);
        painter.rect_stroke(rect, Rounding::same(12.0), Stroke::new(1.0, theme::LINE));

        // 窄窗口里播放区只剩一条窄带，隐藏次要说明，保证控制按钮可见。
        if height < 180.0 {
            let text = if let Some(state) = &self.playback {
                format!("VLC 正在播放 · {}", state.channel_name)
            } else if self.pending_preflight.is_some() {
                "正在检查线路与广告特征…".to_owned()
            } else if let Some((title, _)) = &self.last_error {
                title.clone()
            } else {
                "选择频道开始播放".to_owned()
            };
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                text,
                FontId::proportional(15.0),
                if self.last_error.is_some() && self.playback.is_none() {
                    theme::ROSE
                } else {
                    theme::TEXT
                },
            );
            if let Some(warning) = &self.last_preflight_warning {
                painter.text(
                    rect.left_bottom() + Vec2::new(10.0, -6.0),
                    Align2::LEFT_BOTTOM,
                    warning,
                    FontId::proportional(9.5),
                    theme::AMBER,
                );
            }
            return;
        }

        if let Some(state) = &self.playback {
            let dot = rect.left_top() + Vec2::new(22.0, 24.0);
            painter.circle_filled(dot, 4.0, theme::ROSE);
            painter.text(
                dot + Vec2::new(10.0, 0.0),
                Align2::LEFT_CENTER,
                "LIVE",
                FontId::proportional(10.0),
                theme::ROSE,
            );

            painter.text(
                rect.center() - Vec2::new(0.0, 42.0),
                Align2::CENTER_CENTER,
                "VLC 正在播放",
                FontId::proportional(12.0),
                theme::TEXT_SECONDARY,
            );
            painter.text(
                rect.center() - Vec2::new(0.0, 8.0),
                Align2::CENTER_CENTER,
                &state.channel_name,
                FontId::proportional(26.0),
                theme::TEXT,
            );
            let line = self
                .playing_channel()
                .map(|channel| {
                    format!(
                        "线路 {}/{} · {}",
                        state.line_index + 1,
                        channel.line_count(),
                        if channel.resolution.is_empty() {
                            "画质未知"
                        } else {
                            channel.resolution.as_str()
                        }
                    )
                })
                .unwrap_or_else(|| format!("线路 {}", state.line_index + 1));
            painter.text(
                rect.center() + Vec2::new(0.0, 28.0),
                Align2::CENTER_CENTER,
                line,
                FontId::proportional(12.0),
                theme::TEXT_SECONDARY,
            );
            painter.text(
                rect.left_bottom() + Vec2::new(22.0, -20.0),
                Align2::LEFT_BOTTOM,
                "播放控制请使用 VLC 窗口",
                FontId::proportional(10.5),
                theme::TEXT_TERTIARY,
            );
            painter.text(
                rect.right_bottom() + Vec2::new(-22.0, -20.0),
                Align2::RIGHT_BOTTOM,
                "F 全屏 · 空格 暂停 · N/P 换台",
                FontId::proportional(10.5),
                theme::TEXT_TERTIARY,
            );
        } else if self.pending_preflight.is_some() {
            painter.text(
                rect.center() - Vec2::new(0.0, 12.0),
                Align2::CENTER_CENTER,
                "正在检查线路与广告特征…",
                FontId::proportional(17.0),
                theme::TEXT,
            );
            painter.text(
                rect.center() + Vec2::new(0.0, 16.0),
                Align2::CENTER_CENTER,
                "首次连接可能需要几秒，请稍候",
                FontId::proportional(11.0),
                theme::TEXT_TERTIARY,
            );
        } else if let Some((title, detail)) = &self.last_error {
            painter.text(
                rect.left_top() + Vec2::new(28.0, 26.0),
                Align2::LEFT_TOP,
                title,
                FontId::proportional(18.0),
                theme::ROSE,
            );
            let galley = ui.fonts(|fonts| {
                fonts.layout(
                    detail.clone(),
                    FontId::proportional(12.0),
                    theme::TEXT_SECONDARY,
                    (rect.width() - 56.0).max(120.0),
                )
            });
            painter.galley(
                rect.left_top() + Vec2::new(28.0, 56.0),
                galley,
                theme::TEXT_SECONDARY,
            );
            painter.text(
                rect.left_bottom() + Vec2::new(28.0, -22.0),
                Align2::LEFT_BOTTOM,
                "可点击下方「重新连接」，或换一个频道",
                FontId::proportional(10.5),
                theme::TEXT_TERTIARY,
            );
        } else {
            let tv =
                Rect::from_center_size(rect.center() - Vec2::new(0.0, 42.0), Vec2::new(74.0, 52.0));
            paint_tv_outline(painter, tv, theme::ACCENT_SOFT);
            painter.text(
                rect.center() + Vec2::new(0.0, 8.0),
                Align2::CENTER_CENTER,
                "选择频道开始播放",
                FontId::proportional(18.0),
                theme::TEXT,
            );
            painter.text(
                rect.center() + Vec2::new(0.0, 36.0),
                Align2::CENTER_CENTER,
                "点击频道列表即可调用 VLC 播放",
                FontId::proportional(11.0),
                theme::TEXT_TERTIARY,
            );
        }

        if let Some(warning) = &self.last_preflight_warning {
            painter.text(
                rect.left_bottom() + Vec2::new(16.0, -6.0),
                Align2::LEFT_BOTTOM,
                warning,
                FontId::proportional(10.0),
                theme::AMBER,
            );
        }
    }

    fn draw_player_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let selected = self.selected_channel().cloned();
        let key = self
            .playback
            .as_ref()
            .map(|state| state.channel_key.clone())
            .or_else(|| self.selected_key.clone());
        let line = self
            .playback
            .as_ref()
            .map(|state| state.line_index)
            .unwrap_or(0);
        let playing = self.playback.is_some();
        let pending = self.pending_preflight.is_some();
        let batch_running = self.batch.is_some();
        let vlc_ready = self.vlc_ready();
        let favorite = selected
            .as_ref()
            .map(|channel| channel.favorite)
            .unwrap_or(false);
        let line_count = selected
            .as_ref()
            .map(|channel| channel.line_count())
            .unwrap_or(1);

        ui.horizontal_wrapped(|ui| {
            if playing || pending {
                if ui.add(primary_button("停止")).clicked() {
                    self.stop_playback();
                }
            } else if selected.is_some() && ui.add(primary_button("开始播放")).clicked() {
                if let Some(key) = key.clone() {
                    self.play_channel(key, ctx);
                }
            }

            if selected.is_some() {
                if ui.add(bar_button("重新连接")).clicked() {
                    self.retry_current(ctx);
                }
                if ui.add(bar_button("上一台")).clicked() {
                    self.step_channel(-1, ctx);
                }
                if ui.add(bar_button("下一台")).clicked() {
                    self.step_channel(1, ctx);
                }
                if line_count > 1 {
                    let label = format!("线路 {}/{}", line + 1, line_count);
                    if ui.add(bar_button(&label)).clicked() {
                        self.switch_line(ctx);
                    }
                }
                if ui.add(bar_button("测速")).clicked() {
                    if let Some(key) = key.as_deref() {
                        self.start_single_speed(key, ctx);
                    }
                }
                if ui
                    .add(bar_button(if favorite { "取消收藏" } else { "收藏" }))
                    .clicked()
                {
                    if let Some(key) = key.as_deref() {
                        self.toggle_favorite(key);
                    }
                }
            }

            if !vlc_ready && ui.add(bar_button("设置 VLC")).clicked() {
                self.open_settings();
            }

            if batch_running {
                ui.label(
                    RichText::new("批量测速中…")
                        .size(11.0)
                        .color(theme::TEXT_TERTIARY),
                );
            }
        });
    }

    fn draw_player_hints(&mut self, ui: &mut egui::Ui) {
        if !self.vlc_ready() {
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "未检测到 VLC：请安装 VLC 3.x，或在设置中填写 vlc.exe 的完整路径。",
                    )
                    .size(11.0)
                    .color(theme::AMBER),
                )
                .wrap(true),
            );
        }
        ui.add(
            egui::Label::new(
                RichText::new("播放控制使用 VLC：空格 暂停 · F 全屏 · N/P 切换播放列表 · Q 退出。")
                    .size(10.5)
                    .color(theme::TEXT_TERTIARY),
            )
            .wrap(true),
        );
        if self.config.use_playlist {
            ui.add(
                egui::Label::new(
                    RichText::new("当前分类已写入 VLC 播放列表，可直接在 VLC 里换台。")
                        .size(10.5)
                        .color(theme::TEXT_TERTIARY),
                )
                .wrap(true),
            );
        }
        if let Some(channel) = self.playing_channel().or_else(|| self.selected_channel()) {
            let line_index = self
                .playback
                .as_ref()
                .filter(|state| state.channel_key == channel.key())
                .map(|state| state.line_index)
                .unwrap_or(0);
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "来源：{} · 当前线路 {}/{} · {}",
                        if channel.source.is_empty() {
                            "未知"
                        } else {
                            channel.source.as_str()
                        },
                        line_index + 1,
                        channel.line_count(),
                        channel.line(line_index)
                    ))
                    .size(10.0)
                    .color(theme::TEXT_TERTIARY),
                )
                .wrap(true),
            );
        }
    }

    // ------------------------------------------------------------------ 频道列表

    fn draw_channel_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("频道列表")
                    .size(15.0)
                    .strong()
                    .color(theme::TEXT),
            );
            ui.label(
                RichText::new(format!("共 {}", self.visible.len()))
                    .size(11.0)
                    .color(theme::TEXT_TERTIARY),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(bar_small_button(if self.sort_latency {
                        "延迟排序 ✓"
                    } else {
                        "延迟排序"
                    }))
                    .clicked()
                {
                    self.toggle_sort();
                }
                if ui.add(bar_small_button("刷新")).clicked() {
                    self.refresh_channels();
                }
            });
        });

        ui.add_space(5.0);
        let clear_width = if self.query.is_empty() { 0.0 } else { 34.0 };
        let search_width = (ui.available_width() - clear_width - 10.0).max(80.0);
        let mut query = self.query.clone();
        ui.horizontal(|ui| {
            let response = ui.add_sized(
                [search_width, 34.0],
                egui::TextEdit::singleline(&mut query)
                    .hint_text("搜索频道 / 分类 / 画质")
                    .margin(Vec2::new(10.0, 7.0)),
            );
            if response.changed() {
                self.query = query;
                self.rebuild_visible(true);
            }
            if !self.query.is_empty() && ui.add_sized([clear_width, 34.0], close_button()).clicked()
            {
                self.query.clear();
                self.rebuild_visible(true);
            }
        });

        ui.add_space(6.0);
        let chips = self.chips.clone();
        egui::ScrollArea::horizontal()
            .id_source("chips")
            .drag_to_scroll(true)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (label, _) in chips {
                        let selected = self.filter == label;
                        if channel_list::draw_chip(ui, &label, selected).clicked() {
                            self.set_filter(label);
                        }
                    }
                });
            });

        ui.add_space(4.0);
        ui.separator();
        ui.add_space(2.0);

        if self.visible.is_empty() {
            self.scroll_to_current = false;
            ui.add_space(36.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("没有匹配的频道")
                        .size(13.0)
                        .color(theme::TEXT_TERTIARY),
                );
                ui.label(
                    RichText::new("试试切换分类或清空搜索")
                        .size(11.0)
                        .color(theme::TEXT_TERTIARY),
                );
            });
            return;
        }

        let selected_key = self
            .playback
            .as_ref()
            .map(|state| state.channel_key.clone())
            .or_else(|| self.selected_key.clone());
        let scroll_position = if self.scroll_to_current {
            selected_key.as_ref().and_then(|key| {
                self.visible
                    .iter()
                    .position(|&index| self.channels[index].key() == *key)
            })
        } else {
            None
        };

        let viewport_height = ui.available_height();
        let spacing = ui.spacing().item_spacing.y;
        let mut scroll = egui::ScrollArea::vertical()
            .id_source("channel_list")
            .auto_shrink([false, false])
            .drag_to_scroll(true);
        if let Some(position) = scroll_position {
            let offset = (position as f32 * (channel_list::ROW_HEIGHT + spacing)
                - viewport_height * 0.35)
                .max(0.0);
            scroll = scroll.vertical_scroll_offset(offset);
        }

        let mut action: Option<(String, RowAction)> = None;
        {
            let visible = &self.visible;
            let channels = &self.channels;
            scroll.show_rows(ui, channel_list::ROW_HEIGHT, visible.len(), |ui, range| {
                for row in range {
                    let index = visible[row];
                    let channel = &channels[index];
                    let is_current = selected_key
                        .as_ref()
                        .map(|key| channel.key() == *key)
                        .unwrap_or(false);
                    let result = channel_list::draw_channel_row(ui, channel, row, is_current);
                    if result.play || result.toggle_favorite {
                        action = Some((channel.key(), result));
                        break;
                    }
                }
            });
        }
        self.scroll_to_current = false;

        if let Some((key, action)) = action {
            if action.toggle_favorite {
                self.toggle_favorite(&key);
            } else if action.play {
                self.play_channel(key, ctx);
            }
        }
    }

    // ------------------------------------------------------------------ 设置

    fn draw_settings(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.settings.clone() else {
            return;
        };

        let mut action: Option<SettingsAction> = None;
        let mut open = true;
        let config_load_failed = self.config_load_failed;
        let config_error = self.config_error.clone();
        let custom_url = self.config.custom_source_url.clone();
        let custom_present = self.custom_source_present;
        let importing = self.importing;
        let import_message = self.import_message.clone();
        let settings_message = self.settings_message.clone();
        let confirm_clear = self.confirm_clear_source;
        let screen = ctx.screen_rect();
        let window_width = (screen.width() - 40.0).clamp(340.0, 640.0);
        let max_height = (screen.height() - 280.0).max(240.0);

        egui::Window::new("设置")
            .collapsible(false)
            .resizable(false)
            .default_width(window_width)
            .max_height(screen.height() - 30.0)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                // 保存/取消放在顶部，内容再长也不会被挤出屏幕。
                ui.horizontal(|ui| {
                    if ui.add(primary_button("保存并关闭")).clicked() {
                        action = Some(SettingsAction::Save);
                    }
                    if ui.add(bar_button("取消")).clicked() {
                        action = Some(SettingsAction::Cancel);
                    }
                });
                ui.separator();

                if config_load_failed {
                    message_label(
                        ui,
                        config_error
                            .as_deref()
                            .unwrap_or("配置文件读取失败，当前使用默认值。保存后会覆盖原文件。"),
                        true,
                    );
                }
                if let Some((message, error)) = &settings_message {
                    message_label(ui, message, *error);
                }
                ui.separator();

                egui::ScrollArea::vertical()
                    .max_height(max_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        section_title(ui, "播放");
                        ui.horizontal(|ui| {
                            ui.label("VLC 路径");
                            let field_width = (ui.available_width() - 88.0).max(120.0);
                            ui.add_sized(
                                [field_width, 30.0],
                                egui::TextEdit::singleline(&mut draft.vlc_path)
                                    .hint_text("例如 C:\\Program Files\\VideoLAN\\VLC\\vlc.exe"),
                            );
                            if ui.button("自动检测").clicked() {
                                action = Some(SettingsAction::AutoDetect);
                            }
                        });
                        ui.checkbox(&mut draft.fullscreen, "VLC 全屏播放");
                        ui.checkbox(
                            &mut draft.use_playlist,
                            "把当前分类写入 VLC 播放列表（可在 VLC 里用 N/P 换台）",
                        );
                        ui.checkbox(
                            &mut draft.disable_hw_decode,
                            "禁用硬件解码（花屏 / 绿屏时勾选）",
                        );
                        ui.checkbox(&mut draft.ad_guard, "启动前检查已知广告特征");
                        ui.checkbox(&mut draft.auto_play_last, "启动程序时自动播放上次频道");
                        ui.horizontal(|ui| {
                            ui.label("网络缓存");
                            ui.add(
                                egui::DragValue::new(&mut draft.network_caching_ms)
                                    .clamp_range(0..=60_000)
                                    .suffix(" ms")
                                    .speed(100.0),
                            );
                            ui.label(
                                RichText::new("低配平板建议 1500-3000")
                                    .size(10.0)
                                    .color(theme::TEXT_TERTIARY),
                            );
                        });

                        ui.collapsing("高级网络选项", |ui| {
                            ui.horizontal(|ui| {
                                ui.label("User-Agent");
                                let width = (ui.available_width() - 4.0).max(100.0);
                                ui.add_sized(
                                    [width, 28.0],
                                    egui::TextEdit::singleline(&mut draft.user_agent)
                                        .hint_text("留空使用 VLC 默认值"),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label("Referer");
                                let width = (ui.available_width() - 4.0).max(100.0);
                                ui.add_sized(
                                    [width, 28.0],
                                    egui::TextEdit::singleline(&mut draft.referer)
                                        .hint_text("留空不使用"),
                                );
                            });
                            ui.label(
                                RichText::new(
                                    "提示：VLC 3.0 对完整浏览器 UA 的支持不稳定；留空通常最稳。",
                                )
                                .size(10.0)
                                .color(theme::TEXT_TERTIARY),
                            );
                        });

                        ui.separator();
                        section_title(ui, "自定义直播源");
                        if custom_present || !custom_url.trim().is_empty() {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!(
                                        "当前自定义源：{}",
                                        if custom_url.trim().is_empty() {
                                            "本地缓存（未记录来源地址）"
                                        } else {
                                            custom_url.trim()
                                        }
                                    ))
                                    .size(10.5)
                                    .color(theme::TEXT_SECONDARY),
                                )
                                .wrap(true),
                            );
                        } else {
                            ui.label(
                                RichText::new("当前仅使用内置频道")
                                    .size(10.5)
                                    .color(theme::TEXT_SECONDARY),
                            );
                        }

                        ui.horizontal(|ui| {
                            let button_width = 64.0;
                            let width = (ui.available_width() - button_width - 6.0).max(120.0);
                            ui.add_sized(
                                [width, 30.0],
                                egui::TextEdit::singleline(&mut draft.source_input)
                                    .hint_text("http(s)://…/playlist.m3u"),
                            );
                            if ui.add_enabled(!importing, bar_button("导入")).clicked() {
                                action = Some(SettingsAction::Import);
                            }
                        });
                        if importing {
                            message_label(ui, "正在下载并解析直播源…", false);
                        }
                        if let Some((message, error)) = &import_message {
                            message_label(ui, message, *error);
                        }

                        if confirm_clear {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("确定清除自定义源？")
                                        .size(11.0)
                                        .color(theme::AMBER),
                                );
                                if ui.add(bar_small_button("确定")).clicked() {
                                    action = Some(SettingsAction::ConfirmClear);
                                }
                                if ui.add(bar_small_button("取消")).clicked() {
                                    action = Some(SettingsAction::CancelClear);
                                }
                            });
                        } else if ui.add(bar_small_button("清除自定义源")).clicked() {
                            action = Some(SettingsAction::ClearSource);
                        }

                        ui.separator();
                        section_title(ui, "关于");
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "TouchIPTV Desktop {} · Rust + egui + 外部 VLC\n\
                                     为 Win32 低性能平板重写：主程序只负责频道浏览、测速和\n\
                                     广告预检，视频解码交给 VLC。支持 HLS / TS / FLV。",
                                    env!("CARGO_PKG_VERSION")
                                ))
                                .size(10.5)
                                .color(theme::TEXT_SECONDARY),
                            )
                            .wrap(true),
                        );
                        ui.add(
                            egui::Label::new(
                                RichText::new(
                                    "限制：外部 VLC 无法拦截播放中途才插入的广告；\
                                     HTTPS 播放中的清单内容也不可拦截。",
                                )
                                .size(10.0)
                                .color(theme::TEXT_TERTIARY),
                            )
                            .wrap(true),
                        );
                    });
            });

        // 把本帧编辑结果写回草稿，再执行动作。否则 AutoDetect/Import 等
        // 动作会读到上一帧的旧值。
        if self.settings.is_some() {
            self.settings = Some(draft);
        }
        if let Some(action) = action {
            self.apply_settings_action(action, ctx);
        }
        if !open {
            self.settings = None;
            self.confirm_clear_source = false;
        }
    }
}

// ---------------------------------------------------------------------- 小组件

fn paint_logo(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), Sense::hover());
    let painter = ui.painter();
    let screen = Rect::from_min_size(rect.min + Vec2::new(1.0, 2.0), Vec2::new(20.0, 14.0));
    painter.rect_stroke(screen, Rounding::same(3.0), Stroke::new(1.5, theme::ACCENT));
    painter.line_segment(
        [
            screen.center_bottom(),
            screen.center_bottom() + Vec2::new(-5.0, 4.0),
        ],
        Stroke::new(1.5, theme::ACCENT),
    );
    painter.line_segment(
        [
            screen.center_bottom(),
            screen.center_bottom() + Vec2::new(5.0, 4.0),
        ],
        Stroke::new(1.5, theme::ACCENT),
    );
    painter.line_segment(
        [
            screen.center_bottom() + Vec2::new(-7.0, 5.0),
            screen.center_bottom() + Vec2::new(7.0, 5.0),
        ],
        Stroke::new(1.5, theme::ACCENT),
    );
}

fn paint_tv_outline(painter: &egui::Painter, rect: Rect, color: Color32) {
    let screen = Rect::from_center_size(rect.center(), rect.size());
    painter.rect_stroke(screen, Rounding::same(6.0), Stroke::new(2.0, color));
    painter.line_segment(
        [
            screen.center_bottom(),
            screen.center_bottom() + Vec2::new(-12.0, 12.0),
        ],
        Stroke::new(2.0, color),
    );
    painter.line_segment(
        [
            screen.center_bottom(),
            screen.center_bottom() + Vec2::new(12.0, 12.0),
        ],
        Stroke::new(2.0, color),
    );
}

fn badge(ui: &mut egui::Ui, text: &str, fg: Color32, bg: Color32) {
    if text.trim().is_empty() {
        return;
    }
    let font = FontId::proportional(10.0);
    let galley = ui.fonts(|fonts| fonts.layout_no_wrap(text.to_owned(), font, fg));
    let size = galley.size() + Vec2::new(10.0, 4.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, Rounding::same(4.0), bg);
    ui.painter()
        .galley(rect.min + Vec2::new(5.0, 2.0), galley, fg);
}

fn latency_text(latency: LatencyState) -> String {
    match latency {
        LatencyState::Ok(ms) => format!("{ms}ms"),
        LatencyState::Failed => "超时".to_owned(),
        LatencyState::Unknown => String::new(),
    }
}

fn bar_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_owned()).size(12.5).color(theme::TEXT))
        .min_size(Vec2::new(66.0, 32.0))
        .fill(theme::BG_ELEVATED)
        .stroke(Stroke::new(1.0, theme::LINE))
        .rounding(Rounding::same(8.0))
}

fn bar_small_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text.to_owned())
            .size(11.0)
            .color(theme::TEXT_SECONDARY),
    )
    .min_size(Vec2::new(54.0, 26.0))
    .fill(theme::BG_ELEVATED)
    .stroke(Stroke::new(1.0, theme::LINE))
    .rounding(Rounding::same(7.0))
}

fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text.to_owned())
            .size(13.0)
            .strong()
            .color(theme::BG_ROOT),
    )
    .min_size(Vec2::new(86.0, 32.0))
    .fill(theme::ACCENT)
    .stroke(Stroke::NONE)
    .rounding(Rounding::same(8.0))
}

fn close_button() -> egui::Button<'static> {
    egui::Button::new(RichText::new("×").size(15.0).color(theme::TEXT_SECONDARY))
        .min_size(Vec2::new(30.0, 32.0))
        .fill(theme::BG_ELEVATED)
        .stroke(Stroke::new(1.0, theme::LINE))
        .rounding(Rounding::same(7.0))
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.add_space(3.0);
    ui.label(RichText::new(text).size(12.0).strong().color(theme::ACCENT));
    ui.add_space(2.0);
}

fn message_label(ui: &mut egui::Ui, text: &str, error: bool) {
    ui.add(
        egui::Label::new(RichText::new(text.to_owned()).size(10.5).color(if error {
            theme::ROSE
        } else {
            theme::AMBER
        }))
        .wrap(true),
    );
}
