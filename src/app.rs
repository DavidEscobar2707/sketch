//! The windowed application: winit event loop, wgpu surface, egui integration, file dialogs.

use crate::editor::{AppRequest, Button, Editor, InputEvent, Key, Modifiers};
use crate::geom::{V2, v2};
use crate::io::{file, png, svg};
use crate::model::{Color, Document};
use crate::render::frame::{self, Painter};
use crate::ui::{self, UiState};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WKey, NamedKey};
use winit::window::{Window, WindowId};

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    painter: Painter,
    egui: egui_winit::State,
}

pub struct App {
    gfx: Option<Gfx>,
    pub ed: Editor,
    st: UiState,
    ctx: egui::Context,
    start: Instant,
    canvas: egui::Rect,
    captured: bool,
    title: String,
}

impl App {
    pub fn new(ed: Editor) -> App {
        let ctx = egui::Context::default();
        ui::setup_style(&ctx);
        App {
            gfx: None,
            ed,
            st: UiState::default(),
            ctx,
            start: Instant::now(),
            canvas: egui::Rect::NOTHING,
            captured: false,
            title: String::new(),
        }
    }

    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }

    fn to_canvas(&self, p: V2) -> V2 {
        p - v2(self.canvas.min.x as f64, self.canvas.min.y as f64)
    }

    fn over_canvas(&self, p: V2) -> bool {
        self.canvas.contains(egui::pos2(p.x as f32, p.y as f32)) && !self.ctx.is_pointer_over_egui()
    }

    fn confirm_discard(&mut self) -> bool {
        if !self.ed.is_dirty() {
            return true;
        }
        let r = rfd::MessageDialog::new()
            .set_title("Unsaved changes")
            .set_description(format!("Save changes to \"{}\" first?", self.ed.doc.name))
            .set_buttons(rfd::MessageButtons::YesNoCancel)
            .show();
        match r {
            rfd::MessageDialogResult::Yes => self.save(false),
            rfd::MessageDialogResult::No => true,
            _ => false,
        }
    }

    fn error(&mut self, msg: String) {
        log::error!("{msg}");
        self.ed.status = msg.clone();
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("paper-rs")
            .set_description(msg)
            .show();
    }

    fn save(&mut self, save_as: bool) -> bool {
        let path = match (&self.ed.file_path, save_as) {
            (Some(p), false) => p.clone(),
            _ => match rfd::FileDialog::new()
                .add_filter("paper-rs document", &[file::EXTENSION])
                .set_file_name(format!("{}.{}", self.ed.doc.name, file::EXTENSION))
                .save_file()
            {
                Some(p) => with_ext(p, file::EXTENSION),
                None => return false,
            },
        };
        match file::save(&path, &self.ed.doc, Some(&self.ed.camera)) {
            Ok(()) => {
                self.ed.mark_saved();
                self.ed.status = format!("Saved {}", path.display());
                self.ed.file_path = Some(path);
                true
            }
            Err(e) => {
                self.error(format!("Could not save: {e}"));
                false
            }
        }
    }

    fn export_roots(&self) -> Option<Vec<crate::model::NodeId>> {
        let sel = self.ed.top_selection();
        (!sel.is_empty()).then_some(sel)
    }

    fn handle_requests(&mut self) {
        for req in std::mem::take(&mut self.ed.requests) {
            match req {
                AppRequest::New => {
                    if self.confirm_discard() {
                        self.ed.load(Document::default(), None, None);
                    }
                }
                AppRequest::Open => {
                    if !self.confirm_discard() {
                        continue;
                    }
                    let Some(p) = rfd::FileDialog::new()
                        .add_filter("paper-rs document", &[file::EXTENSION])
                        .pick_file()
                    else {
                        continue;
                    };
                    match file::load(&p) {
                        Ok((doc, cam)) => {
                            self.ed.load(doc, cam, Some(p.clone()));
                            self.ed.status = format!("Opened {}", p.display());
                        }
                        Err(e) => self.error(format!("Could not open {}: {e}", p.display())),
                    }
                }
                AppRequest::Save => {
                    self.save(false);
                }
                AppRequest::SaveAs => {
                    self.save(true);
                }
                AppRequest::ExportPng => {
                    let Some(p) = rfd::FileDialog::new()
                        .add_filter("PNG image", &["png"])
                        .set_file_name(format!("{}.png", self.ed.doc.name))
                        .save_file()
                    else {
                        continue;
                    };
                    let p = with_ext(p, "png");
                    let bg = (!self.st.export_transparent).then_some(Color::WHITE);
                    let roots = self.export_roots();
                    match png::export(
                        &p,
                        &self.ed.doc,
                        roots.as_deref(),
                        self.st.export_scale.max(1) as f64,
                        bg,
                    ) {
                        Ok((w, h)) => {
                            self.ed.status = format!("Exported {w}×{h} PNG to {}", p.display())
                        }
                        Err(e) => self.error(format!("PNG export failed: {e}")),
                    }
                }
                AppRequest::ExportSvg => {
                    let Some(p) = rfd::FileDialog::new()
                        .add_filter("SVG image", &["svg"])
                        .set_file_name(format!("{}.svg", self.ed.doc.name))
                        .save_file()
                    else {
                        continue;
                    };
                    let p = with_ext(p, "svg");
                    let roots = self.export_roots();
                    match svg::export(&self.ed.doc, roots.as_deref(), None) {
                        Some(s) => match std::fs::write(&p, s) {
                            Ok(()) => self.ed.status = format!("Exported SVG to {}", p.display()),
                            Err(e) => self.error(format!("SVG export failed: {e}")),
                        },
                        None => self.ed.status = "Nothing to export".into(),
                    }
                }
            }
        }
    }

    fn redraw(&mut self) {
        let Some(g) = self.gfx.as_mut() else { return };
        let input = g.egui.take_egui_input(&g.window);
        let mut l = frame::layout(&self.ctx, input, &mut self.ed, &mut self.st);
        self.canvas = l.canvas;
        let platform = std::mem::take(&mut l.output.platform_output);
        g.egui.handle_platform_output(&g.window, platform);

        let frame = match g.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Occluded => {
                // macOS reports Occluded until the window is on screen. Keep egui's texture
                // state in sync, but don't spin: WindowEvent::Occluded(false) triggers a redraw.
                log::debug!("surface occluded; skipping frame");
                g.painter
                    .skip_frame(&g.device, &g.queue, &mut l.output.textures_delta);
                return;
            }
            other => {
                log::warn!("surface unavailable: {other:?}");
                g.surface.configure(&g.device, &g.config);
                g.painter
                    .skip_frame(&g.device, &g.queue, &mut l.output.textures_delta);
                g.window.request_redraw();
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let size = [g.config.width, g.config.height];
        g.painter.paint(
            &g.device,
            &g.queue,
            &view,
            size,
            &self.ctx,
            l,
            &self.ed,
            !self.st.show_grid_off,
        );
        g.window.pre_present_notify();
        g.queue.present(frame);

        let title = format!(
            "{}{} — paper-rs",
            self.ed.doc.name,
            if self.ed.is_dirty() { " •" } else { "" }
        );
        if title != self.title {
            g.window.set_title(&title);
            self.title = title;
        }
        self.handle_requests();
        if (self.ctx.has_requested_repaint() || !self.ed.requests.is_empty())
            && let Some(g) = &self.gfx
        {
            g.window.request_redraw();
        }
    }
}

fn with_ext(p: PathBuf, ext: &str) -> PathBuf {
    if p.extension().is_some_and(|e| e == ext) {
        p
    } else {
        p.with_extension(ext)
    }
}

fn map_key(k: &WKey) -> Option<Key> {
    Some(match k {
        WKey::Named(NamedKey::Escape) => Key::Escape,
        WKey::Named(NamedKey::Enter) => Key::Enter,
        WKey::Named(NamedKey::Backspace) => Key::Backspace,
        WKey::Named(NamedKey::Delete) => Key::Delete,
        WKey::Named(NamedKey::Tab) => Key::Tab,
        WKey::Named(NamedKey::Space) => Key::Space,
        WKey::Named(NamedKey::ArrowLeft) => Key::Left,
        WKey::Named(NamedKey::ArrowRight) => Key::Right,
        WKey::Named(NamedKey::ArrowUp) => Key::Up,
        WKey::Named(NamedKey::ArrowDown) => Key::Down,
        WKey::Named(NamedKey::Home) => Key::Home,
        WKey::Named(NamedKey::End) => Key::End,
        WKey::Character(s) => Key::Char(s.chars().next()?),
        _ => return None,
    })
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("paper-rs")
            .with_inner_size(winit::dpi::LogicalSize::new(1440.0, 900.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(720.0, 480.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        window.set_ime_allowed(true);
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("no compatible GPU adapter");
        log::info!("adapter: {:?}", adapter.get_info());
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("paper-rs"),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .expect("create device");
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: Default::default(),
        };
        surface.configure(&device, &config);
        let painter = Painter::new(&device, format);
        let egui = egui_winit::State::new(
            self.ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(device.limits().max_texture_dimension_2d as usize),
        );
        window.request_redraw();
        self.gfx = Some(Gfx {
            window,
            surface,
            device,
            queue,
            config,
            painter,
            egui,
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(g) = self.gfx.as_mut() else { return };
        let resp = g.egui.on_window_event(&g.window, &event);
        let scale = g.window.scale_factor();
        let mut redraw = resp.repaint;
        let now = self.now();
        match event {
            WindowEvent::CloseRequested => {
                if self.confirm_discard() {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(s) => {
                let g = self.gfx.as_mut().unwrap();
                g.config.width = s.width.max(1);
                g.config.height = s.height.max(1);
                g.surface.configure(&g.device, &g.config);
                redraw = true;
            }
            WindowEvent::Occluded(false) => redraw = true,
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::ModifiersChanged(m) => {
                let s = m.state();
                self.ed.handle(InputEvent::Modifiers(Modifiers {
                    shift: s.shift_key(),
                    ctrl: s.control_key(),
                    alt: s.alt_key(),
                    logo: s.super_key(),
                }));
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = v2(position.x / scale, position.y / scale);
                if self.captured || self.over_canvas(p) {
                    self.ed.handle(InputEvent::PointerMove {
                        pos: self.to_canvas(p),
                        pressure: None,
                        time_ms: now,
                    });
                    redraw = true;
                } else if self.ed.hover.take().is_some() {
                    redraw = true;
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let b = match button {
                    MouseButton::Left => Button::Primary,
                    MouseButton::Middle => Button::Middle,
                    MouseButton::Right => Button::Secondary,
                    _ => return,
                };
                let p = self
                    .ctx
                    .input(|i| i.pointer.latest_pos())
                    .map(|p| v2(p.x as f64, p.y as f64));
                let Some(p) = p else { return };
                match state {
                    ElementState::Pressed if self.over_canvas(p) => {
                        // Clicking the canvas takes keyboard focus away from text fields.
                        self.ctx.memory_mut(|m| {
                            if let Some(f) = m.focused() {
                                m.surrender_focus(f)
                            }
                        });
                        self.captured = true;
                        self.ed.handle(InputEvent::PointerDown {
                            pos: self.to_canvas(p),
                            button: b,
                            pressure: None,
                            time_ms: now,
                        });
                    }
                    ElementState::Released if self.captured => {
                        self.captured = false;
                        self.ed.handle(InputEvent::PointerUp {
                            pos: self.to_canvas(p),
                            button: b,
                            time_ms: now,
                        });
                    }
                    _ => {}
                }
                redraw = true;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let p = self
                    .ctx
                    .input(|i| i.pointer.latest_pos())
                    .map(|p| v2(p.x as f64, p.y as f64));
                if let Some(p) = p.filter(|p| self.over_canvas(*p)) {
                    let d = match delta {
                        MouseScrollDelta::LineDelta(x, y) => v2(x as f64 * 40.0, y as f64 * 40.0),
                        MouseScrollDelta::PixelDelta(pp) => v2(pp.x / scale, pp.y / scale),
                    };
                    let d = if self.ed.mods.shift && d.x == 0.0 {
                        v2(d.y, 0.0)
                    } else {
                        d
                    };
                    self.ed.handle(InputEvent::Scroll {
                        delta: -d,
                        pos: self.to_canvas(p),
                    });
                    redraw = true;
                }
            }
            WindowEvent::PinchGesture { delta, .. } => {
                let p = self
                    .ctx
                    .input(|i| i.pointer.latest_pos())
                    .map(|p| v2(p.x as f64, p.y as f64));
                if let Some(p) = p.filter(|p| self.over_canvas(*p)) {
                    self.ed.handle(InputEvent::Zoom {
                        factor: 1.0 + delta,
                        pos: self.to_canvas(p),
                    });
                    redraw = true;
                }
            }
            WindowEvent::Touch(t) => {
                // Pens and touch screens: forward pressure for ink.
                let p = v2(t.location.x / scale, t.location.y / scale);
                let pressure = t.force.map(|f| f.normalized());
                let pos = self.to_canvas(p);
                match t.phase {
                    TouchPhase::Started if self.over_canvas(p) => {
                        self.captured = true;
                        self.ed.handle(InputEvent::PointerDown {
                            pos,
                            button: Button::Primary,
                            pressure,
                            time_ms: now,
                        });
                    }
                    TouchPhase::Moved if self.captured => self.ed.handle(InputEvent::PointerMove {
                        pos,
                        pressure,
                        time_ms: now,
                    }),
                    TouchPhase::Ended | TouchPhase::Cancelled if self.captured => {
                        self.captured = false;
                        self.ed.handle(InputEvent::PointerUp {
                            pos,
                            button: Button::Primary,
                            time_ms: now,
                        });
                    }
                    _ => {}
                }
                redraw = true;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if self.ctx.egui_wants_keyboard_input() {
                    self.gfx.as_ref().unwrap().window.request_redraw();
                    return;
                }
                let key = map_key(&event.logical_key).or_else(|| match &event.physical_key {
                    // Shortcuts should work regardless of the active keyboard layout's shift state.
                    winit::keyboard::PhysicalKey::Code(c) => physical_char(*c).map(Key::Char),
                    _ => None,
                });
                if let Some(key) = key {
                    match event.state {
                        ElementState::Pressed => {
                            self.ed.handle(InputEvent::KeyDown {
                                key,
                                repeat: event.repeat,
                            });
                            if let Some(t) = &event.text
                                && self.ed.text_edit.is_some()
                                && !self.ed.mods.command()
                            {
                                self.ed.handle(InputEvent::Text(t.to_string()));
                            }
                        }
                        ElementState::Released => self.ed.handle(InputEvent::KeyUp { key }),
                    }
                }
                redraw = true;
            }
            WindowEvent::Ime(winit::event::Ime::Commit(s)) => {
                if !self.ctx.egui_wants_keyboard_input() {
                    self.ed.handle(InputEvent::Text(s));
                }
                redraw = true;
            }
            WindowEvent::DroppedFile(p) => {
                if p.extension().is_some_and(|e| e == file::EXTENSION) && self.confirm_discard() {
                    match file::load(&p) {
                        Ok((doc, cam)) => self.ed.load(doc, cam, Some(p)),
                        Err(e) => self.error(format!("Could not open {}: {e}", p.display())),
                    }
                }
                redraw = true;
            }
            _ => {}
        }
        if redraw && let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }
}

fn physical_char(c: winit::keyboard::KeyCode) -> Option<char> {
    use winit::keyboard::KeyCode::*;
    Some(match c {
        BracketLeft => '[',
        BracketRight => ']',
        Equal => '=',
        Minus => '-',
        Digit0 => '0',
        Digit1 => '1',
        Digit2 => '2',
        _ => return None,
    })
}

pub fn run(ed: Editor) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| format!("cannot open a window: {e}"))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(ed);
    event_loop.run_app(&mut app).map_err(|e| e.to_string())
}
