//! 后台任务：测速、批量测速、播放前预检、自定义源导入。
//!
//! 所有任务都通过 mpsc 把结果送回 UI 线程，并用 `egui::Context` 主动请求
//! 重绘。UI 线程不做任何网络请求，低配平板上也不会卡住界面。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;

use eframe::egui;
use reqwest::blocking::Client;

use crate::models::{PreflightOutcome, SpeedInfo};
use crate::net;

#[derive(Debug)]
pub enum JobEvent {
    Speed {
        channel_key: String,
        /// 批量测速时带上批次号；单频道测速为 None。
        batch_id: Option<u64>,
        result: Result<SpeedInfo, String>,
    },
    BatchProgress {
        batch_id: u64,
        done: usize,
        total: usize,
    },
    BatchDone {
        batch_id: u64,
        total: usize,
        cancelled: bool,
    },
    Preflight {
        token: u64,
        channel_key: String,
        line_index: usize,
        outcome: PreflightOutcome,
    },
    Import {
        /// 导入任务的有效性标识；清除源或开始新导入后旧任务自动失效。
        token: u64,
        url: String,
        /// 成功时返回 (M3U 原文, 频道数)。
        result: Result<(String, usize), String>,
    },
}

/// 单频道测速。
pub fn spawn_speed_test(
    client: Client,
    url: String,
    referer: String,
    channel_key: String,
    tx: Sender<JobEvent>,
    ctx: egui::Context,
) {
    thread::spawn(move || {
        let result = net::test_speed(&client, &url, &referer);
        let _ = tx.send(JobEvent::Speed {
            channel_key,
            batch_id: None,
            result,
        });
        ctx.request_repaint();
    });
}

/// 批量测速。固定少量并发，避免低配平板同时打开太多连接。
#[allow(clippy::too_many_arguments)]
pub fn spawn_batch_test(
    client: Client,
    items: Vec<(String, String)>,
    referer: String,
    worker_count: usize,
    batch_id: u64,
    cancel: Arc<AtomicBool>,
    tx: Sender<JobEvent>,
    ctx: egui::Context,
) {
    let total = items.len();
    if total == 0 {
        let _ = tx.send(JobEvent::BatchDone {
            batch_id,
            total: 0,
            cancelled: false,
        });
        ctx.request_repaint();
        return;
    }

    let queue = Arc::new(Mutex::new(VecDeque::from(items)));
    let done = Arc::new(AtomicUsize::new(0));
    let worker_count = worker_count.clamp(1, 6).min(total);
    let mut handles = Vec::with_capacity(worker_count);

    for _ in 0..worker_count {
        let client = client.clone();
        let queue = queue.clone();
        let done = done.clone();
        let cancel = cancel.clone();
        let tx = tx.clone();
        let ctx = ctx.clone();
        let referer = referer.clone();

        handles.push(thread::spawn(move || loop {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let item = {
                let mut queue = queue
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                queue.pop_front()
            };
            let Some((channel_key, url)) = item else {
                break;
            };

            let result = net::test_speed(&client, &url, &referer);
            let _ = tx.send(JobEvent::Speed {
                channel_key,
                batch_id: Some(batch_id),
                result,
            });
            let completed = done.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = tx.send(JobEvent::BatchProgress {
                batch_id,
                done: completed.min(total),
                total,
            });
            ctx.request_repaint();
        }));
    }

    thread::spawn(move || {
        for handle in handles {
            let _ = handle.join();
        }
        let _ = tx.send(JobEvent::BatchDone {
            batch_id,
            total,
            cancelled: cancel.load(Ordering::Relaxed),
        });
        ctx.request_repaint();
    });
}

/// 播放前检查 URL 与 HLS 清单里的已知广告特征。
#[allow(clippy::too_many_arguments)]
pub fn spawn_preflight(
    client: Client,
    url: String,
    referer: String,
    ad_guard: bool,
    token: u64,
    channel_key: String,
    line_index: usize,
    tx: Sender<JobEvent>,
    ctx: egui::Context,
) {
    thread::spawn(move || {
        let outcome = net::preflight(&client, &url, ad_guard, &referer);
        let _ = tx.send(JobEvent::Preflight {
            token,
            channel_key,
            line_index,
            outcome,
        });
        ctx.request_repaint();
    });
}

/// 下载并解析自定义 M3U。真正的落盘由 UI 线程完成。
pub fn spawn_import(
    client: Client,
    url: String,
    token: u64,
    tx: Sender<JobEvent>,
    ctx: egui::Context,
) {
    thread::spawn(move || {
        let result =
            net::download_m3u(&client, &url).map(|(text, channels)| (text, channels.len()));
        let _ = tx.send(JobEvent::Import { token, url, result });
        ctx.request_repaint();
    });
}
