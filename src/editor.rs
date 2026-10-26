//! Reel-to-reel style editor: a cream faceplate with two reels and four knobs, painted by hand.

use crate::SpoolParams;
use nih_plug::prelude::*;
use nih_plug_egui::egui::{
    self, emath::TSTransform, Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke,
    StrokeKind, Ui, UiBuilder, Vec2,
};
use nih_plug_egui::{create_egui_editor, resizable_window::ResizableWindow};
use std::f32::consts::{PI, TAU};
use std::sync::Arc;

pub const WIDTH: u32 = 440;
pub const HEIGHT: u32 = 400;
const DESIGN: Rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(WIDTH as f32, HEIGHT as f32));

const KNOB_RADIUS: f32 = 28.0;
/// Knobs turn +-135 degrees from straight up
const SWEEP: f32 = 0.75 * PI;
const INK: Color32 = Color32::from_rgb(40, 34, 28);
const FACEPLATE: Color32 = Color32::from_rgb(224, 214, 192);
const TAPE: Color32 = Color32::from_rgb(70, 44, 28);

pub fn create(params: Arc<SpoolParams>) -> Option<Box<dyn Editor>> {
    create_egui_editor(
        params.editor_state.clone(),
        (),
        |_, _| {},
        move |ctx, setter, _| {
            ResizableWindow::new("resize")
                .min_size(DESIGN.size() / 2.0)
                .show(ctx, &params.editor_state, |ui| {
                    let window = ui.max_rect();
                    ui.painter()
                        .rect_filled(window, 0.0, Color32::from_gray(48));

                    // Draw at the design size on its own layer, then scale that layer (input
                    // included) to fit the window. The window corner stays in the margin so the
                    // resize handle underneath remains visible and grabbable.
                    let scale =
                        (window.width() / DESIGN.width()).min(window.height() / DESIGN.height());
                    let offset = window.center().to_vec2() - DESIGN.center().to_vec2() * scale;
                    let layer = egui::LayerId::new(egui::Order::Middle, egui::Id::new("deck"));
                    ctx.set_transform_layer(layer, TSTransform::new(offset, scale));
                    ui.scope_builder(UiBuilder::new().layer_id(layer).max_rect(DESIGN), |ui| {
                        ui.set_clip_rect(DESIGN);
                        deck(ui, setter, &params);
                    });
                });
        },
    )
}

fn deck(ui: &Ui, setter: &ParamSetter, params: &SpoolParams) {
    let body = ui.max_rect().shrink(12.0);
    let p = ui.painter();
    p.rect_filled(body, 10.0, FACEPLATE);
    p.rect_stroke(
        body,
        10.0,
        Stroke::new(2.0_f32, Color32::from_rgb(150, 138, 116)),
        StrokeKind::Inside,
    );
    for corner in [
        body.left_top(),
        body.right_top(),
        body.left_bottom(),
        body.right_bottom(),
    ] {
        screw(p, corner + (body.center() - corner).normalized() * 18.0);
    }

    // Supply reel full, take-up reel nearly empty, tape running under the head between them
    let reel_y = body.top() + 92.0;
    let supply = Pos2::new(body.left() + 100.0, reel_y);
    let takeup = Pos2::new(body.right() - 100.0, reel_y);
    let head = Rect::from_center_size(
        Pos2::new(body.center().x, reel_y + 62.0),
        Vec2::new(36.0, 18.0),
    );
    let tape = Stroke::new(2.0_f32, TAPE);
    p.line_segment([supply + Vec2::new(0.0, 54.0), head.left_bottom()], tape);
    p.line_segment([head.right_bottom(), takeup + Vec2::new(0.0, 32.0)], tape);
    reel(p, supply, 54.0);
    reel(p, takeup, 32.0);
    p.rect_filled(head, 3.0, Color32::from_gray(150));
    p.rect_stroke(
        head,
        3.0,
        Stroke::new(1.0_f32, Color32::from_gray(90)),
        StrokeKind::Inside,
    );

    lamp(
        ui,
        setter,
        &params.bypass,
        Pos2::new(body.center().x, body.top() + 44.0),
    );

    let logo = Pos2::new(body.center().x, body.top() + 200.0);
    // The default font has no bold weight, so fake one by overdrawing
    for offset in [
        Vec2::ZERO,
        Vec2::new(1.0, 0.0),
        Vec2::new(0.0, 1.0),
        Vec2::splat(1.0),
    ] {
        p.text(
            logo + offset,
            Align2::CENTER_CENTER,
            "SPOOL",
            FontId::proportional(40.0),
            INK,
        );
    }
    p.text(
        logo + Vec2::new(0.0, 28.0),
        Align2::CENTER_CENTER,
        "T A P E   M A C H I N E",
        FontId::proportional(11.0),
        INK,
    );

    for (i, param) in [&params.drive, &params.colour, &params.wear, &params.output]
        .into_iter()
        .enumerate()
    {
        let x = body.left() + body.width() * (i as f32 + 0.5) / 4.0;
        knob(ui, setter, param, Pos2::new(x, body.bottom() - 66.0));
    }
}

fn reel(p: &Painter, center: Pos2, pack: f32) {
    const RADIUS: f32 = 66.0;
    p.circle_filled(
        center + Vec2::new(0.0, 3.0),
        RADIUS,
        Color32::from_black_alpha(60),
    );
    p.circle_filled(center, RADIUS, Color32::from_gray(185));
    p.circle_stroke(
        center,
        RADIUS,
        Stroke::new(1.5_f32, Color32::from_gray(120)),
    );
    p.circle_filled(center, pack, TAPE);
    // Three windows in the flange
    for i in 0..3 {
        let a = i as f32 * TAU / 3.0;
        let window: Vec<Pos2> = (0..=12)
            .map(|j| a - 0.45 + 0.9 * j as f32 / 12.0)
            .flat_map(|b| [(b, RADIUS - 8.0), (b, 22.0)])
            .map(|(b, r)| center + dir(b) * r)
            .collect();
        // Draw each window as a fan of quads so concave edges stay clean
        for q in window.windows(4).step_by(2) {
            p.add(egui::Shape::convex_polygon(
                vec![q[0], q[1], q[3], q[2]],
                Color32::from_black_alpha(if pack > 40.0 { 40 } else { 90 }),
                Stroke::NONE,
            ));
        }
    }
    p.circle_filled(center, 16.0, Color32::from_gray(210));
    p.circle_stroke(center, 16.0, Stroke::new(1.0_f32, Color32::from_gray(110)));
    for i in 0..3 {
        let a = i as f32 * TAU / 3.0;
        p.line_segment(
            [center + dir(a) * 4.0, center + dir(a) * 12.0],
            Stroke::new(2.5_f32, Color32::from_gray(90)),
        );
    }
}

/// Unit vector for a knob angle, measured clockwise from straight up.
fn dir(angle: f32) -> Vec2 {
    Vec2::new(angle.sin(), -angle.cos())
}

fn knob(ui: &Ui, setter: &ParamSetter, param: &FloatParam, center: Pos2) {
    let rect = Rect::from_center_size(center, Vec2::splat(2.0 * KNOB_RADIUS + 12.0));
    let response = ui.interact(rect, ui.id().with(param.name()), Sense::click_and_drag());

    // Accumulate drags in egui memory since the param value can lag behind the host round trip
    if response.drag_started() {
        setter.begin_set_parameter(param);
        ui.data_mut(|d| d.insert_temp(response.id, param.unmodulated_normalized_value()));
    }
    if response.dragged() {
        let speed = if ui.input(|i| i.modifiers.shift) {
            0.0005
        } else {
            0.005
        };
        let delta = response.drag_delta();
        let start = ui
            .data(|d| d.get_temp(response.id))
            .unwrap_or(param.unmodulated_normalized_value());
        let value = (start + (delta.x - delta.y) * speed).clamp(0.0, 1.0);
        ui.data_mut(|d| d.insert_temp(response.id, value));
        setter.set_parameter_normalized(param, value);
    }
    if response.drag_stopped() {
        setter.end_set_parameter(param);
    }
    if response.double_clicked() {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, param.default_plain_value());
        setter.end_set_parameter(param);
    }

    if response.hovered() {
        let (notches, shift) = ui.input(|i| {
            let notches: f32 = i
                .events
                .iter()
                .map(|e| match e {
                    // Smooth-scrolling touchpads report points; treat ~50 points as one notch
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta,
                        ..
                    } => delta.y / 50.0,
                    egui::Event::MouseWheel { delta, .. } => delta.y,
                    _ => 0.0,
                })
                .sum();
            (notches, i.modifiers.shift)
        });
        if notches != 0.0 {
            let step = if shift { 0.002 } else { 0.02 };
            let value = (param.unmodulated_normalized_value() + notches * step).clamp(0.0, 1.0);
            setter.begin_set_parameter(param);
            setter.set_parameter_normalized(param, value);
            setter.end_set_parameter(param);
        }
    }

    let value = param.unmodulated_normalized_value();
    let angle = -SWEEP + 2.0 * SWEEP * value;
    let p = ui.painter();

    for i in 0..=10 {
        let a = -SWEEP + 2.0 * SWEEP * i as f32 / 10.0;
        let r = if i % 5 == 0 { 2.0 } else { 1.2 };
        p.circle_filled(center + dir(a) * (KNOB_RADIUS + 9.0), r, INK);
    }

    // Knurled skirt that turns with the knob
    p.circle_filled(
        center + Vec2::new(0.0, 3.0),
        KNOB_RADIUS,
        Color32::from_black_alpha(90),
    );
    p.circle_filled(center, KNOB_RADIUS, Color32::from_gray(8));
    for i in 0..40 {
        let a = angle + i as f32 * TAU / 40.0;
        p.line_segment(
            [
                center + dir(a) * (KNOB_RADIUS - 6.0),
                center + dir(a) * (KNOB_RADIUS - 0.5),
            ],
            Stroke::new(1.5_f32, Color32::from_gray(42)),
        );
    }
    p.circle_filled(center, KNOB_RADIUS - 7.0, Color32::from_gray(26));
    p.circle_stroke(
        center,
        KNOB_RADIUS - 7.0,
        Stroke::new(1.0_f32, Color32::from_gray(58)),
    );
    p.line_segment(
        [
            center + dir(angle) * 7.0,
            center + dir(angle) * (KNOB_RADIUS - 9.0),
        ],
        Stroke::new(3.0_f32, FACEPLATE),
    );

    p.text(
        center - Vec2::new(0.0, KNOB_RADIUS + 16.0),
        Align2::CENTER_BOTTOM,
        param.name().to_uppercase(),
        FontId::proportional(12.0),
        INK,
    );
    if response.hovered() || response.dragged() {
        p.text(
            center + Vec2::new(0.0, KNOB_RADIUS + 14.0),
            Align2::CENTER_TOP,
            param.normalized_value_to_string(value, true),
            FontId::proportional(11.0),
            Color32::from_rgb(110, 98, 80),
        );
    }
}

/// Amber pilot lamp that doubles as the bypass button: lit means the tape is running.
fn lamp(ui: &Ui, setter: &ParamSetter, bypass: &BoolParam, center: Pos2) {
    let rect = Rect::from_center_size(center, Vec2::splat(30.0));
    let response = ui.interact(rect, ui.id().with("lamp"), Sense::click());
    if response.clicked() {
        setter.begin_set_parameter(bypass);
        setter.set_parameter(bypass, !bypass.value());
        setter.end_set_parameter(bypass);
    }

    let p = ui.painter();
    p.circle_filled(center, 11.0, Color32::from_gray(150));
    p.circle_stroke(center, 11.0, Stroke::new(1.0_f32, Color32::from_gray(90)));
    if bypass.value() {
        p.circle_filled(center, 7.0, Color32::from_rgb(90, 52, 10));
    } else {
        p.circle_filled(
            center,
            16.0,
            Color32::from_rgba_unmultiplied(255, 170, 40, 50),
        );
        p.circle_filled(center, 7.0, Color32::from_rgb(255, 170, 40));
        p.circle_filled(
            center + Vec2::new(-2.0, -2.0),
            2.0,
            Color32::from_rgb(255, 230, 190),
        );
    }
    p.text(
        center + Vec2::new(0.0, 15.0),
        Align2::CENTER_TOP,
        if bypass.value() { "BYPASS" } else { "ON" },
        FontId::proportional(9.0),
        INK,
    );
}

fn screw(p: &Painter, center: Pos2) {
    p.circle_filled(center, 5.5, Color32::from_gray(150));
    p.circle_stroke(center, 5.5, Stroke::new(1.0_f32, Color32::from_gray(90)));
    p.line_segment(
        [center - dir(0.6) * 4.0, center + dir(0.6) * 4.0],
        Stroke::new(1.5_f32, Color32::from_gray(80)),
    );
}
