//! Reel-to-reel style editor: a cream faceplate with spinning reels, VU meters and knobs, painted
//! by hand.

use crate::{Meters, Preset, SpoolParams, PRESETS};
use nih_plug::prelude::*;
use nih_plug_egui::egui::{
    self, emath::TSTransform, Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke,
    StrokeKind, Ui, UiBuilder, Vec2,
};
use nih_plug_egui::{create_egui_editor, resizable_window::ResizableWindow};
use std::f32::consts::{PI, TAU};
use std::sync::Arc;

pub const WIDTH: u32 = 440;
pub const HEIGHT: u32 = 490;
const DESIGN: Rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(WIDTH as f32, HEIGHT as f32));

/// Knobs turn +-135 degrees from straight up
const SWEEP: f32 = 0.75 * PI;
const INK: Color32 = Color32::from_rgb(40, 34, 28);
const FACEPLATE: Color32 = Color32::from_rgb(224, 214, 192);
const TAPE: Color32 = Color32::from_rgb(70, 44, 28);

const REEL_RADIUS: f32 = 66.0;
const SUPPLY_PACK: f32 = 54.0;
const TAKEUP_PACK: f32 = 32.0;
/// Tape speed in design pixels per second; each reel turns at this over its pack radius
const TAPE_SPEED: f32 = 40.0;
/// Both reels look the same every third of a turn, so the tape position wraps here without a jump
const TAPE_WRAP: f32 = TAU / 3.0 * SUPPLY_PACK * TAKEUP_PACK;

/// VU needle swing either side of straight up, in radians
const VU_SWING: f32 = 0.7;
const VU_MIN: f32 = -20.0;
const VU_MAX: f32 = 3.0;

pub fn create(params: Arc<SpoolParams>, meters: Arc<Meters>) -> Option<Box<dyn Editor>> {
    create_egui_editor(
        params.editor_state.clone(),
        (),
        |_, _| {},
        move |ctx, setter, _| {
            // The meters and reels move without input, so redraw every frame
            ctx.request_repaint();
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
                        deck(ui, setter, &params, &meters);
                    });
                });
        },
    )
}

fn deck(ui: &Ui, setter: &ParamSetter, params: &SpoolParams, meters: &Meters) {
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
    p.line_segment(
        [supply + Vec2::new(0.0, SUPPLY_PACK), head.left_bottom()],
        tape,
    );
    p.line_segment(
        [head.right_bottom(), takeup + Vec2::new(0.0, TAKEUP_PACK)],
        tape,
    );

    // Tape position, advanced while the host plays. Tape leaves the bottom of the supply reel
    // heading right, so both reels turn anticlockwise.
    let travel_id = ui.id().with("tape-travel");
    let mut travel: f32 = ui.data(|d| d.get_temp(travel_id)).unwrap_or(0.0);
    if meters.running() {
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        travel = (travel + TAPE_SPEED * dt) % TAPE_WRAP;
        ui.data_mut(|d| d.insert_temp(travel_id, travel));
    }
    reel(p, supply, SUPPLY_PACK, -travel / SUPPLY_PACK);
    reel(p, takeup, TAKEUP_PACK, -travel / TAKEUP_PACK);
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
        Pos2::new(body.center().x, body.top() + 96.0),
    );

    let logo = Pos2::new(body.left() + 112.0, body.top() + 200.0);
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

    preset_selector(
        ui,
        setter,
        params,
        Pos2::new(body.center().x, body.top() + 38.0),
    );

    for (i, label) in ["L", "R"].into_iter().enumerate() {
        let x = body.right() - 152.0 + 92.0 * i as f32;
        let rect = Rect::from_center_size(Pos2::new(x, body.top() + 205.0), Vec2::new(84.0, 60.0));
        vu_meter(p, rect, meters.vu(i), label);
    }

    // Main controls, then the trims for each part of Wear underneath
    let rows = [
        (
            [
                &params.drive,
                &params.colour,
                &params.wear,
                &params.mix,
                &params.output,
            ],
            310.0,
            26.0,
        ),
        (
            [
                &params.wow,
                &params.flutter,
                &params.dropouts,
                &params.hiss,
                &params.elastic,
            ],
            408.0,
            18.0,
        ),
    ];
    for (row, y, radius) in rows {
        let n = row.len() as f32;
        for (i, param) in row.into_iter().enumerate() {
            let x = body.left() + body.width() * (i as f32 + 0.5) / n;
            knob(ui, setter, param, Pos2::new(x, body.top() + y), radius);
        }
    }
}

/// Position of the needle across the scale, 0..1. VU scales are linear in voltage, not dB.
fn vu_position(db: f32) -> f32 {
    let gain = |db: f32| 10f32.powf(db / 20.0);
    // Let it pin a little past +3 like a real meter
    ((gain(db) - gain(VU_MIN)) / (gain(VU_MAX) - gain(VU_MIN))).clamp(0.0, 1.05)
}

fn vu_meter(p: &Painter, rect: Rect, db: f32, label: &str) {
    p.rect_filled(rect, 4.0, Color32::from_rgb(238, 222, 160));
    // The pivot sits below the face, so the needle only shows its upper part
    let pivot = Pos2::new(rect.center().x, rect.bottom() + 8.0);
    let at = |db: f32, r: f32| pivot + dir(-VU_SWING + 2.0 * VU_SWING * vu_position(db)) * r;
    let red = Color32::from_rgb(190, 40, 30);

    let arc = |from: f32, to: f32| -> Vec<Pos2> {
        (0..=16)
            .map(|i| at(from + (to - from) * i as f32 / 16.0, 48.0))
            .collect()
    };
    p.add(egui::Shape::line(
        arc(VU_MIN, 0.0),
        Stroke::new(1.0_f32, INK),
    ));
    p.add(egui::Shape::line(
        arc(0.0, VU_MAX),
        Stroke::new(3.0_f32, red),
    ));
    for tick in [-20.0, -10.0, -7.0, -5.0, -3.0, -1.0, 0.0, 1.0, 2.0, 3.0] {
        let color = if tick > 0.0 { red } else { INK };
        p.line_segment(
            [at(tick, 48.0), at(tick, 53.0)],
            Stroke::new(1.0_f32, color),
        );
    }
    for (tick, text) in [
        (-20.0, "20"),
        (-10.0, "10"),
        (-5.0, "5"),
        (0.0, "0"),
        (3.0, "+3"),
    ] {
        p.text(
            at(tick, 58.0),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(7.0),
            if tick > 0.0 { red } else { INK },
        );
    }
    p.text(
        rect.left_bottom() + Vec2::new(6.0, -4.0),
        Align2::LEFT_BOTTOM,
        format!("VU {label}"),
        FontId::proportional(8.0),
        INK,
    );

    p.with_clip_rect(rect).line_segment(
        [pivot, at(db, 54.0)],
        Stroke::new(1.2_f32, Color32::from_gray(20)),
    );
    p.rect_stroke(
        rect,
        4.0,
        Stroke::new(2.0_f32, Color32::from_gray(70)),
        StrokeKind::Outside,
    );
}

/// `angle` turns the reel, clockwise in radians.
fn reel(p: &Painter, center: Pos2, pack: f32, angle: f32) {
    const RADIUS: f32 = REEL_RADIUS;
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
        let a = angle + i as f32 * TAU / 3.0;
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
        let a = angle + i as f32 * TAU / 3.0;
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

fn knob(ui: &Ui, setter: &ParamSetter, param: &FloatParam, center: Pos2, radius: f32) {
    let rect = Rect::from_center_size(center, Vec2::splat(2.0 * radius + 12.0));
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
        p.circle_filled(center + dir(a) * (radius + 9.0), r, INK);
    }

    // Knurled skirt that turns with the knob
    p.circle_filled(
        center + Vec2::new(0.0, 3.0),
        radius,
        Color32::from_black_alpha(90),
    );
    p.circle_filled(center, radius, Color32::from_gray(8));
    for i in 0..40 {
        let a = angle + i as f32 * TAU / 40.0;
        p.line_segment(
            [
                center + dir(a) * (radius - 6.0),
                center + dir(a) * (radius - 0.5),
            ],
            Stroke::new(1.5_f32, Color32::from_gray(42)),
        );
    }
    p.circle_filled(center, radius - 7.0, Color32::from_gray(26));
    p.circle_stroke(
        center,
        radius - 7.0,
        Stroke::new(1.0_f32, Color32::from_gray(58)),
    );
    p.line_segment(
        [
            center + dir(angle) * (0.25 * radius),
            center + dir(angle) * (0.68 * radius),
        ],
        Stroke::new(3.0_f32, FACEPLATE),
    );

    p.text(
        center - Vec2::new(0.0, radius + 16.0),
        Align2::CENTER_BOTTOM,
        param.name().to_uppercase(),
        FontId::proportional(if radius < 24.0 { 10.0 } else { 12.0 }),
        INK,
    );
    if response.hovered() || response.dragged() {
        p.text(
            center + Vec2::new(0.0, radius + 14.0),
            Align2::CENTER_TOP,
            param.normalized_value_to_string(value, true),
            FontId::proportional(11.0),
            Color32::from_rgb(110, 98, 80),
        );
    }
}

/// The preset's values for each float param, in plain units.
fn preset_values<'a>(params: &'a SpoolParams, p: &Preset) -> [(&'a FloatParam, f32); 9] {
    [
        (&params.drive, p.drive_db),
        (&params.colour, p.colour),
        (&params.wear, p.wear),
        (&params.output, util::db_to_gain(p.output_db)),
        (&params.wow, p.wow),
        (&params.flutter, p.flutter),
        (&params.dropouts, p.dropouts),
        (&params.hiss, p.hiss),
        (&params.elastic, p.elastic),
    ]
}

/// Index of the preset the current (unmodulated) settings match, if any.
fn matching_preset(params: &SpoolParams) -> Option<usize> {
    PRESETS.iter().position(|p| {
        // Compare normalized values so dB and percentage params share one tolerance
        preset_values(params, p).into_iter().all(|(param, value)| {
            (param.unmodulated_normalized_value() - param.preview_normalized(value)).abs() < 1e-3
        })
    })
}

fn apply_preset(setter: &ParamSetter, params: &SpoolParams, preset: &Preset) {
    for (param, value) in preset_values(params, preset) {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, value);
        setter.end_set_parameter(param);
    }
}

/// `‹ NAME ›` selector. Shows "CUSTOM" once the knobs no longer match a preset.
fn preset_selector(ui: &Ui, setter: &ParamSetter, params: &SpoolParams, center: Pos2) {
    let slot = Rect::from_center_size(center, Vec2::new(84.0, 16.0));
    let arrow_size = Vec2::new(16.0, slot.height());
    let prev_rect = Rect::from_min_size(slot.left_top(), arrow_size);
    let next_rect =
        Rect::from_min_size(slot.right_top() - Vec2::new(arrow_size.x, 0.0), arrow_size);
    let prev = ui.interact(
        prev_rect.expand(4.0),
        ui.id().with("preset-prev"),
        Sense::click(),
    );
    let next = ui.interact(
        next_rect.expand(4.0),
        ui.id().with("preset-next"),
        Sense::click(),
    );

    let current = matching_preset(params);
    // Remember the last pick so stepping from CUSTOM continues from where you were
    let memory_id = ui.id().with("preset-last");
    let last = current.or_else(|| ui.data(|d| d.get_temp::<usize>(memory_id)));
    let n = PRESETS.len();
    let step = if prev.clicked() {
        Some(last.map_or(n - 1, |i| (i + n - 1) % n))
    } else if next.clicked() {
        Some(last.map_or(0, |i| (i + 1) % n))
    } else {
        None
    };
    if let Some(i) = step {
        apply_preset(setter, params, &PRESETS[i]);
        ui.data_mut(|d| d.insert_temp(memory_id, i));
    }

    let p = ui.painter();
    p.rect_filled(slot, 7.0, Color32::from_gray(30));
    p.rect_stroke(
        slot,
        7.0,
        Stroke::new(1.0_f32, Color32::from_gray(90)),
        StrokeKind::Outside,
    );
    for (rect, response, flip) in [(prev_rect, &prev, -1.0), (next_rect, &next, 1.0)] {
        let c = rect.center();
        let color = if response.hovered() {
            FACEPLATE
        } else {
            Color32::from_gray(150)
        };
        let triangle = vec![
            c + Vec2::new(3.0 * flip, 0.0),
            c + Vec2::new(-2.0 * flip, -4.0),
            c + Vec2::new(-2.0 * flip, 4.0),
        ];
        p.add(egui::Shape::convex_polygon(triangle, color, Stroke::NONE));
    }
    let (name, color) = match current {
        Some(i) => (
            PRESETS[i].name.to_uppercase(),
            Color32::from_rgb(255, 190, 90),
        ),
        None => ("CUSTOM".to_string(), Color32::from_gray(150)),
    };
    p.text(
        slot.center(),
        Align2::CENTER_CENTER,
        name,
        FontId::proportional(10.0),
        color,
    );
    p.text(
        Pos2::new(center.x, slot.bottom() + 4.0),
        Align2::CENTER_TOP,
        "TAPE",
        FontId::proportional(9.0),
        INK,
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_fit_the_params_and_default_matches_the_first() {
        let params = SpoolParams::default();
        assert_eq!(matching_preset(&params), Some(0));
        for p in PRESETS {
            for (param, value) in preset_values(&params, p) {
                // Out-of-range values would be clamped on apply and never match again
                let range = param.preview_plain(0.0)..=param.preview_plain(1.0);
                assert!(
                    range.contains(&value),
                    "{} {}: {value}",
                    p.name,
                    param.name()
                );
            }
        }
    }
}
