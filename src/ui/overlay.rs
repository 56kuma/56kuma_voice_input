//! The floating status window (eframe/egui). This file is the Humble Object:
//! it draws what `state_view` tells it to and nothing else.
//!
//! Design: a small translucent capsule at the top-center of the screen, one
//! glyph, no text. Sizes follow the silver ratio via [`Layout`]. It only
//! repaints while `StateView::needs_animation` is true, so Idle costs
//! nothing.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};

use super::state_view::{pulse_scale, Glyph, Layout, StateView};
use super::Overlay;
use crate::app::state::State;
use crate::audio::recorder::LevelMeter;
use crate::config::config::OverlayConfig;

/// Shared between the controller thread (writes state) and the UI thread.
pub struct OverlayModel {
    view: Mutex<StateView>,
    ctx: Mutex<Option<egui::Context>>,
    level: LevelMeter,
}

impl OverlayModel {
    pub fn new(level: LevelMeter) -> Arc<Self> {
        Arc::new(Self {
            view: Mutex::new(StateView::new()),
            ctx: Mutex::new(None),
            level,
        })
    }

    fn attach(&self, ctx: egui::Context) {
        *self.ctx.lock().unwrap() = Some(ctx);
    }
}

/// The `Overlay` handed to the controller.
#[derive(Clone)]
pub struct OverlayHandle(pub Arc<OverlayModel>);

impl Overlay for OverlayHandle {
    fn render(&self, state: State) {
        self.0.view.lock().unwrap().apply(state, Instant::now());
        if let Some(ctx) = self.0.ctx.lock().unwrap().as_ref() {
            ctx.request_repaint();
        }
    }
}

/// Runs the overlay on the calling (main) thread until the window closes.
/// `on_ready` runs once the egui context exists (used to register the
/// hotkey on the UI thread, which Windows requires).
pub fn run(
    model: Arc<OverlayModel>,
    config: OverlayConfig,
    on_ready: Box<dyn FnOnce() + Send>,
) -> eframe::Result {
    let layout = Layout::from_height(config.height);
    let viewport = egui::ViewportBuilder::default()
        .with_title("voice_input")
        .with_inner_size([layout.width, layout.height])
        .with_min_inner_size([layout.width, layout.height])
        .with_resizable(false)
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_taskbar(false)
        .with_active(false);
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let mut on_ready = Some(on_ready);
    eframe::run_native(
        "voice_input",
        options,
        Box::new(move |cc| {
            model.attach(cc.egui_ctx.clone());
            if let Some(f) = on_ready.take() {
                f();
            }
            Ok(Box::new(OverlayApp {
                model,
                config,
                layout,
                started: Instant::now(),
                positioned: false,
            }))
        }),
    )
}

struct OverlayApp {
    model: Arc<OverlayModel>,
    config: OverlayConfig,
    layout: Layout,
    started: Instant,
    positioned: bool,
}

// Palette: quiet greys plus one accent per meaning.
const BACKGROUND: Color32 = Color32::from_rgba_premultiplied(28, 28, 30, 200);
const IDLE: Color32 = Color32::from_rgb(174, 174, 178);
const RECORDING: Color32 = Color32::from_rgb(255, 69, 58);
const WORKING: Color32 = Color32::from_rgb(235, 235, 240);
const SUCCESS: Color32 = Color32::from_rgb(48, 209, 88);
const ERROR: Color32 = Color32::from_rgb(255, 159, 10);

impl OverlayApp {
    fn place_window(&mut self, ctx: &egui::Context) {
        if self.positioned {
            return;
        }
        let Some(monitor) = ctx.input(|i| i.viewport().monitor_size) else {
            return;
        };
        if monitor.x <= 0.0 {
            return;
        }
        let margin = self.config.margin;
        let x = match self.config.position.as_str() {
            "top-left" => margin,
            "top-right" => monitor.x - self.layout.width - margin,
            _ => (monitor.x - self.layout.width) / 2.0,
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(Pos2::new(x, margin)));
        self.positioned = true;
    }
}

impl eframe::App for OverlayApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.place_window(ctx);
        let now = Instant::now();
        let (glyph, animating) = {
            let view = self.model.view.lock().unwrap();
            (view.glyph(now), view.needs_animation(now))
        };
        let level = self.model.level.get();
        let t = now.duration_since(self.started);

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let painter = ui.painter();
                painter.rect_filled(rect, self.layout.corner_radius, BACKGROUND);
                draw_glyph(
                    painter,
                    rect.center(),
                    self.layout.glyph_size,
                    glyph,
                    t,
                    level,
                );

                // Double-click the capsule to quit (there is no other chrome).
                if ui.input(|i| {
                    i.pointer
                        .button_double_clicked(egui::PointerButton::Primary)
                }) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });

        if animating {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
    }
}

fn draw_glyph(
    painter: &egui::Painter,
    center: Pos2,
    size: f32,
    glyph: Glyph,
    t: Duration,
    level: f32,
) {
    let r = size / 2.0;
    match glyph {
        Glyph::Ring => {
            painter.circle_stroke(center, r * 0.8, Stroke::new(size / 8.0, IDLE));
        }
        Glyph::Dot => {
            let scale = pulse_scale(t, level);
            painter.circle_filled(center, r * 0.8 * scale, RECORDING);
        }
        Glyph::Dots => {
            let gap = size * std::f32::consts::SQRT_2 / 2.0;
            let phase = t.as_secs_f32() * 2.0;
            for i in 0..3 {
                let wave = 0.5 + 0.5 * (phase - i as f32 * 0.6).sin();
                let alpha = (90.0 + 165.0 * wave) as u8;
                let color =
                    Color32::from_rgba_unmultiplied(WORKING.r(), WORKING.g(), WORKING.b(), alpha);
                let x = center.x + (i as f32 - 1.0) * gap;
                painter.circle_filled(Pos2::new(x, center.y), size / 6.0, color);
            }
        }
        Glyph::Check => {
            let stroke = Stroke::new(size / 7.0, SUCCESS);
            let a = center + Vec2::new(-r * 0.7, 0.05 * r);
            let b = center + Vec2::new(-r * 0.2, r * 0.55);
            let c = center + Vec2::new(r * 0.75, -r * 0.55);
            painter.line_segment([a, b], stroke);
            painter.line_segment([b, c], stroke);
        }
        Glyph::Bang => {
            let stroke = Stroke::new(size / 6.0, ERROR);
            painter.line_segment(
                [
                    center + Vec2::new(0.0, -r * 0.8),
                    center + Vec2::new(0.0, r * 0.25),
                ],
                stroke,
            );
            painter.circle_filled(center + Vec2::new(0.0, r * 0.75), size / 11.0, ERROR);
        }
    }
    let _ = Rect::NOTHING;
}
