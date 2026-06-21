#![allow(dead_code, unused_imports)]

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use go_template::{Context, Func, FuncError, Template, Value, from_value};
use rand::rngs::StdRng;
use rand::{Rng, RngCore, SeedableRng};
use serde_json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};
use time::{Duration, OffsetDateTime};
use titlecase::titlecase;

use watchtower_rs::meta;

#[cfg(target_arch = "wasm32")]
use js_sys::{Array, Object, Reflect};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use web_sys::console;

const CONTAINER_NAMES: &[&str] = &[
    "cyberscribe",
    "datamatrix",
    "nexasync",
    "quantumquill",
    "aerosphere",
    "virtuos",
    "fusionflow",
    "neuralink",
    "pixelpulse",
    "synthwave",
    "codecraft",
    "zapzone",
    "robologic",
    "dreamstream",
    "infinisync",
    "megamesh",
    "novalink",
    "xenogenius",
    "ecosim",
    "innovault",
    "techtracer",
    "fusionforge",
    "quantumquest",
    "neuronest",
    "codefusion",
    "datadyno",
    "pixelpioneer",
    "vortexvision",
    "cybercraft",
    "synthsphere",
    "infinitescript",
    "roborhythm",
    "dreamengine",
    "aquasync",
    "geniusgrid",
    "megamind",
    "novasync-pro",
    "xenonwave",
    "ecologic",
    "innoscan",
];

const ORGANIZATION_NAMES: &[&str] = &[
    "techwave",
    "codecrafters",
    "innotechlabs",
    "fusionsoft",
    "cyberpulse",
    "quantumscribe",
    "datadynamo",
    "neuralink",
    "pixelpro",
    "synthwizards",
    "virtucorplabs",
    "robologic",
    "dreamstream",
    "novanest",
    "megamind",
    "xenonwave",
    "ecologic",
    "innosync",
    "techgenius",
    "nexasoft",
    "codewave",
    "zapzone",
    "techsphere",
    "aquatech",
    "quantumcraft",
    "neuronest",
    "datafusion",
    "pixelpioneer",
    "synthsphere",
    "infinitescribe",
    "roborhythm",
    "dreamengine",
    "vortexvision",
    "geniusgrid",
    "megamesh",
    "novasync",
    "xenogeniuslabs",
    "ecosim",
    "innovault",
];

const ERROR_MESSAGES: &[&str] = &[
    "Error 404: Resource not found",
    "Critical Error: System meltdown imminent",
    "Error 500: Internal server error",
    "Invalid input: Please check your data",
    "Access denied: Unauthorized access detected",
    "Network connection lost: Please check your connection",
    "Error 403: Forbidden access",
    "Fatal error: System crash imminent",
    "File not found: Check the file path",
    "Invalid credentials: Authentication failed",
    "Error 502: Bad Gateway",
    "Database connection failed: Please try again later",
    "Security breach detected: Take immediate action",
    "Error 400: Bad request",
    "Out of memory: Close unnecessary applications",
    "Invalid configuration: Check your settings",
    "Error 503: Service unavailable",
    "File is read-only: Cannot modify",
    "Data corruption detected: Backup your data",
    "Error 401: Unauthorized",
    "Disk space full: Free up disk space",
    "Connection timeout: Retry your request",
    "Error 504: Gateway timeout",
    "File access denied: Permission denied",
    "Unexpected error: Please contact support",
    "Error 429: Too many requests",
    "Invalid URL: Check the URL format",
    "Database query failed: Try again later",
    "Error 408: Request timeout",
    "File is in use: Close the file and try again",
    "Invalid parameter: Check your input",
    "Error 502: Proxy error",
    "Database connection lost: Reconnect and try again",
    "File size exceeds limit: Reduce the file size",
    "Error 503: Overloaded server",
    "Operation aborted: Try again",
    "Invalid API key: Check your API key",
    "Error 507: Insufficient storage",
    "Database deadlock: Retry your transaction",
    "Error 405: Method not allowed",
    "File format not supported: Choose a different format",
    "Unknown error: Contact system administrator",
];

const SKIPPED_MESSAGES: &[&str] = &[
    "Fear of introducing new bugs",
    "Don't have time for the update process",
    "Current version works fine for my needs",
    "Concerns about compatibility with other software",
    "Limited bandwidth for downloading updates",
    "Worries about losing custom settings or configurations",
    "Lack of trust in the software developer's updates",
    "Dislike changes to the user interface",
    "Avoiding potential subscription fees",
    "Suspicion of hidden data collection in updates",
    "Apprehension about changes in privacy policies",
    "Prefer the older version's features or design",
    "Worry about software becoming more resource-intensive",
    "Avoiding potential changes in licensing terms",
    "Waiting for initial bugs to be resolved in the update",
    "Concerns about update breaking third-party plugins or extensions",
    "Belief that the software is already secure enough",
    "Don't want to relearn how to use the software",
    "Fear of losing access to older file formats",
    "Avoiding the hassle of having to update multiple devices",
];

const LOG_MESSAGES: &[&str] = &[
    "Checking for available updates...",
    "Downloading update package...",
    "Verifying update integrity...",
    "Preparing to install update...",
    "Backing up existing configuration...",
    "Installing update...",
    "Update installation complete.",
    "Applying configuration settings...",
    "Cleaning up temporary files...",
    "Update successful! Software is now up-to-date.",
    "Restarting the application...",
    "Restart complete. Enjoy the latest features!",
    "Update rollback complete. Your software remains at the previous version.",
];

const LOG_ERRORS: &[&str] = &[
    "Unable to check for updates. Please check your internet connection.",
    "Update package download failed. Try again later.",
    "Update verification failed. Please contact support.",
    "Update installation failed. Rolling back to the previous version...",
    "Your configuration settings may have been reset to defaults.",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Scanned,
    Updated,
    Failed,
    Skipped,
    Stale,
    Fresh,
}

impl State {
    fn from_compact_char(value: char) -> Option<Self> {
        match value {
            'c' => Some(Self::Scanned),
            'u' => Some(Self::Updated),
            'e' => Some(Self::Failed),
            'k' => Some(Self::Skipped),
            't' => Some(Self::Stale),
            'f' => Some(Self::Fresh),
            _ => None,
        }
    }

    #[cfg(target_arch = "wasm32")]
    #[allow(dead_code)]
    fn from_explicit_name(value: &str) -> Option<Self> {
        match value {
            "scanned" => Some(Self::Scanned),
            "updated" => Some(Self::Updated),
            "failed" => Some(Self::Failed),
            "skipped" => Some(Self::Skipped),
            "stale" => Some(Self::Stale),
            "fresh" => Some(Self::Fresh),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Scanned => "scanned",
            Self::Updated => "updated",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Stale => "stale",
            Self::Fresh => "fresh",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LogLevel {
    Panic,
    Fatal,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    fn from_compact_char(value: char) -> Option<Self> {
        match value {
            'p' => Some(Self::Panic),
            'f' => Some(Self::Fatal),
            'e' => Some(Self::Error),
            'w' => Some(Self::Warn),
            'i' => Some(Self::Info),
            'd' => Some(Self::Debug),
            't' => Some(Self::Trace),
            _ => None,
        }
    }

    #[cfg(target_arch = "wasm32")]
    #[allow(dead_code)]
    fn from_explicit_name(value: &str) -> Option<Self> {
        match value {
            "panic" => Some(Self::Panic),
            "fatal" => Some(Self::Fatal),
            "error" => Some(Self::Error),
            "warning" => Some(Self::Warn),
            "info" => Some(Self::Info),
            "debug" => Some(Self::Debug),
            "trace" => Some(Self::Trace),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Panic => "panic",
            Self::Fatal => "fatal",
            Self::Error => "error",
            Self::Warn => "warning",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

#[derive(Clone, Debug)]
struct ContainerReportData {
    id: String,
    name: String,
    current_image_id: String,
    latest_image_id: String,
    image_name: String,
    error: String,
    state: State,
}

impl ContainerReportData {
    fn to_value(&self) -> Value {
        let mut map = HashMap::new();
        map.insert("ID".to_string(), Value::from(self.id.clone()));
        map.insert("Name".to_string(), Value::from(self.name.clone()));
        map.insert(
            "CurrentImageID".to_string(),
            image_id_value(&self.current_image_id),
        );
        map.insert(
            "LatestImageID".to_string(),
            image_id_value(&self.latest_image_id),
        );
        map.insert(
            "ImageName".to_string(),
            Value::from(self.image_name.clone()),
        );
        map.insert("Error".to_string(), Value::from(self.error.clone()));
        map.insert("State".to_string(), Value::from(self.state.as_str()));
        Value::from(map)
    }
}

#[derive(Clone, Debug, Default)]
struct ReportData {
    scanned: Vec<ContainerReportData>,
    updated: Vec<ContainerReportData>,
    failed: Vec<ContainerReportData>,
    skipped: Vec<ContainerReportData>,
    stale: Vec<ContainerReportData>,
    fresh: Vec<ContainerReportData>,
}

impl ReportData {
    fn add(&mut self, container: ContainerReportData) {
        match container.state {
            State::Scanned => self.scanned.push(container),
            State::Updated => self.updated.push(container),
            State::Failed => self.failed.push(container),
            State::Skipped => self.skipped.push(container),
            State::Stale => self.stale.push(container),
            State::Fresh => self.fresh.push(container),
        }
    }

    fn all(&self) -> Vec<ContainerReportData> {
        let total = self.scanned.len()
            + self.updated.len()
            + self.failed.len()
            + self.skipped.len()
            + self.stale.len()
            + self.fresh.len();
        let mut all = Vec::with_capacity(total);
        let mut seen = HashSet::<&str>::new();

        for bucket in [
            &self.updated,
            &self.failed,
            &self.skipped,
            &self.stale,
            &self.fresh,
            &self.scanned,
        ] {
            for report in bucket {
                if seen.insert(report.id.as_str()) {
                    all.push(report.clone());
                }
            }
        }

        all.sort_by(|left, right| left.id.cmp(&right.id));
        all
    }

    fn to_value(&self) -> Value {
        let mut map = HashMap::new();
        map.insert(
            "Scanned".to_string(),
            list_value(self.scanned.iter().map(ContainerReportData::to_value)),
        );
        map.insert(
            "Updated".to_string(),
            list_value(self.updated.iter().map(ContainerReportData::to_value)),
        );
        map.insert(
            "Failed".to_string(),
            list_value(self.failed.iter().map(ContainerReportData::to_value)),
        );
        map.insert(
            "Skipped".to_string(),
            list_value(self.skipped.iter().map(ContainerReportData::to_value)),
        );
        map.insert(
            "Stale".to_string(),
            list_value(self.stale.iter().map(ContainerReportData::to_value)),
        );
        map.insert(
            "Fresh".to_string(),
            list_value(self.fresh.iter().map(ContainerReportData::to_value)),
        );
        map.insert(
            "All".to_string(),
            list_value(self.all().into_iter().map(|report| report.to_value())),
        );
        Value::from(map)
    }
}

#[derive(Clone, Debug)]
struct LogEntryData {
    message: String,
    data: HashMap<String, Value>,
    time: String,
    level: LogLevel,
}

impl LogEntryData {
    fn to_value(&self) -> Value {
        let mut map = HashMap::new();
        map.insert("Message".to_string(), Value::from(self.message.clone()));
        map.insert("Data".to_string(), Value::from(self.data.clone()));
        map.insert("Time".to_string(), Value::from(self.time.clone()));
        map.insert("Level".to_string(), Value::from(self.level.as_str()));
        Value::from(map)
    }
}

#[derive(Clone, Debug)]
struct StaticData {
    title: String,
    host: String,
}

impl StaticData {
    fn to_value(&self) -> Value {
        let mut map = HashMap::new();
        map.insert("Title".to_string(), Value::from(self.title.clone()));
        map.insert("Host".to_string(), Value::from(self.host.clone()));
        Value::from(map)
    }
}

#[derive(Clone, Debug)]
struct PreviewData {
    rng: StdRng,
    last_time: OffsetDateTime,
    report: Option<ReportData>,
    container_count: usize,
    entries: Vec<LogEntryData>,
    static_data: StaticData,
}

impl PreviewData {
    fn new() -> Self {
        Self {
            rng: StdRng::seed_from_u64(1),
            last_time: OffsetDateTime::now_utc() - Duration::minutes(30),
            report: None,
            container_count: 0,
            entries: Vec::new(),
            static_data: StaticData {
                title: "Title".to_string(),
                host: "Host".to_string(),
            },
        }
    }

    fn add_from_state(&mut self, state: State) {
        let cid = self.generate_id();
        let old = self.generate_id();
        let new = self.generate_id();
        let name = self.generate_name();
        let image = self.generate_image_name(&name);
        let error = match state {
            State::Failed => random_entry(&mut self.rng, ERROR_MESSAGES).to_string(),
            State::Skipped => random_entry(&mut self.rng, SKIPPED_MESSAGES).to_string(),
            _ => String::new(),
        };

        let report = self.report.get_or_insert_with(ReportData::default);
        report.add(ContainerReportData {
            id: cid,
            name,
            current_image_id: old,
            latest_image_id: new,
            image_name: image,
            error,
            state,
        });

        self.container_count += 1;
    }

    fn add_log_entry(&mut self, level: LogLevel) {
        let message = match level {
            LogLevel::Fatal | LogLevel::Error | LogLevel::Warn => {
                random_entry(&mut self.rng, LOG_ERRORS).to_string()
            }
            _ => random_entry(&mut self.rng, LOG_MESSAGES).to_string(),
        };
        let time = self.generate_time();

        self.entries.push(LogEntryData {
            message,
            data: HashMap::new(),
            time,
            level,
        });
    }

    fn generate_id(&mut self) -> String {
        let mut bytes = [0u8; 32];
        self.rng.fill_bytes(&mut bytes);
        hex_encode(&bytes)
    }

    fn generate_time(&mut self) -> String {
        let offset = self.rng.gen_range(0..30);
        self.last_time += Duration::seconds(offset as i64);
        format_time(self.last_time)
    }

    fn generate_name(&self) -> String {
        let index = self.container_count;
        if index < CONTAINER_NAMES.len() {
            format!("/{}", CONTAINER_NAMES[index])
        } else {
            let suffix = index / CONTAINER_NAMES.len();
            let slot = index % CONTAINER_NAMES.len();
            format!("/{}{}", CONTAINER_NAMES[slot], suffix)
        }
    }

    fn generate_image_name(&self, name: &str) -> String {
        let index = self.container_count % ORGANIZATION_NAMES.len();
        format!("{}{}:latest", ORGANIZATION_NAMES[index], name)
    }

    fn to_value(&self) -> Value {
        let mut map = HashMap::new();
        map.insert(
            "Entries".to_string(),
            list_value(self.entries.iter().map(LogEntryData::to_value)),
        );
        map.insert("StaticData".to_string(), self.static_data.to_value());
        map.insert(
            "Report".to_string(),
            self.report
                .as_ref()
                .map_or(Value::NoValue, ReportData::to_value),
        );
        Value::from(map)
    }
}

fn render_preview(input: &str, states: &[State], levels: &[LogLevel]) -> Result<String, String> {
    let mut data = PreviewData::new();
    let template = build_template(input)?;

    for state in states {
        data.add_from_state(*state);
    }

    for level in levels {
        data.add_log_entry(*level);
    }

    template
        .render(&Context::from(data.to_value()))
        .map_err(|err| format!("failed to execute template: {err}"))
}

/// Render a notification template from the compact preview selectors.
pub fn render_preview_from_strings(
    input: &str,
    states: &str,
    entries: &str,
) -> Result<String, String> {
    let states = states_from_string(states);
    let levels = levels_from_string(entries);
    render_preview(input, &states, &levels)
}

fn build_template(input: &str) -> Result<Template, String> {
    let mut template = Template::default();
    template.add_funcs(&legacy_funcs());
    template
        .parse(input)
        .map_err(|err| format!("failed to parse {err}"))?;
    Ok(template)
}

fn legacy_funcs() -> [(&'static str, Func); 4] {
    [
        ("ToJSON", to_json as Func),
        ("ToUpper", to_upper as Func),
        ("ToLower", to_lower as Func),
        ("Title", to_title as Func),
    ]
}

fn to_json(args: &[Value]) -> Result<Value, FuncError> {
    if args.len() != 1 {
        return Err(FuncError::ExactlyXArgs("ToJSON".into(), 1));
    }

    match value_to_json(&args[0]) {
        Ok(json) => Ok(Value::from(
            serde_json::to_string_pretty(&json).unwrap_or_else(|_| json.to_string()),
        )),
        Err(err) => Ok(Value::from(format!(
            "failed to marshal JSON in notification template: {err}"
        ))),
    }
}

fn to_upper(args: &[Value]) -> Result<Value, FuncError> {
    map_single_string("ToUpper", args, |value| value.to_uppercase())
}

fn to_lower(args: &[Value]) -> Result<Value, FuncError> {
    map_single_string("ToLower", args, |value| value.to_lowercase())
}

fn to_title(args: &[Value]) -> Result<Value, FuncError> {
    map_single_string("Title", args, |value| titlecase(&value))
}

fn map_single_string(
    name: &'static str,
    args: &[Value],
    map: impl FnOnce(String) -> String,
) -> Result<Value, FuncError> {
    if args.len() != 1 {
        return Err(FuncError::ExactlyXArgs(name.into(), 1));
    }

    let value: String = from_value(&args[0]).ok_or(FuncError::UnableToConvertFromValue)?;
    Ok(Value::from(map(value)))
}

fn value_to_json(value: &Value) -> Result<JsonValue, String> {
    match value {
        Value::NoValue | Value::Nil => Ok(JsonValue::Null),
        Value::Bool(v) => Ok(JsonValue::Bool(*v)),
        Value::String(v) => Ok(JsonValue::String(v.clone())),
        Value::Array(v) => {
            let mut out = Vec::with_capacity(v.len());
            for item in v {
                out.push(value_to_json(item)?);
            }
            Ok(JsonValue::Array(out))
        }
        Value::Object(v) | Value::Map(v) => {
            let mut out = JsonMap::new();
            for (key, item) in v {
                out.insert(key.clone(), value_to_json(item)?);
            }
            Ok(JsonValue::Object(out))
        }
        Value::Number(v) => {
            if let Some(number) = v.as_i64() {
                Ok(JsonValue::Number(JsonNumber::from(number)))
            } else if let Some(number) = v.as_u64() {
                Ok(JsonValue::Number(JsonNumber::from(number)))
            } else if let Some(number) = v.as_f64() {
                JsonNumber::from_f64(number)
                    .map(JsonValue::Number)
                    .ok_or_else(|| "cannot convert non-finite number to JSON".to_string())
            } else {
                Err("unable to convert number to JSON".to_string())
            }
        }
        Value::Function(_) => Err("cannot marshal function values to JSON".to_string()),
    }
}

fn image_id_value(id: &str) -> Value {
    let mut map = HashMap::new();
    map.insert("String".to_string(), Value::from(id.to_string()));
    map.insert("ShortID".to_string(), Value::from(short_id(id)));
    Value::from(map)
}

fn short_id(id: &str) -> String {
    let trimmed = id.strip_prefix("sha256:").unwrap_or(id);
    trimmed.chars().take(12).collect()
}

fn list_value<I>(items: I) -> Value
where
    I: IntoIterator<Item = Value>,
{
    Value::from(items.into_iter().collect::<Vec<_>>())
}

fn random_entry<'a>(rng: &mut StdRng, entries: &'a [&'a str]) -> &'a str {
    let index = rng.gen_range(0..entries.len());
    entries[index]
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn format_time(time: OffsetDateTime) -> String {
    let date = time.date();
    let time_of_day = time.time();

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} +0000 UTC",
        date.year(),
        u8::from(date.month()),
        date.day(),
        time_of_day.hour(),
        time_of_day.minute(),
        time_of_day.second()
    )
}

fn states_from_string(input: &str) -> Vec<State> {
    input.chars().filter_map(State::from_compact_char).collect()
}

fn levels_from_string(input: &str) -> Vec<LogLevel> {
    input
        .chars()
        .filter_map(LogLevel::from_compact_char)
        .collect()
}

#[cfg(target_arch = "wasm32")]
fn states_from_js_arg(value: &JsValue) -> Vec<State> {
    if let Some(text) = value.as_string() {
        states_from_string(&text)
    } else {
        let mut states = Vec::new();
        for item in Array::from(value).iter() {
            if let Some(text) = item.as_string() {
                if let Some(state) = State::from_explicit_name(&text) {
                    states.push(state);
                }
            }
        }
        states
    }
}

#[cfg(target_arch = "wasm32")]
fn levels_from_js_arg(value: &JsValue) -> Vec<LogLevel> {
    if let Some(text) = value.as_string() {
        levels_from_string(&text)
    } else {
        let mut levels = Vec::new();
        for item in Array::from(value).iter() {
            if let Some(text) = item.as_string() {
                if let Some(level) = LogLevel::from_explicit_name(&text) {
                    levels.push(level);
                }
            }
        }
        levels
    }
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
    let states = states_from_js_arg(&states);
    let levels = levels_from_js_arg(&levels);

    match render_preview(&input, &states, &levels) {
        Ok(result) => JsValue::from_str(&result),
        Err(error) => JsValue::from_str(&format!("Error: {error}")),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("watchtower/tplprev v{}\n", meta::version());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_state_parsing_matches_legacy_letters() {
        assert_eq!(
            states_from_string("cuxe ktf"),
            vec![
                State::Scanned,
                State::Updated,
                State::Failed,
                State::Skipped,
                State::Stale,
                State::Fresh,
            ]
        );
    }

    #[test]
    fn compact_level_parsing_matches_legacy_letters() {
        assert_eq!(
            levels_from_string("pfewidt"),
            vec![
                LogLevel::Panic,
                LogLevel::Fatal,
                LogLevel::Error,
                LogLevel::Warn,
                LogLevel::Info,
                LogLevel::Debug,
                LogLevel::Trace,
            ]
        );
    }

    #[test]
    fn render_keeps_static_data_and_report_shape() {
        let rendered = render_preview(
            "{{.StaticData.Title}} {{.StaticData.Host}} {{if .Report}}{{len .Report.Scanned}}{{end}}",
            &[State::Scanned],
            &[],
        )
        .expect("render should succeed");

        assert_eq!(rendered.split_whitespace().next(), Some("Title"));
        assert!(rendered.contains("Host"));
        assert!(rendered.trim_end().ends_with('1'));
    }

    #[test]
    fn render_formats_logs_and_json_helper() {
        let rendered = render_preview(
            "{{range .Entries}}{{.Level}} {{.Message}}{{println}}{{end}}{{ . | ToJSON }}",
            &[],
            &[LogLevel::Error],
        )
        .expect("render should succeed");

        assert!(rendered.contains("error"));
        assert!(!rendered.contains("failed to marshal JSON"));
    }

    #[test]
    fn report_all_deduplicates_by_id() {
        let mut report = ReportData::default();
        report.add(ContainerReportData {
            id: "b".to_string(),
            name: "one".to_string(),
            current_image_id: "sha256:1234567890abcdef".to_string(),
            latest_image_id: "sha256:abcdef1234567890".to_string(),
            image_name: "img".to_string(),
            error: String::new(),
            state: State::Updated,
        });
        report.add(ContainerReportData {
            id: "a".to_string(),
            name: "two".to_string(),
            current_image_id: "sha256:1111111111111111".to_string(),
            latest_image_id: "sha256:2222222222222222".to_string(),
            image_name: "img".to_string(),
            error: String::new(),
            state: State::Scanned,
        });

        let all = report.all();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, "a");
        assert_eq!(all[1].id, "b");
    }
}
