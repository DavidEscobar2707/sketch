//! macOS integration: vibrancy behind the sidebar, traffic lights inset into the toolbar, and a
//! native menu bar (muda) wired to the editor's actions.

use crate::editor::Tool;
use crate::editor::shortcuts::Action;
use muda::accelerator::{Accelerator, Code, Modifiers as M};
use muda::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use objc2::MainThreadOnly;
use objc2_app_kit::{NSView, NSWindow, NSWindowButton};
use objc2_foundation::NSPoint;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::collections::HashMap;
use winit::window::Window;

fn ns_window(window: &Window) -> Option<objc2::rc::Retained<NSWindow>> {
    let RawWindowHandle::AppKit(h) = window.window_handle().ok()?.as_raw() else {
        return None;
    };
    // SAFETY: winit hands out a valid NSView pointer for the lifetime of the window, and we
    // are on the main thread (all winit window callbacks are).
    let view: &NSView = unsafe { h.ns_view.cast().as_ref() };
    view.window()
}

/// Put an NSVisualEffectView (Sidebar material, behind-window blending) behind the app, so
/// wherever wgpu leaves pixels transparent the desktop shows through, blurred, as in a native
/// sidebar. It is a subview of winit's content view whose layer is pushed behind wgpu's Metal
/// sublayer (zPosition -1); as a plain subview it would cover the Metal layer, and AppKit
/// doesn't honour ordering for views added to the window's frame view.
/// Returns whether it was applied.
pub fn apply_vibrancy(window: &Window) -> bool {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{
        NSAutoresizingMaskOptions, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
        NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode,
    };
    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return false;
    };
    let Some(win) = ns_window(window) else {
        return false;
    };
    let Some(content) = win.contentView() else {
        return false;
    };
    let fx = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), content.bounds());
    fx.setMaterial(NSVisualEffectMaterial::Sidebar);
    fx.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    fx.setState(NSVisualEffectState::FollowsWindowActiveState);
    fx.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    fx.setWantsLayer(true);
    content.addSubview_positioned_relativeTo(&fx, NSWindowOrderingMode::Below, None);
    // SAFETY: `layer` is a CALayer (the view is layer-backed); setZPosition: takes a CGFloat.
    unsafe {
        let layer: *mut AnyObject = msg_send![&*fx, layer];
        if layer.is_null() {
            return false;
        }
        let _: () = msg_send![layer, setZPosition: -1.0f64];
    }
    true
}

/// Move the close/minimise/zoom buttons so they start `x` points from the left and are
/// vertically centred in a titlebar area `bar_h` points tall (our toolbar). AppKit lays the
/// titlebar out again on show, resize and fullscreen changes, so this is re-applied from
/// redraw; it returns early when the buttons are already in place.
pub fn inset_traffic_lights(window: &Window, x: f64, bar_h: f64) {
    let Some(win) = ns_window(window) else { return };
    let (Some(close), Some(mini), Some(zoom)) = (
        win.standardWindowButton(NSWindowButton::CloseButton),
        win.standardWindowButton(NSWindowButton::MiniaturizeButton),
        win.standardWindowButton(NSWindowButton::ZoomButton),
    ) else {
        return;
    };
    // SAFETY: plain view-hierarchy getters on main-thread AppKit objects.
    let Some(container) = (unsafe { close.superview().and_then(|v| v.superview()) }) else {
        return;
    };
    let close_rect = close.frame();
    let btn_y = ((bar_h - close_rect.size.height) / 2.0).round();
    if (close_rect.origin.x - x).abs() < 0.5
        && (close_rect.origin.y - btn_y).abs() < 0.5
        && (container.frame().size.height - bar_h).abs() < 0.5
    {
        return;
    }
    let mut bar = container.frame();
    bar.size.height = bar_h;
    bar.origin.y = win.frame().size.height - bar_h;
    container.setFrame(bar);
    // The buttons' own superview (NSTitlebarView) must grow with the container.
    // SAFETY: as above.
    if let Some(tv) = unsafe { close.superview() } {
        let mut f = tv.frame();
        f.origin.y = 0.0;
        f.size.height = bar_h;
        tv.setFrame(f);
    }
    let gap = mini.frame().origin.x - close_rect.origin.x;
    for (i, b) in [close, mini, zoom].iter().enumerate() {
        b.setFrameOrigin(NSPoint::new(x + gap * i as f64, btn_y));
    }
}

#[derive(Clone, Copy, Debug)]
pub enum MenuCmd {
    Run(Action),
    Quit,
    ToggleGrid,
    ToggleHelp,
}

pub struct NativeMenu {
    _menu: Menu,
    cmds: HashMap<MenuId, MenuCmd>,
    pub snapping: CheckMenuItem,
    pub grid: CheckMenuItem,
    pub help: CheckMenuItem,
}

impl NativeMenu {
    pub fn command(&self, id: &MenuId) -> Option<MenuCmd> {
        self.cmds.get(id).copied()
    }
}

fn acc(m: M, c: Code) -> Option<Accelerator> {
    Some(Accelerator::new(m, c))
}

/// Build the App / File / Edit / View / Window menus and install them on NSApp.
pub fn install_menu() -> NativeMenu {
    let mut cmds = HashMap::new();
    let mut item = |label: &str, a: Option<Accelerator>, cmd: MenuCmd| {
        let it = MenuItem::new(label, true, a);
        cmds.insert(it.id().clone(), cmd);
        it
    };
    let cmd = M::META;
    let cmd_shift = M::META | M::SHIFT;
    let cmd_alt = M::META | M::ALT;
    use Action as A;
    use MenuCmd::Run;

    let quit = item("Quit paper-rs", acc(cmd, Code::KeyQ), MenuCmd::Quit);
    let new = item("New", acc(cmd, Code::KeyN), Run(A::New));
    let open = item("Open…", acc(cmd, Code::KeyO), Run(A::Open));
    let save = item("Save", acc(cmd, Code::KeyS), Run(A::Save));
    let save_as = item("Save As…", acc(cmd_shift, Code::KeyS), Run(A::SaveAs));
    let png = item("Export PNG…", acc(cmd_shift, Code::KeyE), Run(A::ExportPng));
    let svg = item("Export SVG…", acc(cmd_alt, Code::KeyE), Run(A::ExportSvg));
    let undo = item("Undo", acc(cmd, Code::KeyZ), Run(A::Undo));
    let redo = item("Redo", acc(cmd_shift, Code::KeyZ), Run(A::Redo));
    let cut = item("Cut", acc(cmd, Code::KeyX), Run(A::Cut));
    let copy = item("Copy", acc(cmd, Code::KeyC), Run(A::Copy));
    let paste = item("Paste", acc(cmd, Code::KeyV), Run(A::Paste));
    let dup = item("Duplicate", acc(cmd, Code::KeyD), Run(A::Duplicate));
    // Delete stays on the Delete key itself (handled by the editor), so no key equivalent here.
    let del = item("Delete", None, Run(A::Delete));
    let all = item("Select All", acc(cmd, Code::KeyA), Run(A::SelectAll));
    let fwd = item(
        "Bring Forward",
        acc(cmd, Code::BracketRight),
        Run(A::Forward),
    );
    let back = item(
        "Send Backward",
        acc(cmd, Code::BracketLeft),
        Run(A::Backward),
    );
    let front = item(
        "Bring to Front",
        acc(cmd_shift, Code::BracketRight),
        Run(A::ToFront),
    );
    let bottom = item(
        "Send to Back",
        acc(cmd_shift, Code::BracketLeft),
        Run(A::ToBack),
    );
    let wrap = item(
        "Wrap in Frame",
        acc(cmd_alt, Code::KeyG),
        Run(A::WrapInFrame),
    );
    let vis = item(
        "Show/Hide Layer",
        acc(cmd_shift, Code::KeyH),
        Run(A::ToggleVisible),
    );
    let lock = item(
        "Lock/Unlock Layer",
        acc(cmd_shift, Code::KeyL),
        Run(A::ToggleLock),
    );
    let zin = item("Zoom In", acc(cmd, Code::Equal), Run(A::ZoomIn));
    let zout = item("Zoom Out", acc(cmd, Code::Minus), Run(A::ZoomOut));
    let z100 = item("Actual Size", acc(cmd, Code::Digit0), Run(A::Zoom100));
    // Shift+1 / Shift+2 stay editor shortcuts: as menu key equivalents they would swallow
    // "!" and "@" while typing.
    let zfit = item("Zoom to Fit\t⇧1", None, Run(A::ZoomFit));
    let zsel = item("Zoom to Selection\t⇧2", None, Run(A::ZoomSelection));
    let tools: Vec<MenuItem> = Tool::ALL
        .iter()
        .map(|t| {
            item(
                &format!("{}\t{}", t.label(), t.shortcut()),
                None,
                Run(A::Tool(*t)),
            )
        })
        .collect();

    let snapping = CheckMenuItem::new("Snapping", true, true, acc(cmd, Code::Quote));
    cmds.insert(snapping.id().clone(), Run(A::ToggleSnapping));
    let grid = CheckMenuItem::new("Dot Grid", true, true, None);
    cmds.insert(grid.id().clone(), MenuCmd::ToggleGrid);
    let help = CheckMenuItem::new("Keyboard Shortcuts", true, false, acc(cmd, Code::Slash));
    cmds.insert(help.id().clone(), MenuCmd::ToggleHelp);

    let sep = PredefinedMenuItem::separator;
    let app_menu = Submenu::with_items(
        "paper-rs",
        true,
        &[
            &PredefinedMenuItem::about(Some("About paper-rs"), None),
            &sep(),
            &PredefinedMenuItem::services(None),
            &sep(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &sep(),
            &quit,
        ],
    )
    .expect("app menu");
    let file = Submenu::with_items(
        "File",
        true,
        &[
            &new,
            &open,
            &sep(),
            &save,
            &save_as,
            &sep(),
            &png,
            &svg,
            &sep(),
            &PredefinedMenuItem::close_window(None),
        ],
    )
    .expect("file menu");
    let edit = Submenu::with_items(
        "Edit",
        true,
        &[
            &undo,
            &redo,
            &sep(),
            &cut,
            &copy,
            &paste,
            &dup,
            &del,
            &all,
            &sep(),
            &fwd,
            &back,
            &front,
            &bottom,
            &sep(),
            &wrap,
            &vis,
            &lock,
        ],
    )
    .expect("edit menu");
    let tool_refs: Vec<&dyn muda::IsMenuItem> =
        tools.iter().map(|t| t as &dyn muda::IsMenuItem).collect();
    let tools_menu = Submenu::with_items("Tools", true, &tool_refs).expect("tools menu");
    let view = Submenu::with_items(
        "View",
        true,
        &[
            &zin,
            &zout,
            &z100,
            &zfit,
            &zsel,
            &sep(),
            &snapping,
            &grid,
            &sep(),
            &help,
            &sep(),
            &PredefinedMenuItem::fullscreen(None),
        ],
    )
    .expect("view menu");
    let window = Submenu::with_items(
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(None),
            &PredefinedMenuItem::maximize(Some("Zoom")),
            &sep(),
            &PredefinedMenuItem::bring_all_to_front(None),
        ],
    )
    .expect("window menu");
    let menu =
        Menu::with_items(&[&app_menu, &file, &edit, &view, &tools_menu, &window]).expect("menu");
    menu.init_for_nsapp();
    window.set_as_windows_menu_for_nsapp();
    NativeMenu {
        _menu: menu,
        cmds,
        snapping,
        grid,
        help,
    }
}
