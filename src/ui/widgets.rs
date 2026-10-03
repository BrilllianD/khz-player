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

/// Large monospace time readout.
pub fn digits(ui: &mut Ui, rect: Rect, text: &str, theme: &Theme) -> Response {
    let resp = ui.allocate_rect(rect, Sense::click());
    lcd(ui, rect, theme);
    ui.painter().text(
        rect.right_center() - vec2(10.0, 0.0),
        Align2::RIGHT_CENTER,
        text,
        FontId::monospace(rect.height() * 0.62),
        theme.digits,
    );
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
