#![forbid(unsafe_code)]

use watchtower_rs::meta;
use watchtower_rs::notifications::preview::{self, LogLevel, State};

#[cfg(target_arch = "wasm32")]
use js_sys::{Array, Object, Reflect};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use web_sys::console;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("watchtower/tplprev v{}\n", meta::version());

    let args: Vec<String> = std::env::args().collect();

    // Parse flags manually (simple approach)
    let mut states = "cccuuueeekkktttfff".to_string();
    let mut entries = "ewwiiidddd".to_string();
    let mut template_path = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-states" => {
                i += 1;
                if i < args.len() {
                    states = args[i].clone();
                }
            }
            "-entries" => {
                i += 1;
                if i < args.len() {
                    entries = args[i].clone();
                }
            }
            arg if !arg.starts_with('-') && template_path.is_none() => {
                template_path = Some(arg.to_string());
            }
            _ => {}
        }
        i += 1;
    }

    match template_path {
        None => {
            eprintln!("Missing required argument TEMPLATE");
            eprintln!("Usage: tplprev [flags] <template-file>");
            eprintln!("  -states string   Container states (default \"cccuuueeekkktttfff\")");
            eprintln!("  -entries string  Log levels (default \"ewwiiidddd\")");
            std::process::exit(1);
        }
        Some(path) => {
            let input = match std::fs::read_to_string(&path) {
                Ok(content) => content,
                Err(e) => {
                    eprintln!("Failed to read template file {path:?}: {e}");
                    std::process::exit(1);
                }
            };

            match render_preview_from_strings(&input, &states, &entries) {
                Ok(result) => println!("{result}"),
                Err(e) => {
                    eprintln!("Failed to render template: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}

/// Render a notification template from the compact preview selectors.
fn render_preview_from_strings(
    input: &str,
    states: &str,
    entries: &str,
) -> Result<String, String> {
    let states = states_from_string(states);
    let levels = levels_from_string(entries);
    preview::render(input, &states, &levels)
}

fn states_from_string(input: &str) -> Vec<State> {
    let mut states = Vec::with_capacity(input.len());
    for c in input.chars() {
        match c {
            'c' => states.push(State::Scanned),
            'u' => states.push(State::Updated),
            'e' => states.push(State::Failed),
            'k' => states.push(State::Skipped),
            't' => states.push(State::Stale),
            'f' => states.push(State::Fresh),
            _ => continue,
        }
    }
    states
}

fn levels_from_string(input: &str) -> Vec<LogLevel> {
    let mut levels = Vec::with_capacity(input.len());
    for c in input.chars() {
        match c {
            'p' => levels.push(LogLevel::Panic),
            'f' => levels.push(LogLevel::Fatal),
            'e' => levels.push(LogLevel::Error),
            'w' => levels.push(LogLevel::Warn),
            'i' => levels.push(LogLevel::Info),
            'd' => levels.push(LogLevel::Debug),
            't' => levels.push(LogLevel::Trace),
            _ => continue,
        }
    }
    levels
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console::log_1(&format!("watchtower/tplprev v{}", meta::version()).into());
    install_watchtower_global()
}

#[cfg(target_arch = "wasm32")]
fn install_watchtower_global() -> Result<(), JsValue> {
    let watchtower = Object::new();
    let tplprev = Closure::wrap(
        Box::new(move |input: JsValue, states: JsValue, levels: JsValue| {
            let response = js_tplprev(input, states, levels);
            response
        }) as Box<dyn FnMut(JsValue, JsValue, JsValue) -> JsValue>,
    );

    let global = js_sys::global();
    Reflect::set(&global, &JsValue::from_str("WATCHTOWER"), &watchtower)?;
    Reflect::set(&watchtower, &JsValue::from_str("tplprev"), tplprev.as_ref())?;
    tplprev.forget();
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn js_tplprev(input: JsValue, states: JsValue, levels: JsValue) -> JsValue {
    let input = input.as_string().unwrap_or_default();

    let states_vec = if let Some(s) = states.as_string() {
        states_from_string(&s)
    } else {
        let mut v = Vec::new();
        let arr = Array::from(&states);
        for i in 0..arr.length() {
            if let Some(item_str) = arr.get(i).as_string() {
                for c in item_str.chars() {
                    match c {
                        'c' => v.push(State::Scanned),
                        'u' => v.push(State::Updated),
                        'e' => v.push(State::Failed),
                        'k' => v.push(State::Skipped),
                        't' => v.push(State::Stale),
                        'f' => v.push(State::Fresh),
                        _ => continue,
                    }
                }
            }
        }
        v
    };

    let levels_vec = if let Some(s) = levels.as_string() {
        levels_from_string(&s)
    } else {
        let mut v = Vec::new();
        let arr = Array::from(&levels);
        for i in 0..arr.length() {
            if let Some(item_str) = arr.get(i).as_string() {
                for c in item_str.chars() {
                    match c {
                        'p' => v.push(LogLevel::Panic),
                        'f' => v.push(LogLevel::Fatal),
                        'e' => v.push(LogLevel::Error),
                        'w' => v.push(LogLevel::Warn),
                        'i' => v.push(LogLevel::Info),
                        'd' => v.push(LogLevel::Debug),
                        't' => v.push(LogLevel::Trace),
                        _ => continue,
                    }
                }
            }
        }
        v
    };

    match preview::render(&input, &states_vec, &levels_vec) {
        Ok(result) => JsValue::from_str(&result),
        Err(error) => JsValue::from_str(&format!("Error: {error}")),
    }
}
