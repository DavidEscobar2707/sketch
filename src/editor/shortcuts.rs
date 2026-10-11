//! Keyboard shortcuts. One table drives dispatch, the in-app help and the README.

use super::Tool;
use super::input::{Key, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Tool(Tool),
    Escape,
    EditText,
    Delete,
    Undo,
    Redo,
    SelectAll,
    Copy,
    Cut,
    Paste,
    Duplicate,
    Forward,
    Backward,
    ToFront,
    ToBack,
    ZoomIn,
    ZoomOut,
    Zoom100,
    ZoomFit,
    ZoomSelection,
    ToggleVisible,
    ToggleLock,
    Nudge(f64, f64),
    WrapInFrame,
    New,
    Open,
    Save,
    SaveAs,
    ExportPng,
    ExportSvg,
    ToggleSnapping,
}

impl Action {
    pub fn repeatable(&self) -> bool {
        matches!(
            self,
            Action::Nudge(..) | Action::Undo | Action::Redo | Action::ZoomIn | Action::ZoomOut
        )
    }
}

/// `(cmd, shift, alt)` requirement for a binding. `cmd` is Cmd on macOS and Ctrl elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Chord {
    cmd: bool,
    shift: bool,
    alt: bool,
}

const fn c(cmd: bool, shift: bool, alt: bool) -> Chord {
    Chord { cmd, shift, alt }
}
const PLAIN: Chord = c(false, false, false);
const SHIFT: Chord = c(false, true, false);
const CMD: Chord = c(true, false, false);
const CMD_SHIFT: Chord = c(true, true, false);
const CMD_ALT: Chord = c(true, false, true);

/// Bindings: (key, chord, action, description shown in help).
fn bindings() -> Vec<(Key, Chord, Action, &'static str)> {
    use Action as A;
    use Key::*;
    vec![
        (Char('v'), PLAIN, A::Tool(Tool::Select), "Select tool"),
        (
            Char('h'),
            PLAIN,
            A::Tool(Tool::Hand),
            "Hand tool (or hold Space)",
        ),
        (Char('f'), PLAIN, A::Tool(Tool::Frame), "Frame tool"),
        (Char('a'), PLAIN, A::Tool(Tool::Frame), "Frame tool"),
        (Char('r'), PLAIN, A::Tool(Tool::Rect), "Rectangle tool"),
        (Char('o'), PLAIN, A::Tool(Tool::Ellipse), "Ellipse tool"),
        (Char('l'), PLAIN, A::Tool(Tool::Line), "Line tool"),
        (Char('t'), PLAIN, A::Tool(Tool::Text), "Text tool"),
        (
            Char('p'),
            PLAIN,
            A::Tool(Tool::Pencil),
            "Pencil (freehand sketch)",
        ),
        (
            Char('b'),
            PLAIN,
            A::Tool(Tool::Pencil),
            "Pencil (freehand sketch)",
        ),
        (Char('e'), PLAIN, A::Tool(Tool::Eraser), "Eraser"),
        (
            Escape,
            PLAIN,
            A::Escape,
            "Cancel / select parent / deselect",
        ),
        (Enter, PLAIN, A::EditText, "Edit text / select children"),
        (Delete, PLAIN, A::Delete, "Delete selection"),
        (Backspace, PLAIN, A::Delete, "Delete selection"),
        (Char('z'), CMD, A::Undo, "Undo"),
        (Char('z'), CMD_SHIFT, A::Redo, "Redo"),
        (Char('y'), CMD, A::Redo, "Redo"),
        (Char('a'), CMD, A::SelectAll, "Select all"),
        (Char('c'), CMD, A::Copy, "Copy"),
        (Char('x'), CMD, A::Cut, "Cut"),
        (Char('v'), CMD, A::Paste, "Paste"),
        (Char('d'), CMD, A::Duplicate, "Duplicate"),
        (Char(']'), CMD, A::Forward, "Bring forward"),
        (Char('['), CMD, A::Backward, "Send backward"),
        (Char(']'), CMD_SHIFT, A::ToFront, "Bring to front"),
        (Char('['), CMD_SHIFT, A::ToBack, "Send to back"),
        (Char('}'), CMD_SHIFT, A::ToFront, "Bring to front"),
        (Char('{'), CMD_SHIFT, A::ToBack, "Send to back"),
        (Char('='), CMD, A::ZoomIn, "Zoom in"),
        (Char('+'), CMD, A::ZoomIn, "Zoom in"),
        (Char('+'), CMD_SHIFT, A::ZoomIn, "Zoom in"),
        (Char('-'), CMD, A::ZoomOut, "Zoom out"),
        (Char('0'), CMD, A::Zoom100, "Zoom to 100%"),
        (Char('1'), SHIFT, A::ZoomFit, "Zoom to fit"),
        (Char('!'), SHIFT, A::ZoomFit, "Zoom to fit"),
        (Char('2'), SHIFT, A::ZoomSelection, "Zoom to selection"),
        (Char('@'), SHIFT, A::ZoomSelection, "Zoom to selection"),
        (Char('h'), CMD_SHIFT, A::ToggleVisible, "Show / hide"),
        (Char('l'), CMD_SHIFT, A::ToggleLock, "Lock / unlock"),
        (
            Char('g'),
            CMD_ALT,
            A::WrapInFrame,
            "Wrap selection in frame",
        ),
        (Left, PLAIN, A::Nudge(-1.0, 0.0), "Nudge left"),
        (Right, PLAIN, A::Nudge(1.0, 0.0), "Nudge right"),
        (Up, PLAIN, A::Nudge(0.0, -1.0), "Nudge up"),
        (Down, PLAIN, A::Nudge(0.0, 1.0), "Nudge down"),
        (Left, SHIFT, A::Nudge(-10.0, 0.0), "Nudge left 10"),
        (Right, SHIFT, A::Nudge(10.0, 0.0), "Nudge right 10"),
        (Up, SHIFT, A::Nudge(0.0, -10.0), "Nudge up 10"),
        (Down, SHIFT, A::Nudge(0.0, 10.0), "Nudge down 10"),
        (Char('n'), CMD, A::New, "New file"),
        (Char('o'), CMD, A::Open, "Open…"),
        (Char('s'), CMD, A::Save, "Save"),
        (Char('s'), CMD_SHIFT, A::SaveAs, "Save as…"),
        (Char('e'), CMD_SHIFT, A::ExportPng, "Export PNG"),
        (Char('e'), CMD_ALT, A::ExportSvg, "Export SVG"),
        (Char('\''), CMD, A::ToggleSnapping, "Toggle snapping"),
    ]
}

pub fn lookup(key: &Key, m: Modifiers) -> Option<Action> {
    let key = match key {
        Key::Char(ch) => Key::Char(ch.to_ascii_lowercase()),
        k => k.clone(),
    };
    let chord = c(m.command(), m.shift, m.alt);
    bindings()
        .into_iter()
        .find(|(k, ch, _, _)| *k == key && *ch == chord)
        .map(|(_, _, a, _)| a)
}

/// Help description of the first binding for an action.
pub fn lookup_desc(action: Action) -> Option<&'static str> {
    bindings()
        .into_iter()
        .find(|(_, _, a, _)| *a == action)
        .map(|(_, _, _, d)| d)
}

/// Human-readable list for help and docs, deduplicated by description.
pub fn help_table() -> Vec<(String, &'static str)> {
    let cmd = if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl+"
    };
    let mut out: Vec<(String, &'static str)> = Vec::new();
    for (k, ch, _, desc) in bindings() {
        let key = match &k {
            Key::Char(c) => c.to_ascii_uppercase().to_string(),
            Key::Left => "←".into(),
            Key::Right => "→".into(),
            Key::Up => "↑".into(),
            Key::Down => "↓".into(),
            other => format!("{other:?}"),
        };
        let mut s = String::new();
        if ch.cmd {
            s += cmd;
        }
        if ch.alt {
            s += if cfg!(target_os = "macos") {
                "⌥"
            } else {
                "Alt+"
            };
        }
        if ch.shift {
            s += if cfg!(target_os = "macos") {
                "⇧"
            } else {
                "Shift+"
            };
        }
        s += &key;
        if let Some(e) = out.iter_mut().find(|(_, d)| *d == desc) {
            if !e.0.contains(&s)
                && e.0.matches(" / ").count() < 1
                && !matches!(k, Key::Char('!' | '@' | '{' | '}' | '+'))
            {
                e.0 = format!("{} / {}", e.0, s);
            }
        } else {
            out.push((s, desc));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_respects_modifiers() {
        assert_eq!(
            lookup(&Key::Char('z'), Modifiers::cmd()),
            Some(Action::Undo)
        );
        assert_eq!(
            lookup(&Key::Char('Z'), Modifiers::cmd_shift()),
            Some(Action::Redo)
        );
        assert_eq!(
            lookup(&Key::Char('r'), Modifiers::NONE),
            Some(Action::Tool(Tool::Rect))
        );
        assert_eq!(lookup(&Key::Char('r'), Modifiers::SHIFT), None);
        assert_eq!(
            lookup(&Key::Char('a'), Modifiers::cmd()),
            Some(Action::SelectAll)
        );
    }

    #[test]
    fn no_conflicting_bindings() {
        let b = bindings();
        for (i, x) in b.iter().enumerate() {
            for y in &b[i + 1..] {
                assert!(!(x.0 == y.0 && x.1 == y.1), "conflict: {:?} {:?}", x.0, x.1);
            }
        }
        assert!(help_table().len() > 30);
    }
}
