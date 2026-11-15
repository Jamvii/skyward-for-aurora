//! The independent download manager.

use crate::app::{Action, DownloadState, Frame, Hit};
use crate::canvas::Canvas;
use crate::color::Rgba;
use crate::pages::Ctx;
use crate::widgets::{self, Row, Trail};

use crate::icons::Icon;

const GREEN: Rgba = Rgba::rgb(0x1a, 0x9e, 0x55);
const BLUE: Rgba = Rgba::rgb(0x2f, 0x80, 0xe8);
const RED: Rgba = Rgba::rgb(0xd1, 0x3a, 0x3a);

pub fn manager(cx: &Ctx) -> Frame {
    let (t, f, s, l) = (cx.t, cx.f, cx.state, cx.state.lang);
    let m = &t.m;
    let mut c = Canvas::band(m.w, m.h, 0..m.h, 0..m.w, t.pal.bg);
    let top = m.content_top;
    let mut out = Vec::new();
    let y = widgets::section_first(&mut c, t, f, top, l.pick(["下载", "下載", "Downloads"]));
    let items = &s.downloads.items;
    let mut rows = Vec::new();
    let mut details = Vec::new();
    let mut statuses = Vec::new();
    for d in items.iter().take(10) {
        let status = match &d.state {
            DownloadState::Downloading => {
                let progress = d.share().map_or(String::new(), |p| format!(" {:.0}%", p * 100.0));
                format!("{}{progress}", fmt_size(d.done))
            }
            DownloadState::Done => l.pick(["已完成", "已完成", "Done"]).to_string(),
            DownloadState::Failed(why) => why.clone(),
            DownloadState::Cancelled => l.pick(["已取消", "已取消", "Cancelled"]).to_string(),
        };
        let source = match d.source {
            crate::app::DownloadSource::Aera => l.pick(["浏览器", "瀏覽器", "Browser"]),
            crate::app::DownloadSource::Custom => l.pick(["链接", "連結", "Link"]),
        };
        details.push(format!("{source} · {}", d.path));
        statuses.push(status);
    }
    if items.is_empty() {
        rows.push(
            Row::new(l.pick(["还没有下载", "還沒有下載", "No downloads yet"]))
                .subtitle(l.pick(["浏览器触发的下载会出现在这里", "瀏覽器觸發的下載會出現在這裡", "Downloads started by the browser appear here"]))
                .icon(t.pal.subtitle, Icon::Install)
                .trail(Trail::None),
        );
    } else {
        for (i, d) in items.iter().take(10).enumerate() {
            rows.push(
                Row::new(&d.name)
                    .subtitle(&details[i])
                    .icon(if d.active() { BLUE } else { GREEN }, Icon::Install)
                    .trail(Trail::Value(&statuses[i])),
            );
        }
    }
    let list = widgets::list(&mut c, t, f, y, &rows);
    for (i, d) in items.iter().take(10).enumerate() {
        let action = match d.state {
            DownloadState::Downloading => Action::DownloadCancel(d.id),
            DownloadState::Failed(_) | DownloadState::Cancelled => Action::DownloadRetry(d.id),
            DownloadState::Done => continue,
        };
        out.push(Hit { rect: list.rows[i], action });
    }
    let mut y = list.bottom + m.card_gap;
    let add = widgets::list(
        &mut c,
        t,
        f,
        y,
        &[Row::new(l.pick(["添加下载链接", "新增下載連結", "Add a download link"]))
            .subtitle(l.pick(["输入 http:// 或 https:// 地址", "輸入 http:// 或 https:// 位址", "Type an http:// or https:// URL"]))
            .icon(BLUE, Icon::Install)
            .trail(Trail::Arrow)],
    );
    out.push(Hit { rect: add.rows[0], action: Action::OpenDownloadLink });
    y = add.bottom + m.card_gap;
    if items.iter().any(|d| !d.active()) {
        let clear = widgets::list(
            &mut c,
            t,
            f,
            y,
            &[Row::new(l.pick(["清理已完成", "清理已完成", "Clear finished"]))
                .icon(RED, Icon::Wipe)
                .trail(Trail::None)],
        );
        out.push(Hit { rect: clear.rows[0], action: Action::DownloadClear });
        y = clear.bottom;
    }
    Frame::new(t, c, out, l.pick(["下载", "下載", "Downloads"]), top)
}

pub fn link(cx: &Ctx) -> Frame {
    let (t, f, s, l) = (cx.t, cx.f, cx.state, cx.state.lang);
    let mut c = Canvas::band(t.m.w, t.m.h, 0..t.m.h, 0..t.m.w, t.pal.bg);
    let (at, cancel) = widgets::search_layout(t);
    let field = widgets::search_field(&mut c, t, f, at.x, at.w, at.y + at.h / 2.0, &s.downloads.address, "http:// 或 https://");
    widgets::cancel_button(&mut c, t, cancel);
    let mut out = vec![Hit { rect: cancel, action: Action::Back }];
    let note = s.downloads.error.as_deref().unwrap_or(l.pick([
        "下载将保存到 /sdcard/AERA/Downloads",
        "下載將儲存到 /sdcard/AERA/Downloads",
        "Downloads are saved to /sdcard/AERA/Downloads",
    ]));
    let top = field.field.y + field.field.h + t.m.dp(12.0);
    widgets::footnote(&mut c, t, f, top, note);
    let mut keys = Vec::new();
    crate::pages::keyboard_hits(&mut c, t, f, s, l.pick(["下载", "下載", "Download"]), &mut keys);
    out.extend(keys);
    if let Some(rect) = field.clear {
        out.push(Hit { rect, action: Action::ClearSearch });
    }
    Frame { back: false, header: false, caret: Some(field.caret), ..Frame::new(t, c, out, l.pick(["下载", "下載", "Downloads"]), 0.0) }
}

fn fmt_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / MB)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}
