//! Custom-painted Winamp-style widgets.

use egui::{
    Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui,
    Vec2, pos2, vec2,
};

use crate::audio::spectrum::BARS;
use crate::theme::Theme;

/// Inset "LCD" background.
pub fn lcd(ui: &Ui, rect: Rect, theme: &Theme) {
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, theme.bg_darker);
    p.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, theme.frame), StrokeKind::Inside);
}

/// Segment mask for a seven-segment cell: a=bit 0 (top), b, c, d (bottom), e, f, g=bit 6 (middle).
fn segments(c: char) -> u8 {
    match c {
        '0' => 0x3F,
        '1' => 0x06,
        '2' => 0x5B,
        '3' => 0x4F,
        '4' => 0x66,
        '5' => 0x6D,
        '6' => 0x7D,
        '7' => 0x07,
        '8' => 0x7F,
        '9' => 0x6F,
        '-' => 0x40,
        _ => 0,
    }
}

/// Cell rects for `text`, left to right, right-aligned 10 px from the rect's right edge.
/// Unsupported characters become blank digit cells.
fn lcd_cells(text: &str, rect: Rect) -> Vec<(char, Rect)> {
    let chars: Vec<char> = text
        .chars()
        .map(|c| if c.is_ascii_digit() || c == ':' || c == '-' { c } else { ' ' })
        .collect();
    let widths = |h: f32| -> Vec<f32> {
        chars.iter().map(|&c| if c == ':' { 0.25 * h } else { 0.55 * h }).collect()
    };
    let total = |h: f32| -> f32 {
        let n = chars.len();
        widths(h).iter().sum::<f32>() + 0.15 * h * n.saturating_sub(1) as f32
    };
    let mut h = rect.height() * 0.6;
    let avail = rect.width() - 36.0 - 10.0; // room left of the play glyph, minus the right margin
    let t = total(h);
    if t > avail && t > 0.0 {
        h *= avail / t;
    }
    let ws = widths(h);
    let gap = 0.15 * h;
    let top = rect.center().y - h / 2.0;
    let mut right = rect.right() - 10.0;
    let mut out = Vec::with_capacity(chars.len());
    for (&c, &w) in chars.iter().zip(&ws).rev() {
        out.push((c, Rect::from_min_size(pos2(right - w, top), vec2(w, h))));
        right -= w + gap;
    }
    out.reverse();
    out
}

/// Large seven-segment time readout; unlit segments stay faintly visible.
pub fn digits(ui: &mut Ui, rect: Rect, text: &str, theme: &Theme) -> Response {
    let resp = ui.allocate_rect(rect, Sense::click());
    lcd(ui, rect, theme);
    let p = ui.painter();
    let lit = theme.digits;
    let dim = theme.digits.gamma_multiply(0.12);
    for (c, cell) in lcd_cells(text, rect) {
        // Same thickness for every cell: 0.16 of a digit cell's width (0.55 of its height).
        let t = 0.16 * 0.55 * cell.height();
        if c == ':' {
            for f in [1.0 / 3.0, 2.0 / 3.0] {
                let ctr = pos2(cell.center().x, cell.top() + cell.height() * f);
                p.rect_filled(Rect::from_center_size(ctr, vec2(t, t)), CornerRadius::ZERO, lit);
            }
            continue;
        }
        let (l, r, tp, b) = (cell.left(), cell.right(), cell.top(), cell.bottom());
        let mid = cell.center().y;
        let half = cell.height() / 2.0 - t;
        let hbar = |y: f32| Rect::from_min_size(pos2(l + t, y), vec2(cell.width() - 2.0 * t, t));
        let vbar = |x: f32, y: f32| Rect::from_min_size(pos2(x, y), vec2(t, half));
        let segs = [
            hbar(tp),
            vbar(r - t, tp + t * 0.5),
            vbar(r - t, mid + t * 0.5),
            hbar(b - t),
            vbar(l, mid + t * 0.5),
            vbar(l, tp + t * 0.5),
            hbar(mid - t / 2.0),
        ];
        let mask = segments(c);
        for (i, s) in segs.iter().enumerate() {
            let col = if mask & (1 << i) != 0 { lit } else { dim };
            p.rect_filled(s.shrink(0.5), CornerRadius::ZERO, col);
        }
    }
    resp
}

/// Scrolling title. Scrolls only when the text does not fit.
pub fn marquee(ui: &mut Ui, rect: Rect, text: &str, elapsed: f32, theme: &Theme) {
    lcd(ui, rect, theme);
    let font = FontId::monospace(13.0);
    let painter = ui.painter().with_clip_rect(rect.shrink(2.0));
    let galley = painter.layout_no_wrap(text.to_string(), font.clone(), theme.text);
    let w = galley.size().x;
    let avail = rect.width() - 12.0;
    let y = rect.center().y - galley.size().y / 2.0;
    if w <= avail {
        painter.galley(pos2(rect.left() + 6.0, y), galley, theme.text);
        return;
    }
    let sep = "  ***  ";
    let gap = painter
        .layout_no_wrap(sep.to_string(), font.clone(), theme.text)
        .size()
        .x;
    let cycle = w + gap;
    // Hold still for a moment before scrolling starts.
    let t = (elapsed - 1.5).max(0.0);
    let off = (t * 40.0) % cycle;
    let full = format!("{text}{sep}{text}{sep}");
    let g = painter.layout_no_wrap(full, font, theme.text);
    painter.galley(pos2(rect.left() + 6.0 - off, y), g, theme.text);
}

pub fn spectrum(ui: &mut Ui, rect: Rect, bars: &[f32; BARS], peaks: &[f32; BARS], theme: &Theme) {
    lcd(ui, rect, theme);
    let p = ui.painter().with_clip_rect(rect.shrink(1.0));
    let inner = rect.shrink2(vec2(4.0, 3.0));
    let gap = 2.0;
    let bw = (inner.width() - gap * (BARS as f32 - 1.0)) / BARS as f32;
    // Segmented bars: draw in horizontal slices for the gradient look.
    let seg = 3.0;
    let n_seg = (inner.height() / seg).floor().max(1.0) as usize;
    for k in 0..BARS {
        let x = inner.left() + k as f32 * (bw + gap);
        let lit = (bars[k].clamp(0.0, 1.0) * n_seg as f32).round() as usize;
        for s in 0..lit {
            let t = s as f32 / n_seg as f32;
            let y1 = inner.bottom() - s as f32 * seg;
            let r = Rect::from_min_max(pos2(x, y1 - seg + 1.0), pos2(x + bw, y1));
            p.rect_filled(r, CornerRadius::ZERO, theme.spectrum_lo.lerp_to_gamma(theme.spectrum_hi, t));
        }
        if peaks[k] > 0.01 {
            let y = inner.bottom() - peaks[k].clamp(0.0, 1.0) * inner.height();
            p.line_segment([pos2(x, y), pos2(x + bw, y)], Stroke::new(2.0, theme.peak));
        }
    }
}

/// Oscilloscope line of mono samples (-1..1) inside an LCD.
pub fn scope(ui: &mut Ui, rect: Rect, samples: &[f32], theme: &Theme) {
    lcd(ui, rect, theme);
    let p = ui.painter().with_clip_rect(rect.shrink(1.0));
    let inner = rect.shrink2(vec2(4.0, 3.0));
    let c = inner.center().y;
    p.line_segment(
        [pos2(inner.left(), c), pos2(inner.right(), c)],
        Stroke::new(1.0, theme.spectrum_lo.gamma_multiply(0.3)),
    );
    p.line(scope_points(inner, samples), Stroke::new(1.0, theme.spectrum_hi));
}

fn scope_points(inner: Rect, samples: &[f32]) -> Vec<Pos2> {
    let c = inner.center().y;
    if samples.is_empty() {
        return vec![pos2(inner.left(), c), pos2(inner.right(), c)];
    }
    let w = inner.width().max(1.0) as usize;
    (0..=w)
        .map(|x| {
            let s = samples[x * (samples.len() - 1) / w];
            let px = inner.left() + inner.width() * x as f32 / w as f32;
            pos2(px, c - s.clamp(-1.0, 1.0) * inner.height() / 2.0)
        })
        .collect()
}

/// Small LED-style toggle button.
pub fn led_toggle(ui: &mut Ui, on: bool, label: &str, theme: &Theme) -> Response {
    let font = FontId::monospace(11.0);
    let color = if on { theme.text_bright } else { theme.text_dim };
    let g = ui.painter().layout_no_wrap(label.to_string(), font, color);
    let size = vec2(g.size().x + 20.0, 18.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    let bg = if resp.hovered() { theme.panel } else { theme.bg_dark };
    p.rect_filled(rect, CornerRadius::ZERO, bg);
    p.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, theme.frame), StrokeKind::Inside);
    let led = Rect::from_center_size(pos2(rect.left() + 8.0, rect.center().y), vec2(5.0, 5.0));
    p.rect_filled(led, CornerRadius::ZERO, if on { theme.accent } else { theme.muted.gamma_multiply(0.5) });
    p.galley(pos2(rect.left() + 14.0, rect.center().y - g.size().y / 2.0), g, color);
    resp
}

/// Flat transport button with an icon glyph.
pub fn button(ui: &mut Ui, size: Vec2, glyph: &str, active: bool, theme: &Theme) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    let (bg, fg) = if resp.is_pointer_button_down_on() {
        (theme.accent, theme.bg_darker)
    } else if resp.hovered() {
        (theme.selection, theme.text_bright)
    } else if active {
        (theme.panel, theme.accent)
    } else {
        (theme.bg_dark, theme.text)
    };
    p.rect_filled(rect, CornerRadius::ZERO, bg);
    p.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, theme.frame), StrokeKind::Inside);
    p.text(rect.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(14.0), fg);
    resp
}

/// Horizontal bar slider (seek / volume / balance). Returns the new fraction while
/// dragging or on click, plus the response.
pub fn bar(
    ui: &mut Ui,
    rect: Rect,
    frac: f32,
    centered: bool,
    theme: &Theme,
) -> (Response, Option<f32>) {
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    let p = ui.painter();
    let track = Rect::from_center_size(rect.center(), vec2(rect.width(), 6.0_f32.min(rect.height())));
    p.rect_filled(track, CornerRadius::ZERO, theme.bg_darker);
    p.rect_stroke(track, CornerRadius::ZERO, Stroke::new(1.0, theme.frame), StrokeKind::Inside);
    let f = frac.clamp(0.0, 1.0);
    let x = track.left() + f * track.width();
    let fill = if centered {
        let c = track.center().x;
        Rect::from_min_max(pos2(c.min(x), track.top() + 1.0), pos2(c.max(x), track.bottom() - 1.0))
    } else {
        Rect::from_min_max(track.left_top() + vec2(1.0, 1.0), pos2(x, track.bottom() - 1.0))
    };
    p.rect_filled(fill, CornerRadius::ZERO, theme.accent.gamma_multiply(0.7));
    let knob = Rect::from_center_size(pos2(x, rect.center().y), vec2(8.0, rect.height().min(14.0)));
    let kc = if resp.hovered() || resp.dragged() { theme.text_bright } else { theme.text };
    p.rect_filled(knob, CornerRadius::ZERO, kc);
    p.rect_stroke(knob, CornerRadius::ZERO, Stroke::new(1.0, theme.bg_darker), StrokeKind::Inside);

    let new = if resp.dragged() || resp.clicked() || resp.drag_started() {
        resp.interact_pointer_pos()
            .map(|pos: Pos2| ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0))
    } else {
        None
    };
    (resp, new)
}

pub fn small_label(ui: &Ui, pos: Pos2, align: Align2, text: &str, color: Color32) -> Rect {
    ui.painter()
        .text(pos, align, text, FontId::monospace(11.0), color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_points_span_and_clamp() {
        let inner = Rect::from_min_size(pos2(10.0, 5.0), vec2(100.0, 40.0));
        let flat = scope_points(inner, &[]);
        assert_eq!(flat.len(), 2);
        assert!(flat.iter().all(|p| p.y == inner.center().y));
        let pts = scope_points(inner, &[2.0, -2.0]);
        assert_eq!(pts.first().unwrap().x, inner.left());
        assert_eq!(pts.last().unwrap().x, inner.right());
        assert!(pts.iter().all(|p| (inner.top()..=inner.bottom()).contains(&p.y)));
    }

    #[test]
    fn segments_table() {
        assert_eq!(segments('8'), 0x7F);
        assert_eq!(segments('1'), 0x06);
        assert_eq!(segments('-'), 0x40);
        assert_eq!(segments(' '), 0);
        assert_eq!(segments('x'), 0);
    }

    #[test]
    fn lcd_cells_right_aligned_and_fit() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(180.0, 60.0));
        let cells = lcd_cells("12:34", rect);
        assert_eq!(cells.len(), 5);
        assert_eq!(cells[4].1.right(), rect.right() - 10.0);
        assert!(cells[2].1.width() < cells[0].1.width());
        let cells = lcd_cells("-1:23:45", rect);
        for (_, c) in &cells {
            assert!(rect.contains_rect(*c));
        }
        assert!(cells[0].1.left() >= rect.left() + 36.0 - 0.01);
    }
}
