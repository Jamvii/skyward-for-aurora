//! The AERA WPE browser: its frame, a floating back button and a two-row
//! control bar at the bottom.

use crate::app::{Action, AeraPanel, Frame, Hit};
use crate::canvas::Canvas;
use crate::color::Rgba;
use crate::geom::Rect;
use crate::icons::{self, Icon};
use crate::pages::Ctx;
use crate::widgets;

const BLACK: Rgba = Rgba::rgb(0, 0, 0);
const WHITE: Rgba = Rgba::rgb(255, 255, 255);
const INK: Rgba = Rgba::rgb(24, 26, 30);

/// The browser page: the worker's frame under a compact bottom toolbar.
pub fn browser(cx: &Ctx) -> Frame {
    let (t, f, s, l) = (cx.t, cx.f, cx.state, cx.state.lang);
    let m = &t.m;
    let (w, h) = (m.w as f32, m.h as f32);
    let status_h = m.dp(48.0);
    let mut c = Canvas::band(m.w, m.h, 0..m.h, 0..m.w, BLACK);
    let mut out = Vec::new();

    // The system status bar keeps its own white area; the page starts below.
    c.fill_rect(0.0, 0.0, w, status_h, Rgba::rgb(250, 250, 252));
    let bar_h = m.dp(112.0);
    let area = Rect::new(0.0, status_h, w, h - status_h - bar_h);
    let shown = frame_rect(area);
    if let Some(frame) = &s.aera.frame {
        draw_frame(&mut c, frame, area);
    }

    // A white round back button floats under the status bar.
    let back = Rect::new(m.dp(16.0), status_h + m.dp(12.0), m.dp(44.0), m.dp(44.0));
    c.fill_circle(back.x + back.w / 2.0, back.y + back.h / 2.0, back.w / 2.0, WHITE);
    c.text_center(&f.value, "<", back.x + back.w / 2.0, back.y + back.h / 2.0, m.dp(22.0), INK);
    out.push(Hit { rect: back, action: Action::Back });

    // Page touches at the worker's 360x700 view coordinates.
    let (cols, rows) = (12, 20);
    if shown.w > 0.0 && shown.h > 0.0 {
        for row in 0..rows {
            for col in 0..cols {
                let x = shown.x + shown.w * col as f32 / cols as f32;
                let y = shown.y + shown.h * row as f32 / rows as f32;
                let r = Rect::new(x, y, shown.w / cols as f32 + 1.0, shown.h / rows as f32 + 1.0);
                let vx = (360.0 * (col as f32 + 0.5) / cols as f32) as u16;
                let vy = (700.0 * (row as f32 + 0.5) / rows as f32) as u16;
                out.push(Hit { rect: r, action: Action::AeraTap(vx, vy) });
            }
        }
    }

    // Two rows: address, then browser history, navigation and downloads.
    let bar = Rect::new(0.0, h - bar_h, w, bar_h);
    c.fill_rect(bar.x, bar.y, bar.w, bar.h, t.pal.card);
    let field = widgets::search_field(
        &mut c,
        t,
        f,
        m.margin,
        w - m.margin * 2.0,
        bar.y + m.dp(30.0),
        &s.aera.address,
        l.pick(["输入网址或搜索", "輸入網址或搜尋", "Address or search"]),
    );
    out.push(Hit { rect: field.field, action: Action::AeraAddress });
    let buttons = [
        (Button::Back, Action::AeraBack),
        (Button::Forward, Action::AeraForward),
        (Button::Reload, Action::AeraReload),
        (Button::History, Action::AeraPanelOpen(AeraPanel::History)),
        (Button::Bookmark, Action::AeraPanelOpen(AeraPanel::Bookmarks)),
        (Button::Download, Action::AeraPanelOpen(AeraPanel::Downloads)),
    ];
    let gap = m.dp(8.0);
    let button_w = (w - m.margin * 2.0 - gap * 5.0) / 6.0;
    for (k, (kind, action)) in buttons.into_iter().enumerate() {
        let r = Rect::new(m.margin + k as f32 * (button_w + gap), bar.y + m.dp(64.0), button_w, m.dp(38.0));
        button_icon(&mut c, t, kind, r);
        out.push(Hit { rect: r, action });
    }

    let status = s.aera.failed.as_deref().unwrap_or(&s.aera.status);
    if !status.is_empty() && s.aera.panel.is_none() && !s.aera.address_open {
        c.text(&f.value, status, m.margin, bar.y - m.dp(8.0), m.dp(11.0), t.pal.subtitle, Some(w - m.margin * 2.0));
    }

    // The address, edited in a bottom sheet rather than another page.
    if s.aera.address_open {
        let keyboard = widgets::keyboard_height(t);
        let top = h - keyboard - m.dp(76.0);
        c.fill_round_rect(m.margin, top, w - m.margin * 2.0, m.dp(68.0), m.dp(18.0), t.pal.card);
        let field = widgets::search_field(
            &mut c,
            t,
            f,
            m.margin + m.dp(10.0),
            w - m.margin * 2.0 - m.dp(20.0),
            top + m.dp(34.0),
            &s.aera.address,
            "输入网址",
        );
        out.push(Hit { rect: field.field, action: Action::AeraAddress });
        c.text_center(&f.value, "x", w - m.margin - m.dp(18.0), top - m.dp(18.0), m.dp(20.0), t.pal.text);
        out.push(Hit { rect: Rect::new(w - m.margin - m.dp(36.0), top - m.dp(36.0), m.dp(36.0), m.dp(36.0)), action: Action::Back });
        let mut keys = Vec::new();
        crate::pages::keyboard_hits(&mut c, t, f, s, l.pick(["打开", "打開", "Open"]), &mut keys);
        out.extend(keys);
    } else if s.aera.keyboard.is_some() {
        let mut keys = Vec::new();
        crate::pages::keyboard_hits(&mut c, t, f, s, l.pick(["完成", "完成", "Done"]), &mut keys);
        out.extend(keys);
        let close = Rect::new(w - m.margin - m.dp(36.0), h - widgets::keyboard_height(t) - m.dp(48.0), m.dp(36.0), m.dp(36.0));
        c.fill_circle(close.x + close.w / 2.0, close.y + close.h / 2.0, close.w / 2.0, t.pal.card);
        c.text_center(&f.value, "x", close.x + close.w / 2.0, close.y + close.h / 2.0, m.dp(18.0), t.pal.text);
        out.push(Hit { rect: close, action: Action::AeraKeyboardHide });
    }

    // History, bookmarks and downloads open over the page.
    if let Some(panel) = s.aera.panel {
        let top = h - m.dp(360.0);
        c.fill_round_rect(0.0, top, w, h - top, m.dp(22.0), t.pal.card);
        let title = match panel {
            AeraPanel::History => "历史",
            AeraPanel::Bookmarks => "书签",
            AeraPanel::Downloads => "下载",
        };
        c.text(&f.value, title, m.margin, top + m.dp(30.0), m.dp(20.0), t.pal.text, Some(w - m.margin * 2.0));
        c.text_center(&f.value, "x", w - m.margin, top + m.dp(24.0), m.dp(20.0), t.pal.text);
        out.push(Hit { rect: Rect::new(w - m.margin - m.dp(32.0), top, m.dp(32.0), m.dp(48.0)), action: Action::AeraPanelClose });
        match panel {
            AeraPanel::History | AeraPanel::Bookmarks => {
                let entries: Vec<(usize, String)> = if panel == AeraPanel::History {
                    s.aera.history.iter().enumerate().rev().take(8).map(|(i, u)| (i, u.clone())).collect()
                } else {
                    s.aera.bookmarks.iter().enumerate().rev().take(8).map(|(i, u)| (i, u.clone())).collect()
                };
                if entries.is_empty() {
                    c.text(&f.value, "暂无记录", m.margin, top + m.dp(70.0), m.dp(14.0), t.pal.subtitle, None);
                }
                for (k, (index, url)) in entries.into_iter().enumerate() {
                    let y = top + m.dp(58.0 + 34.0 * k as f32);
                    c.text(&f.value, &url, m.margin, y + m.dp(20.0), m.dp(13.0), t.pal.text, Some(w - m.margin * 2.0));
                    out.push(Hit {
                        rect: Rect::new(m.margin, y, w - m.margin * 2.0, m.dp(32.0)),
                        action: if panel == AeraPanel::History { Action::AeraHistoryOpen(index) } else { Action::AeraBookmarkOpen(index) },
                    });
                }
                if panel == AeraPanel::Bookmarks {
                    let add = Rect::new(m.margin, top + m.dp(320.0), w - m.margin * 2.0, m.dp(34.0));
                    c.fill_round_rect(add.x, add.y, add.w, add.h, add.h / 2.0, t.pal.bg);
                    c.text_center(&f.value, "收藏当前页面", add.x + add.w / 2.0, add.y + add.h / 2.0, m.dp(14.0), t.pal.text);
                    out.push(Hit { rect: add, action: Action::AeraBookmarkCurrent });
                }
            }
            AeraPanel::Downloads => {
                c.text(&f.value, "下载保存到 /sdcard/AERA/Downloads", m.margin, top + m.dp(70.0), m.dp(14.0), t.pal.subtitle, None);
            }
        }
    }

    let mut page = Frame::new(t, c, out, "AERA", 0.0);
    page.back = false;
    page.header = false;
    page
}

#[derive(Clone, Copy)]
enum Button {
    Back,
    Forward,
    Reload,
    History,
    Bookmark,
    Download,
}

fn button_icon(c: &mut Canvas, t: &crate::theme::Theme, kind: Button, r: Rect) {
    let m = &t.m;
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let ink = t.pal.text;
    let s = m.dp(9.0);
    match kind {
        Button::Back => {
            c.line(cx + s * 0.5, cy - s, cx - s * 0.5, cy, m.dp(2.0), ink);
            c.line(cx - s * 0.5, cy, cx + s * 0.5, cy + s, m.dp(2.0), ink);
        }
        Button::Forward => {
            c.line(cx - s * 0.5, cy - s, cx + s * 0.5, cy, m.dp(2.0), ink);
            c.line(cx + s * 0.5, cy, cx - s * 0.5, cy + s, m.dp(2.0), ink);
        }
        Button::Reload => {
            c.stroke_circle(cx, cy, s, m.dp(2.0), ink);
            c.line(cx - s, cy - s * 0.2, cx - s * 0.4, cy - s * 0.8, m.dp(2.0), ink);
        }
        Button::History => icons::glyph(c, Icon::Clock, cx, cy, m.dp(24.0), ink),
        Button::Bookmark => {
            c.line(cx - s * 0.7, cy - s, cx - s * 0.7, cy + s, m.dp(2.0), ink);
            c.line(cx + s * 0.7, cy - s, cx + s * 0.7, cy + s, m.dp(2.0), ink);
            c.line(cx - s * 0.7, cy - s, cx + s * 0.7, cy - s, m.dp(2.0), ink);
            c.line(cx - s * 0.7, cy + s, cx + s * 0.7, cy + s, m.dp(2.0), ink);
            c.line(cx - s * 0.7, cy + s, cx, cy + s * 0.3, m.dp(2.0), ink);
            c.line(cx + s * 0.7, cy + s, cx, cy + s * 0.3, m.dp(2.0), ink);
        }
        Button::Download => {
            c.line(cx, cy - s, cx, cy + s * 0.4, m.dp(2.0), ink);
            c.line(cx - s * 0.6, cy - s * 0.2, cx, cy + s * 0.4, m.dp(2.0), ink);
            c.line(cx + s * 0.6, cy - s * 0.2, cx, cy + s * 0.4, m.dp(2.0), ink);
            c.line(cx - s, cy + s, cx + s, cy + s, m.dp(2.0), ink);
        }
    }
}

/// Where a screen point lies in AERA's 360x700 view.
pub(crate) fn view_point(m: &crate::theme::Metrics, x: f32, y: f32) -> Option<(u16, u16)> {
    let status = m.dp(48.0);
    let area = Rect::new(0.0, status, m.w as f32, m.h as f32 - status - m.dp(112.0));
    let shown = frame_rect(area);
    if !shown.contains(x, y) {
        return None;
    }
    let x = ((x - shown.x) / shown.w * 360.0).clamp(0.0, 359.0) as u16;
    let y = ((y - shown.y) / shown.h * 700.0).clamp(0.0, 699.0) as u16;
    Some((x, y))
}

fn frame_rect(area: Rect) -> Rect {
    let scale = (area.w / 1080.0).min(area.h / 2100.0);
    let w = 1080.0 * scale;
    let h = 2100.0 * scale;
    Rect::new(area.x + (area.w - w) / 2.0, area.y + (area.h - h) / 2.0, w, h)
}

fn draw_frame(c: &mut Canvas, frame: &[u8], area: Rect) {
    if frame.len() != 1080 * 2100 * 4 {
        return;
    }
    let shown = frame_rect(area);
    let dw = shown.w as usize;
    let dh = shown.h as usize;
    if dw == 0 || dh == 0 {
        return;
    }
    let cw = c.w as usize;
    let xmap: Vec<usize> = (0..dw).map(|dx| dx * 1080 / dw).collect();
    let ymap: Vec<usize> = (0..dh).map(|dy| dy * 2100 / dh).collect();
    let x0 = shown.x as usize;
    let y0 = shown.y as usize;
    for dy in 0..dh {
        let src = ymap[dy] * 1080 * 4;
        let dst_row = (y0 + dy) * cw * 4;
        for dx in 0..dw {
            let s = src + xmap[dx] * 4;
            let d = dst_row + (x0 + dx) * 4;
            c.data[d] = frame[s + 2];
            c.data[d + 1] = frame[s + 1];
            c.data[d + 2] = frame[s];
            c.data[d + 3] = 255;
        }
    }
}

/// Where the address is typed.
pub fn address(cx: &Ctx) -> Frame {
    let (t, f, s, l) = (cx.t, cx.f, cx.state, cx.state.lang);
    let mut c = Canvas::band(t.m.w, t.m.h, 0..t.m.h, 0..t.m.w, t.pal.bg);
    let (at, cancel) = widgets::search_layout(t);
    let field = widgets::search_field(&mut c, t, f, at.x, at.w, at.y + at.h / 2.0, &s.aera.address, "输入网址");
    widgets::cancel_button(&mut c, t, cancel);
    let mut out = vec![Hit { rect: cancel, action: Action::Back }];
    let top = field.field.y + field.field.h + t.m.dp(12.0);
    widgets::footnote(&mut c, t, f, top, l.pick(["只接受 http:// 或 https:// 地址，裸域名会补 https://。", "只接受 http:// 或 https:// 地址，裸域名會補 https://。", "Only http:// or https:// addresses; a bare name gets https://."]));
    let mut keys = Vec::new();
    crate::pages::keyboard_hits(&mut c, t, f, s, l.pick(["打开", "打開", "Open"]), &mut keys);
    out.extend(keys);
    if let Some(rect) = field.clear {
        out.push(Hit { rect, action: Action::ClearSearch });
    }
    Frame { back: false, header: false, caret: Some(field.caret), ..Frame::new(t, c, out, "AERA", 0.0) }
}
