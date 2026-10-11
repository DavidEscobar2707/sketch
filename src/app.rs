//! The windowed application: winit event loop, wgpu surface, egui integration, file dialogs.

use crate::editor::{AppRequest, Button, Editor, InputEvent, Key, Modifiers};
use crate::geom::{V2, v2};
use crate::io::{file, png, svg};
use crate::model::Color;
use crate::project::Store;
use crate::render::frame::{self, Painter};
use crate::render::gpu::Headless;
use crate::session::Session;
use crate::theme;
use crate::ui::home::HomeAction;
use crate::ui::{self, UiState};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
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
    adapter_info: wgpu::AdapterInfo,
}

pub struct App {
    gfx: Option<Gfx>,
    pub ed: Editor,
    session: Session,
    st: UiState,
    ctx: egui::Context,
    start: Instant,
    canvas: egui::Rect,
    captured: bool,
    title: String,
    /// When egui asked for a delayed repaint (tooltips, caret blink, animations).
    next_repaint: Option<Instant>,
    #[cfg(target_os = "macos")]
    menu: Option<crate::macos::NativeMenu>,
}

/// Events delivered to the winit loop from other sources.
#[derive(Debug)]
pub enum UserEvent {
    #[cfg(target_os = "macos")]
    Menu(muda::MenuId),
}

/// What to show at startup.
pub enum Start {
    /// The projects home screen.
    Home,
    /// Open this project folder.
    Project(PathBuf),
    /// A scratch editor that is not backed by a project (demo, tests).
    Scratch(Box<Editor>),
}

impl App {
    pub fn new(start: Start, store: Store) -> App {
        let ctx = egui::Context::default();
        theme::set_accent(theme::system_accent());
        ui::setup_style(&ctx);
        let mut app = App {
            gfx: None,
            ed: Editor::default(),
            session: Session::new(store),
            st: UiState::default(),
            ctx,
            start: Instant::now(),
            canvas: egui::Rect::NOTHING,
            captured: false,
            title: String::new(),
            next_repaint: None,
            #[cfg(target_os = "macos")]
            menu: None,
        };
        match start {
            Start::Home => app.show_home(),
            Start::Project(dir) => {
                if let Err(e) = app.session.open(&mut app.ed, &dir) {
                    app.error(format!("Could not open {}: {e}", dir.display()));
                    app.show_home();
                } else {
                    app.st.project_name = app.session.project_name();
                }
            }
            Start::Scratch(ed) => app.ed = *ed,
        }
        app
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

    /// Before leaving a scratch (project-less) document. Projects autosave, so they never ask.
    fn confirm_discard(&mut self) -> bool {
        if self.session.project.is_some() || self.st.home.is_some() || !self.ed.is_dirty() {
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

    /// Save a scratch document to a legacy file (only used when no project is open).
    fn save(&mut self, _save_as: bool) -> bool {
        if self.session.project.is_some() {
            return self.save_project(true);
        }
        let Some(p) = rfd::FileDialog::new()
            .add_filter("paper-rs document", &[file::EXTENSION])
            .set_file_name(format!("{}.{}", self.ed.doc.name, file::EXTENSION))
            .save_file()
        else {
            return false;
        };
        let path = with_ext(p, file::EXTENSION);
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

    /// A GPU handle for offscreen work (thumbnails, PNG export) on the window's device.
    fn offscreen(&self) -> Option<Headless> {
        let g = self.gfx.as_ref()?;
        Some(Headless {
            device: g.device.clone(),
            queue: g.queue.clone(),
            adapter_info: g.adapter_info.clone(),
        })
    }

    /// Save the open project; `full` also writes the camera and refreshes the thumbnail.
    fn save_project(&mut self, full: bool) -> bool {
        self.st.save_state = String::new();
        let r = if full {
            self.session.save_all(&mut self.ed)
        } else {
            self.session.save(&mut self.ed).map(|_| ())
        };
        if let Err(e) = r {
            self.st.save_state = "Not saved".into();
            self.session.defer();
            self.error(format!("Could not save project: {e}"));
            return false;
        }
        self.st.save_state = "Saved".into();
        if self.session.thumbnail_due(Instant::now(), full)
            && let Some(gpu) = self.offscreen()
            && let Err(e) = self.session.write_thumbnail(&gpu, &self.ed)
        {
            log::warn!("thumbnail: {e}");
        }
        true
    }

    /// Autosave when due. Called after every frame and on timer wake-ups.
    fn tick_autosave(&mut self) {
        if self.session.autosave_due(&self.ed, Instant::now()) {
            self.save_project(false);
        }
    }

    fn show_home(&mut self) {
        let mut hv = self.st.home.take().unwrap_or_default();
        hv.refresh(&self.session.store, crate::project::now_ms());
        self.st.home = Some(hv);
        self.st.project_name.clear();
        self.st.save_state.clear();
    }

    fn open_project(&mut self, dir: &std::path::Path) {
        if self.session.project.is_some() {
            self.save_project(true);
        }
        match self.session.open(&mut self.ed, dir) {
            Ok(()) => {
                self.st.home = None;
                self.st.project_name = self.session.project_name();
                self.st.save_state.clear();
                self.ed.status.clear();
            }
            Err(e) => self.error(format!("Could not open {}: {e}", dir.display())),
        }
    }

    fn new_project(&mut self) {
        if self.session.project.is_some() {
            self.save_project(true);
        }
        match self.session.create(&mut self.ed, "Untitled") {
            Ok(()) => {
                self.st.home = None;
                self.st.project_name = self.session.project_name();
                self.st.save_state.clear();
            }
            Err(e) => self.error(format!("Could not create a project: {e}")),
        }
    }

    fn go_home(&mut self) {
        if self.session.project.is_some() {
            self.save_project(true);
            if let Err(e) = self.session.close(&mut self.ed) {
                self.error(format!("Could not save project: {e}"));
            }
        } else if !self.confirm_discard() {
            return;
        }
        self.show_home();
    }

    /// Import a file into a new project and open it.
    fn import_dialog(&mut self) {
        let Some(p) = rfd::FileDialog::new()
            .add_filter("Supported files", &[file::EXTENSION])
            .add_filter("paper-rs 0.1 document", &[file::EXTENSION])
            .pick_file()
        else {
            return;
        };
        self.import_file(&p);
    }

    fn import_file(&mut self, p: &std::path::Path) {
        match crate::io::import::import_as_pages(p) {
            Ok((name, pages)) => match self.session.store.create_with_pages(&name, pages) {
                Ok(proj) => {
                    let dir = proj.dir.clone();
                    self.open_project(&dir);
                    self.ed.status = format!("Imported {}", p.display());
                }
                Err(e) => self.error(format!("Could not create a project: {e}")),
            },
            Err(e) => self.error(format!("Could not import {}: {e}", p.display())),
        }
    }

    fn handle_home_actions(&mut self) {
        let Some(hv) = &mut self.st.home else { return };
        let actions = std::mem::take(&mut hv.actions);
        if actions.is_empty() {
            return;
        }
        let store = self.session.store.clone();
        for a in actions {
            let r: Result<(), String> = match a {
                HomeAction::New => {
                    self.new_project();
                    Ok(())
                }
                HomeAction::Import => {
                    self.import_dialog();
                    Ok(())
                }
                HomeAction::Open(dir) => {
                    self.open_project(&dir);
                    Ok(())
                }
                HomeAction::Rename(dir, name) => store
                    .rename(&dir, &name)
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                HomeAction::Duplicate(dir) => {
                    store.duplicate(&dir).map(|_| ()).map_err(|e| e.to_string())
                }
                HomeAction::Trash(dir) => store.trash(&dir).map(|_| ()).map_err(|e| e.to_string()),
                HomeAction::Restore(dir) => {
                    store.restore(&dir).map(|_| ()).map_err(|e| e.to_string())
                }
                HomeAction::DeleteForever(dir) => {
                    store.delete_forever(&dir).map_err(|e| e.to_string())
                }
                HomeAction::EmptyTrash => store.empty_trash().map_err(|e| e.to_string()),
            };
            if let Err(e) = r {
                self.error(e);
            }
        }
        if self.st.home.is_some() {
            self.show_home();
        }
    }

    fn export_roots(&self) -> Option<Vec<crate::model::NodeId>> {
        let sel = self.ed.top_selection();
        (!sel.is_empty()).then_some(sel)
    }

    fn handle_requests(&mut self) {
        for req in std::mem::take(&mut self.ed.requests) {
            match req {
                AppRequest::Home => self.go_home(),
                AppRequest::New => self.new_project(),
                AppRequest::Open => self.import_dialog(),
                AppRequest::Save => {
                    self.save(false);
                }
                AppRequest::SaveAs => {
                    if let Some(dir) = self.session.project.as_ref().map(|p| p.dir.clone()) {
                        self.save_project(true);
                        match self.session.store.duplicate(&dir) {
                            Ok(copy) => self.open_project(&copy),
                            Err(e) => self.error(format!("Could not duplicate: {e}")),
                        }
                    } else {
                        self.save(true);
                    }
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
        let repaint_delay = l
            .output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map_or(Duration::MAX, |v| v.repaint_delay);
        let platform = std::mem::take(&mut l.output.platform_output);
        g.egui.handle_platform_output(&g.window, platform);

        let frame = match g.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Occluded => {
                // macOS reports Occluded until the window is on screen (and while it is
                // hidden). Keep egui's texture state in sync, but don't spin:
                // WindowEvent::Occluded(false) triggers a redraw.
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
        // AppKit re-lays out the titlebar (first show, fullscreen, resize) and puts the
        // traffic lights back; re-inset them (a no-op when already in place).
        #[cfg(target_os = "macos")]
        {
            let (x, y) = theme::TRAFFIC_LIGHTS;
            crate::macos::inset_traffic_lights(&g.window, x, y);
        }
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

        let title = if self.st.home.is_some() {
            "paper-rs".to_string()
        } else if self.session.project.is_some() {
            format!(
                "{} — {} — paper-rs",
                self.st.project_name,
                self.ed.page_name()
            )
        } else {
            format!(
                "{}{} — paper-rs",
                self.ed.doc.name,
                if self.ed.is_dirty() { " •" } else { "" }
            )
        };
        if title != self.title {
            g.window.set_title(&title);
            self.title = title;
        }
        let pending = !self.ed.requests.is_empty()
            || self.st.home.as_ref().is_some_and(|h| !h.actions.is_empty());
        self.handle_requests();
        self.handle_home_actions();
        self.tick_autosave();
        self.sync_menu();
        // Repaint only when egui wants it: immediately (an animation is running or state just
        // changed), after a delay (tooltip, caret blink), or never (idle; input wakes us).
        let Some(g) = &self.gfx else { return };
        if log::log_enabled!(log::Level::Trace) {
            log::trace!(
                "frame: repaint in {repaint_delay:?}, causes {:?}",
                self.ctx.repaint_causes()
            );
        }
        if repaint_delay.is_zero() || pending || !self.ed.requests.is_empty() {
            g.window.request_redraw();
            self.next_repaint = None;
        } else if repaint_delay < Duration::from_secs(3600) {
            self.next_repaint = Some(Instant::now() + repaint_delay);
        } else {
            self.next_repaint = None;
        }
    }

    /// Keep the native menu's check marks in step with the editor.
    fn sync_menu(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(m) = &self.menu {
            let set = |item: &muda::CheckMenuItem, v: bool| {
                if item.is_checked() != v {
                    item.set_checked(v);
                }
            };
            set(&m.snapping, self.ed.snapping);
            set(&m.grid, !self.st.show_grid_off);
            set(&m.help, self.st.show_help);
        }
    }

    #[cfg(target_os = "macos")]
    fn menu_command(&mut self, event_loop: &ActiveEventLoop, cmd: crate::macos::MenuCmd) {
        use crate::editor::shortcuts::Action as A;
        use crate::macos::MenuCmd;
        match cmd {
            MenuCmd::Quit => {
                if self.confirm_discard() {
                    event_loop.exit();
                }
            }
            MenuCmd::ToggleGrid => self.st.show_grid_off = !self.st.show_grid_off,
            MenuCmd::ToggleHelp => self.st.show_help = !self.st.show_help,
            // Menu key equivalents never reach egui, so while a text field is focused, hand
            // the standard editing commands to it instead of the canvas.
            MenuCmd::Run(a @ (A::Undo | A::Redo | A::Cut | A::Copy | A::Paste | A::SelectAll))
                if self.ctx.egui_wants_keyboard_input() =>
            {
                let Some(g) = self.gfx.as_mut() else { return };
                let key = |key, shift: bool| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: if shift {
                        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
                    } else {
                        egui::Modifiers::COMMAND
                    },
                };
                let ev = match a {
                    A::Cut => egui::Event::Cut,
                    A::Copy => egui::Event::Copy,
                    A::Paste => match g.egui.clipboard_text() {
                        Some(t) => egui::Event::Paste(t),
                        None => return,
                    },
                    A::Undo => key(egui::Key::Z, false),
                    A::Redo => key(egui::Key::Z, true),
                    _ => key(egui::Key::A, false),
                };
                g.egui.egui_input_mut().events.push(ev);
            }
            MenuCmd::Run(a) => self.ed.run(a),
        }
        if let Some(g) = &self.gfx {
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

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("paper-rs")
            .with_inner_size(winit::dpi::LogicalSize::new(1440.0, 900.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(720.0, 480.0));
        // PAPER_RS_OPAQUE=1 turns off vibrancy (and window transparency) on macOS.
        #[cfg(target_os = "macos")]
        let translucent = std::env::var_os("PAPER_RS_OPAQUE").is_none();
        #[cfg(target_os = "macos")]
        let attrs = {
            use winit::platform::macos::WindowAttributesExtMacOS;
            // Unified, title-less toolbar look: content runs under a transparent titlebar.
            attrs
                .with_titlebar_transparent(true)
                .with_fullsize_content_view(true)
                .with_title_hidden(true)
                .with_transparent(translucent)
        };
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        window.set_ime_allowed(true);
        #[cfg(target_os = "macos")]
        {
            self.st.translucent_sidebar = translucent && crate::macos::apply_vibrancy(&window);
            self.st.native_chrome = true;
            let (x, y) = theme::TRAFFIC_LIGHTS;
            crate::macos::inset_traffic_lights(&window, x, y);
            self.menu = Some(crate::macos::install_menu());
        }
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
        let adapter_info = adapter.get_info();
        log::info!("adapter: {adapter_info:?}");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("paper-rs"),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .expect("create device");
        let caps = surface.get_capabilities(&adapter);
        log::info!("surface alpha modes: {:?}", caps.alpha_modes);
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
            alpha_mode: pick_alpha_mode(&caps.alpha_modes, self.st.translucent_sidebar),
            view_formats: vec![],
            color_space: Default::default(),
        };
        surface.configure(&device, &config);
        let mut painter = Painter::new(&device, format);
        painter.transparent = self.st.translucent_sidebar;
        let egui = egui_winit::State::new(
            self.ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            // Start in the system appearance; ThemeChanged keeps it in sync afterwards.
            window.theme(),
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
            adapter_info,
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(g) = self.gfx.as_mut() else { return };
        let resp = g.egui.on_window_event(&g.window, &event);
        if !matches!(event, WindowEvent::RedrawRequested) {
            log::trace!("event {event:?} -> egui repaint {}", resp.repaint);
        }
        let scale = g.window.scale_factor();
        let mut redraw = resp.repaint;
        let now = self.now();
        match event {
            WindowEvent::CloseRequested => {
                if self.session.project.is_some() {
                    self.save_project(true);
                }
                if self.confirm_discard() {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(s) => {
                let g = self.gfx.as_mut().unwrap();
                g.config.width = s.width.max(1);
                g.config.height = s.height.max(1);
                g.surface.configure(&g.device, &g.config);
                #[cfg(target_os = "macos")]
                {
                    let (x, y) = theme::TRAFFIC_LIGHTS;
                    crate::macos::inset_traffic_lights(&g.window, x, y);
                }
                redraw = true;
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                // Moved between Retina and non-Retina displays. egui-winit has already picked
                // up the new pixels-per-point; resize the surface to the new physical size
                // (winit follows with a Resized, but don't rely on it).
                let g = self.gfx.as_mut().unwrap();
                let s = g.window.inner_size();
                g.config.width = s.width.max(1);
                g.config.height = s.height.max(1);
                g.surface.configure(&g.device, &g.config);
                redraw = true;
            }
            WindowEvent::ThemeChanged(_) => {
                // Light/dark switch: egui-winit swaps the style; refresh the accent too.
                theme::set_accent(theme::system_accent());
                ui::setup_style(&self.ctx);
                redraw = true;
            }
            WindowEvent::Occluded(false) => redraw = true,
            WindowEvent::RedrawRequested => {
                // egui-winit reports `repaint: true` for RedrawRequested itself; honouring it
                // re-requested a redraw every frame (a 100% CPU busy loop). Whether another
                // frame is needed is decided inside redraw() from egui's repaint delay.
                self.redraw();
                return;
            }
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
                    // `delta` is the change in magnification; exp() keeps pinch in and out
                    // symmetric, like MKMapView / Preview.
                    self.ed.handle(InputEvent::Zoom {
                        factor: delta.exp(),
                        pos: self.to_canvas(p),
                    });
                    redraw = true;
                }
            }
            WindowEvent::DoubleTapGesture { .. } => {
                // Trackpad smart zoom (two-finger double tap): toggle fit / 100%.
                let a = if (self.ed.camera.zoom - 1.0).abs() < 1e-3 {
                    crate::editor::shortcuts::Action::ZoomFit
                } else {
                    crate::editor::shortcuts::Action::Zoom100
                };
                self.ed.run(a);
                redraw = true;
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

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            #[cfg(target_os = "macos")]
            UserEvent::Menu(id) => {
                if let Some(cmd) = self.menu.as_ref().and_then(|m| m.command(&id)) {
                    self.menu_command(_event_loop, cmd);
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Autosave runs off a timer so it also fires while the window sits idle.
        if self
            .session
            .next_deadline()
            .is_some_and(|d| Instant::now() >= d)
        {
            self.tick_autosave();
            if let Some(g) = &self.gfx {
                g.window.request_redraw();
            }
        }
        let wake = match (self.next_repaint, self.session.next_deadline()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        match wake {
            Some(t) if Instant::now() >= t => {
                if self.next_repaint.is_some_and(|r| r <= t) {
                    self.next_repaint = None;
                }
                if let Some(g) = &self.gfx {
                    g.window.request_redraw();
                }
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            Some(t) => event_loop.set_control_flow(ControlFlow::WaitUntil(t)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}

/// A non-opaque mode when the window shows vibrancy through transparent pixels.
fn pick_alpha_mode(
    modes: &[wgpu::CompositeAlphaMode],
    translucent: bool,
) -> wgpu::CompositeAlphaMode {
    use wgpu::CompositeAlphaMode as A;
    if translucent {
        for m in [A::PreMultiplied, A::PostMultiplied, A::Inherit] {
            if modes.contains(&m) {
                return m;
            }
        }
        log::warn!("no transparent surface alpha mode in {modes:?}");
    }
    if modes.contains(&A::Opaque) {
        A::Opaque
    } else {
        modes[0]
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

pub fn run(start: Start) -> Result<(), String> {
    let store = Store::open(Store::default_root())
        .map_err(|e| format!("cannot create the projects folder: {e}"))?;
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .map_err(|e| format!("cannot open a window: {e}"))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    #[cfg(target_os = "macos")]
    {
        let proxy = event_loop.create_proxy();
        muda::MenuEvent::set_event_handler(Some(move |e: muda::MenuEvent| {
            let _ = proxy.send_event(UserEvent::Menu(e.id));
        }));
    }
    let mut app = App::new(start, store);
    event_loop.run_app(&mut app).map_err(|e| e.to_string())
}
