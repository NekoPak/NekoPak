//! Scripted UI driver for headless end-to-end tests (feature `automation`).
//! `NEKOPAK_UI_SCRIPT` names a text file with one step per line:
//!
//!   wait 300            pause in milliseconds
//!   size 1280 760       resize the window (logical pixels)
//!   click 100 200       left click      (also: rclick, dclick, move)
//!   type hello          type text into the focused element
//!   key Return          named key (Return, Escape, Tab, Delete, F2, F5, ...)
//!   ctrl a              Ctrl + character (also: shift+key via `shift Down`)
//!   shot name.png       write a PNG next to the script
//!   quit
use crate::app::Ctx;
use slint::{
    platform::{Key, PointerEventButton, WindowEvent},
    ComponentHandle, LogicalPosition, SharedString,
};
use std::{path::PathBuf, time::Duration};

pub fn install(ctx: &Ctx) {
    let Some(script) = std::env::var_os("NEKOPAK_UI_SCRIPT").map(PathBuf::from) else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(&script) else {
        eprintln!("automation: cannot read {}", script.display());
        return;
    };
    let directory = script.parent().map(PathBuf::from).unwrap_or_default();
    let steps: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect();
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        for step in steps {
            if let Some(pause) = run(&ctx, &directory, &step) {
                let (done, wait) = tokio::sync::oneshot::channel::<()>();
                slint::Timer::single_shot(pause, move || {
                    let _ = done.send(());
                });
                let _ = wait.await;
            }
        }
    });
}

fn named_key(name: &str) -> SharedString {
    let key = match name {
        "Return" | "Enter" => Key::Return,
        "Escape" => Key::Escape,
        "Tab" => Key::Tab,
        "Backspace" => Key::Backspace,
        "Delete" => Key::Delete,
        "Up" => Key::UpArrow,
        "Down" => Key::DownArrow,
        "Left" => Key::LeftArrow,
        "Right" => Key::RightArrow,
        "Home" => Key::Home,
        "End" => Key::End,
        "PageDown" => Key::PageDown,
        "PageUp" => Key::PageUp,
        "F2" => Key::F2,
        "F5" => Key::F5,
        "Space" => return " ".into(),
        other => return other.into(),
    };
    key.into()
}

fn press(ctx: &Ctx, text: SharedString, modifier: Option<Key>) {
    let window = ctx.ui.window();
    if let Some(modifier) = modifier {
        window.dispatch_event(WindowEvent::KeyPressed {
            text: modifier.into(),
        });
    }
    window.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    window.dispatch_event(WindowEvent::KeyReleased { text });
    if let Some(modifier) = modifier {
        window.dispatch_event(WindowEvent::KeyReleased {
            text: modifier.into(),
        });
    }
}

fn run(ctx: &Ctx, directory: &std::path::Path, step: &str) -> Option<Duration> {
    let (command, rest) = step.split_once(' ').unwrap_or((step, ""));
    let numbers: Vec<f32> = rest
        .split_whitespace()
        .filter_map(|part| part.parse().ok())
        .collect();
    let window = ctx.ui.window();
    let point = || {
        LogicalPosition::new(
            numbers.first().copied().unwrap_or(0.0),
            numbers.get(1).copied().unwrap_or(0.0),
        )
    };
    let click = |button: PointerEventButton, times: usize| {
        window.dispatch_event(WindowEvent::PointerMoved { position: point() });
        for _ in 0..times {
            window.dispatch_event(WindowEvent::PointerPressed {
                position: point(),
                button,
            });
            window.dispatch_event(WindowEvent::PointerReleased {
                position: point(),
                button,
            });
        }
    };
    match command {
        "wait" => {
            return Some(Duration::from_millis(
                numbers.first().copied().unwrap_or(100.0) as u64,
            ))
        }
        "size" => window.set_size(slint::LogicalSize::new(numbers[0], numbers[1])),
        "move" => window.dispatch_event(WindowEvent::PointerMoved { position: point() }),
        "click" => click(PointerEventButton::Left, 1),
        "dclick" => click(PointerEventButton::Left, 2),
        "rclick" => click(PointerEventButton::Right, 1),
        "type" => {
            for character in rest.chars() {
                press(ctx, character.to_string().into(), None);
            }
        }
        "key" => press(ctx, named_key(rest), None),
        "ctrl" => press(ctx, named_key(rest), Some(Key::Control)),
        "shift" => press(ctx, named_key(rest), Some(Key::Shift)),
        "alt" => press(ctx, named_key(rest), Some(Key::Alt)),
        "shot" => match window.take_snapshot() {
            Ok(image) => {
                let path = directory.join(rest);
                let result = std::fs::File::create(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|file| {
                        let mut encoder = png::Encoder::new(
                            std::io::BufWriter::new(file),
                            image.width(),
                            image.height(),
                        );
                        encoder.set_color(png::ColorType::Rgba);
                        encoder.set_depth(png::BitDepth::Eight);
                        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
                        writer
                            .write_image_data(image.as_bytes())
                            .map_err(|e| e.to_string())
                    });
                if let Err(error) = result {
                    eprintln!("automation: {} not written: {error}", path.display());
                }
            }
            Err(error) => eprintln!("automation: snapshot failed: {error}"),
        },
        "quit" => {
            let _ = slint::quit_event_loop();
        }
        other => eprintln!("automation: unknown step `{other}`"),
    }
    Some(Duration::from_millis(30))
}
