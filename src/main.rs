use paper_rs::editor::Editor;
use paper_rs::io::{file, png, svg};
use paper_rs::model::Color;
use paper_rs::render::{frame, gpu::Headless};
use paper_rs::ui::UiState;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
paper-rs — infinite canvas for planning, working and sketching

USAGE:
  paper-rs [FILE.paper-rs]                 open the app (optionally with a document)
  paper-rs --demo                          open the app with a sample board
  paper-rs --screenshot OUT.png [FILE|--demo] [--size WxH] [--scale S]
                                           render the full UI headlessly to a PNG
  paper-rs --export-png OUT.png FILE [--scale S] [--transparent]
  paper-rs --export-svg OUT.svg FILE
  paper-rs --help
";

fn load_editor(path: Option<&str>, demo: bool) -> Result<Editor, String> {
    if demo {
        return Ok(paper_rs::demo::showcase());
    }
    let Some(p) = path else {
        return Ok(Editor::default());
    };
    let p = PathBuf::from(p);
    let (doc, cam) = file::load(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut ed = Editor::default();
    ed.load(doc, cam, Some(p));
    Ok(ed)
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let positional: Vec<&String> = {
        let mut out = Vec::new();
        let mut skip = false;
        for a in &args {
            if skip {
                skip = false;
                continue;
            }
            if matches!(
                a.as_str(),
                "--screenshot" | "--export-png" | "--export-svg" | "--size" | "--scale"
            ) {
                skip = true;
                continue;
            }
            if !a.starts_with("--") {
                out.push(a);
            }
        }
        out
    };
    let input = positional.first().map(|s| s.as_str());
    let scale: f64 = value("--scale")
        .map(|s| s.parse().map_err(|_| "bad --scale"))
        .transpose()?
        .unwrap_or(2.0);

    if flag("--help") || flag("-h") {
        print!("{USAGE}");
        return Ok(());
    }
    if let Some(out) = value("--screenshot") {
        let mut ed = load_editor(input, flag("--demo"))?;
        let (w, h) = match value("--size") {
            Some(s) => {
                let (w, h) = s.split_once('x').ok_or("--size must be WxH")?;
                (
                    w.parse::<f32>().map_err(|_| "bad width")?,
                    h.parse::<f32>().map_err(|_| "bad height")?,
                )
            }
            None => (1440.0, 900.0),
        };
        let gpu = Headless::new()?;
        let img = frame::capture(
            &gpu,
            &mut ed,
            &mut UiState::default(),
            [w, h],
            scale as f32,
            |ed| ed.zoom_to_fit(),
        );
        img.save(&out).map_err(|e| e.to_string())?;
        println!(
            "wrote {out} ({}x{}) via {}",
            img.width(),
            img.height(),
            gpu.adapter_info.name
        );
        return Ok(());
    }
    if let Some(out) = value("--export-png") {
        let ed = load_editor(input, flag("--demo"))?;
        let bg = (!flag("--transparent")).then_some(Color::WHITE);
        let (w, h) = png::export(out.as_ref(), &ed.doc, None, scale, bg)?;
        println!("wrote {out} ({w}x{h})");
        return Ok(());
    }
    if let Some(out) = value("--export-svg") {
        let ed = load_editor(input, flag("--demo"))?;
        let s = svg::export(&ed.doc, None, None).ok_or("nothing to export")?;
        std::fs::write(&out, s).map_err(|e| e.to_string())?;
        println!("wrote {out}");
        return Ok(());
    }
    let ed = load_editor(input, flag("--demo"))?;
    paper_rs::app::run(ed)
}
