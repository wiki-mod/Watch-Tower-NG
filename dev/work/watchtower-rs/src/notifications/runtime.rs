use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use go_template::{Context, Func, FuncError, Template, Value, from_value};
use serde_json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};
use thiserror::Error;
use titlecase::titlecase;
use tracing::{error, info};

use crate::notifier::{NotificationLogLevel, NotifierSetup};
use crate::notifications::{
    Data, NotificationEntry, StaticData, common_template, default_template, get_scheme,
};
use crate::types::{ContainerReport, Report};

pub const SHOUTRRR_TYPE: &str = "shoutrrr";

type TemplateResult<T> = Result<T, String>;

/// Minimal title-only parameter surface used by the Shoutrrr runtime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShoutrrrParams {
    title: Option<String>,
}

impl ShoutrrrParams {
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = Some(title.into());
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
}

/// One delivery result aligned by index with the configured URLs.
pub type DeliveryResults = Vec<Result<(), NotificationDeliveryError>>;

/// Router surface required by the legacy runtime.
pub trait ShoutrrrRouter: Any + Send + Sync + 'static {
    fn send(&self, message: &str, params: &ShoutrrrParams) -> DeliveryResults;
    fn as_any(&self) -> &dyn Any;
}

impl<T> ShoutrrrRouter for Arc<T>
where
    T: ShoutrrrRouter + ?Sized,
{
    fn send(&self, message: &str, params: &ShoutrrrParams) -> DeliveryResults {
        (**self).send(message, params)
    }

    fn as_any(&self) -> &dyn Any {
        (**self).as_any()
    }
}

/// Factory surface used by the setup-to-runtime bridge.
pub trait ShoutrrrRouterFactory {
    type Router: ShoutrrrRouter;
    type Error: std::error::Error + Send + Sync + 'static;

    fn create(&self, urls: &[String], stdout: bool) -> Result<Self::Router, Self::Error>;
}

/// Diagnostics sink for the translated runtime paths.
pub trait NotificationDiagnostics: Send + Sync + 'static {
    fn template_fallback(&self, error: &str);
    fn send_failure(&self, service: &str, index: usize, error: &NotificationDeliveryError);
    fn skip_empty_message(&self);
    fn waiting_for_worker(&self);
    fn fatal_template_error(&self, error: &str);
}

impl<T> NotificationDiagnostics for Arc<T>
where
    T: NotificationDiagnostics + ?Sized,
{
    fn template_fallback(&self, error: &str) {
        (**self).template_fallback(error);
    }

    fn send_failure(&self, service: &str, index: usize, error: &NotificationDeliveryError) {
        (**self).send_failure(service, index, error);
    }

    fn skip_empty_message(&self) {
        (**self).skip_empty_message();
    }

    fn waiting_for_worker(&self) {
        (**self).waiting_for_worker();
    }

    fn fatal_template_error(&self, error: &str) {
        (**self).fatal_template_error(error);
    }
}

/// Default diagnostics sink backed by `tracing`.
#[derive(Debug, Default, Clone, Copy)]
pub struct TracingNotificationDiagnostics;

impl NotificationDiagnostics for TracingNotificationDiagnostics {
    fn template_fallback(&self, error: &str) {
        error!(
            "Could not use configured notification template: {error}. Using default template"
        );
    }

    fn send_failure(&self, service: &str, index: usize, error: &NotificationDeliveryError) {
        error!(
            service,
            index,
            error = %error,
            "Failed to send shoutrrr notification"
        );
    }

    fn skip_empty_message(&self) {
        info!("Skipping notification due to empty message");
    }

    fn waiting_for_worker(&self) {
        info!("Waiting for the notification goroutine to finish");
    }

    fn fatal_template_error(&self, error: &str) {
        panic!("Notification template error: {error}");
    }
}

/// Log-hook surface mirroring the legacy notifier hook.
pub trait NotificationLogHook: Send + Sync {
    fn levels(&self) -> Vec<NotificationLogLevel>;
    fn fire(&self, entry: NotificationEntry) -> Result<(), ShoutrrrNotifierError>;
}

/// Registry surface for attaching the notifier as a log hook.
pub trait NotificationHookRegistry {
    fn add_hook(&mut self, hook: Arc<dyn NotificationLogHook>);
}

/// A delivery failure reported by the router.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("{message}")]
pub struct NotificationDeliveryError {
    message: String,
}

impl NotificationDeliveryError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Construction failures for the translated runtime.
#[derive(Debug, Error)]
pub enum ShoutrrrNotifierInitError {
    #[error("failed to initialize shoutrrr notifications: {0}")]
    RouterInitialization(String),
}

/// Runtime failures after construction.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ShoutrrrNotifierError {
    #[error("notification queue closed")]
    QueueClosed,

    #[error("notification worker panicked")]
    WorkerPanicked,
}

#[derive(Debug)]
struct RuntimeState {
    entries: Option<Vec<NotificationEntry>>,
    sender: Option<mpsc::SyncSender<String>>,
    receiver: Option<mpsc::Receiver<String>>,
    worker: Option<JoinHandle<()>>,
}

/// Translated Shoutrrr notification runtime from the legacy Go implementation.
pub struct ShoutrrrNotifier {
    urls: Vec<String>,
    router: Arc<dyn ShoutrrrRouter>,
    diagnostics: Arc<dyn NotificationDiagnostics>,
    log_level: NotificationLogLevel,
    template: Mutex<Template>,
    legacy_template: bool,
    params: ShoutrrrParams,
    data: StaticData,
    delay: Duration,
    hook_registered: AtomicBool,
    worker_started: AtomicBool,
    state: Mutex<RuntimeState>,
}

impl ShoutrrrNotifier {
    pub fn create_with_factory<F, D>(
        setup: NotifierSetup,
        factory: &F,
        diagnostics: D,
    ) -> Result<Arc<Self>, ShoutrrrNotifierInitError>
    where
        F: ShoutrrrRouterFactory,
        D: NotificationDiagnostics,
    {
        let router = factory
            .create(&setup.urls, setup.stdout)
            .map_err(|err| ShoutrrrNotifierInitError::RouterInitialization(err.to_string()))?;

        Ok(Self::create_with_router(
            setup.urls,
            setup.level,
            &setup.template,
            setup.legacy_template,
            setup.data,
            router,
            diagnostics,
            setup.delay,
        ))
    }

    pub fn create_with_router<R, D>(
        urls: Vec<String>,
        level: NotificationLogLevel,
        tpl_string: &str,
        legacy: bool,
        data: StaticData,
        router: R,
        diagnostics: D,
        delay: Duration,
    ) -> Arc<Self>
    where
        R: ShoutrrrRouter,
        D: NotificationDiagnostics,
    {
        let diagnostics = Arc::new(diagnostics);
        let template = build_template(tpl_string, legacy).unwrap_or_else(|err| {
            diagnostics.template_fallback(&err);
            default_parsed_template(legacy)
        });

        let mut params = ShoutrrrParams::default();
        if !data.title.is_empty() {
            params.set_title(data.title.clone());
        }

        // Preserve the queue from construction onward so messages buffered
        // before the worker starts are drained by the same worker later.
        let (sender, receiver) = mpsc::sync_channel::<String>(1);
        Arc::new(Self {
            urls,
            router: Arc::new(router),
            diagnostics,
            log_level: level,
            template: Mutex::new(template),
            legacy_template: legacy,
            params,
            data,
            delay,
            hook_registered: AtomicBool::new(false),
            worker_started: AtomicBool::new(false),
            state: Mutex::new(RuntimeState {
                entries: None,
                sender: Some(sender),
                receiver: Some(receiver),
                worker: None,
            }),
        })
    }

    pub fn get_names(&self) -> Vec<String> {
        self.urls
            .iter()
            .map(|url| get_scheme(url).to_string())
            .collect()
    }

    pub fn get_urls(&self) -> &[String] {
        &self.urls
    }

    pub fn params(&self) -> &ShoutrrrParams {
        &self.params
    }

    pub fn add_log_hook(self: &Arc<Self>, registry: &mut impl NotificationHookRegistry) {
        if self.hook_registered.swap(true, Ordering::SeqCst) {
            return;
        }

        registry.add_hook(self.clone());
        self.start_async_delivery();
    }

    pub fn start_async_delivery(&self) {
        if self.worker_started.swap(true, Ordering::SeqCst) {
            return;
        }

        let router = Arc::clone(&self.router);
        let urls = self.urls.clone();
        let params = self.params.clone();
        let diagnostics = Arc::clone(&self.diagnostics);
        let delay = self.delay;
        let receiver = {
            let mut state = self.lock_state();
            state
                .receiver
                .take()
                .expect("notification receiver should exist before worker start")
        };
        let worker = thread::spawn(move || {
            while let Ok(message) = receiver.recv() {
                thread::sleep(delay);
                for (index, result) in router.send(&message, &params).into_iter().enumerate() {
                    if let Err(err) = result {
                        let service = urls
                            .get(index)
                            .map(|url| get_scheme(url))
                            .unwrap_or("invalid");
                        diagnostics.send_failure(service, index, &err);
                    }
                }
            }
        });

        let mut state = self.lock_state();
        state.worker = Some(worker);
    }

    pub fn build_message(&self, data: Data) -> TemplateResult<String> {
        let context = template_context(&data, self.legacy_template);
        let template = self.template.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        template
            .render(&context)
            .map_err(|err| format!("failed to execute template: {err}"))
    }

    pub fn start_notification(&self) {
        let mut state = self.lock_state();
        if state.entries.is_none() {
            state.entries = Some(Vec::with_capacity(10));
        }
    }

    pub fn send_notification(&self, report: Option<Report>) -> Result<(), ShoutrrrNotifierError> {
        let entries = self
            .lock_state()
            .entries
            .take()
            .unwrap_or_default();
        self.send_entries(entries, report)
    }

    pub fn close(&self) -> Result<(), ShoutrrrNotifierError> {
        let worker = {
            let mut state = self.lock_state();
            state.sender.take();
            state.receiver.take();
            state.worker.take()
        };

        if let Some(worker) = worker {
            self.diagnostics.waiting_for_worker();
            worker.join().map_err(|_| ShoutrrrNotifierError::WorkerPanicked)?;
        }

        Ok(())
    }

    pub fn fire(&self, entry: NotificationEntry) -> Result<(), ShoutrrrNotifierError> {
        if should_skip_entry(&entry) {
            return Ok(());
        }

        let mut immediate_entry = Some(entry);
        let queued = {
            let mut state = self.lock_state();
            if let Some(entries) = state.entries.as_mut() {
                entries.push(immediate_entry.take().expect("entry should exist"));
                true
            } else {
                false
            }
        };

        if queued {
            Ok(())
        } else {
            self.send_entries(
                vec![immediate_entry.expect("entry should remain available for immediate send")],
                None,
            )
        }
    }

    fn send_entries(
        &self,
        entries: Vec<NotificationEntry>,
        report: Option<Report>,
    ) -> Result<(), ShoutrrrNotifierError> {
        let data = Data::new(self.data.clone(), entries, report);
        let message = self.build_message(data.clone());

        match message {
            Ok(message) if !message.is_empty() => self
                .lock_state()
                .sender
                .as_ref()
                .ok_or(ShoutrrrNotifierError::QueueClosed)?
                .send(message)
                .map_err(|_| ShoutrrrNotifierError::QueueClosed),
            Ok(_) => {
                if self.urls.len() > 1 {
                    let diagnostics = Arc::clone(&self.diagnostics);
                    thread::spawn(move || diagnostics.skip_empty_message());
                }
                Ok(())
            }
            Err(err) => {
                let diagnostics = Arc::clone(&self.diagnostics);
                thread::spawn(move || diagnostics.fatal_template_error(&err));
                Ok(())
            }
        }
    }

    fn lock_state(&self) -> MutexGuard<'_, RuntimeState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl NotificationLogHook for ShoutrrrNotifier {
    fn levels(&self) -> Vec<NotificationLogLevel> {
        legacy_levels(self.log_level)
    }

    fn fire(&self, entry: NotificationEntry) -> Result<(), ShoutrrrNotifierError> {
        ShoutrrrNotifier::fire(self, entry)
    }
}

impl NotifierSetup {
    pub fn into_shoutrrr_notifier<F, D>(
        self,
        factory: &F,
        diagnostics: D,
    ) -> Result<Arc<ShoutrrrNotifier>, ShoutrrrNotifierInitError>
    where
        F: ShoutrrrRouterFactory,
        D: NotificationDiagnostics,
    {
        ShoutrrrNotifier::create_with_factory(self, factory, diagnostics)
    }
}

fn should_skip_entry(entry: &NotificationEntry) -> bool {
    entry.data.as_ref().and_then(JsonValue::as_object).and_then(|data| {
        data.get("notify").and_then(JsonValue::as_str)
    }) == Some("no")
}

fn legacy_levels(level: NotificationLogLevel) -> Vec<NotificationLogLevel> {
    let all = [
        NotificationLogLevel::Panic,
        NotificationLogLevel::Fatal,
        NotificationLogLevel::Error,
        NotificationLogLevel::Warn,
        NotificationLogLevel::Info,
        NotificationLogLevel::Debug,
        NotificationLogLevel::Trace,
    ];

    let end = match level {
        NotificationLogLevel::Panic => 0,
        NotificationLogLevel::Fatal => 1,
        NotificationLogLevel::Error => 2,
        NotificationLogLevel::Warn => 3,
        NotificationLogLevel::Info => 4,
        NotificationLogLevel::Debug => 5,
        NotificationLogLevel::Trace => 6,
    };

    all[..=end].to_vec()
}

fn build_template(input: &str, legacy: bool) -> Result<Template, String> {
    let resolved = common_template(input).unwrap_or(input);
    if resolved.is_empty() {
        return Ok(default_parsed_template(legacy));
    }

    let mut template = Template::default();
    template.add_funcs(&legacy_funcs());
    template
        .parse(resolved)
        .map_err(|err| format!("{err}"))?;
    Ok(template)
}

fn default_parsed_template(legacy: bool) -> Template {
    let mut template = Template::default();
    template.add_funcs(&legacy_funcs());
    template
        .parse(default_template(legacy))
        .expect("default template should parse");
    template
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

    match template_value_to_json(&args[0]) {
        Ok(json) => Ok(Value::from(
            serde_json::to_string(&json).unwrap_or_else(|_| json.to_string()),
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

fn template_context(data: &Data, legacy: bool) -> Context {
    if legacy {
        Context::from(list_value(data.entries.iter().map(entry_to_template_value)))
    } else {
        Context::from(data_to_template_value(data))
    }
}

fn data_to_template_value(data: &Data) -> Value {
    let mut map = HashMap::new();
    map.insert("Title".to_string(), Value::from(data.static_data.title.clone()));
    map.insert("Host".to_string(), Value::from(data.static_data.host.clone()));
    map.insert("StaticData".to_string(), static_data_value(&data.static_data));
    map.insert(
        "Entries".to_string(),
        list_value(data.entries.iter().map(entry_to_template_value)),
    );
    map.insert(
        "Report".to_string(),
        data.report
            .as_ref()
            .map(report_to_template_value)
            .unwrap_or(Value::NoValue),
    );
    Value::from(map)
}

fn static_data_value(data: &StaticData) -> Value {
    let mut map = HashMap::new();
    map.insert("Title".to_string(), Value::from(data.title.clone()));
    map.insert("Host".to_string(), Value::from(data.host.clone()));
    Value::from(map)
}

fn report_to_template_value(report: &Report) -> Value {
    let mut map = HashMap::new();
    let all = report.all();
    map.insert(
        "Scanned".to_string(),
        list_value(report.scanned.iter().map(container_report_to_template_value)),
    );
    map.insert(
        "Updated".to_string(),
        list_value(report.updated.iter().map(container_report_to_template_value)),
    );
    map.insert(
        "Failed".to_string(),
        list_value(report.failed.iter().map(container_report_to_template_value)),
    );
    map.insert(
        "Skipped".to_string(),
        list_value(report.skipped.iter().map(container_report_to_template_value)),
    );
    map.insert(
        "Stale".to_string(),
        list_value(report.stale.iter().map(container_report_to_template_value)),
    );
    map.insert(
        "Fresh".to_string(),
        list_value(report.fresh.iter().map(container_report_to_template_value)),
    );
    map.insert(
        "All".to_string(),
        list_value(all.iter().map(container_report_to_template_value)),
    );
    Value::from(map)
}

fn container_report_to_template_value(report: &ContainerReport) -> Value {
    let mut map = HashMap::new();
    map.insert("ID".to_string(), id_value(&report.id));
    map.insert("Name".to_string(), Value::from(report.name.clone()));
    map.insert(
        "CurrentImageID".to_string(),
        id_value(&report.current_image_id),
    );
    map.insert(
        "LatestImageID".to_string(),
        id_value(&report.latest_image_id),
    );
    map.insert("ImageName".to_string(), Value::from(report.image_name.clone()));
    map.insert(
        "Error".to_string(),
        Value::from(report.error.clone().unwrap_or_default()),
    );
    map.insert("State".to_string(), Value::from(report.state.clone()));
    Value::from(map)
}

fn id_value<T>(id: &T) -> Value
where
    T: ToString,
{
    let raw = id.to_string();
    let short = raw
        .strip_prefix("sha256:")
        .unwrap_or(raw.as_str())
        .chars()
        .take(12)
        .collect::<String>();
    let mut map = HashMap::new();
    map.insert("String".to_string(), Value::from(raw));
    map.insert("ShortID".to_string(), Value::from(short));
    Value::from(map)
}

fn entry_to_template_value(entry: &NotificationEntry) -> Value {
    let mut map = HashMap::new();
    map.insert("Level".to_string(), Value::from(entry.level.clone()));
    map.insert("Message".to_string(), Value::from(entry.message.clone()));
    map.insert(
        "Data".to_string(),
        entry.data
            .as_ref()
            .map(json_to_template_value)
            .unwrap_or(Value::NoValue),
    );
    map.insert("Time".to_string(), Value::from(entry.time.clone()));
    Value::from(map)
}

fn json_to_template_value(value: &JsonValue) -> Value {
    match value {
        JsonValue::Null => Value::NoValue,
        JsonValue::Bool(value) => Value::from(*value),
        JsonValue::Number(value) => {
            if let Some(number) = value.as_i64() {
                Value::from(number)
            } else if let Some(number) = value.as_u64() {
                Value::from(number)
            } else {
                Value::from(value.as_f64().unwrap_or_default())
            }
        }
        JsonValue::String(value) => Value::from(value.clone()),
        JsonValue::Array(values) => list_value(values.iter().map(json_to_template_value)),
        JsonValue::Object(values) => {
            let mut map = HashMap::new();
            for (key, value) in values {
                map.insert(key.clone(), json_to_template_value(value));
            }
            Value::from(map)
        }
    }
}

fn template_value_to_json(value: &Value) -> Result<JsonValue, String> {
    match value {
        Value::NoValue | Value::Nil => Ok(JsonValue::Null),
        Value::Bool(value) => Ok(JsonValue::Bool(*value)),
        Value::String(value) => Ok(JsonValue::String(value.clone())),
        Value::Array(values) => {
            let mut out = Vec::with_capacity(values.len());
            for value in values {
                out.push(template_value_to_json(value)?);
            }
            Ok(JsonValue::Array(out))
        }
        Value::Object(values) | Value::Map(values) => object_value_to_json(values),
        Value::Number(value) => {
            if let Some(number) = value.as_i64() {
                Ok(JsonValue::Number(JsonNumber::from(number)))
            } else if let Some(number) = value.as_u64() {
                Ok(JsonValue::Number(JsonNumber::from(number)))
            } else if let Some(number) = value.as_f64() {
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

fn object_value_to_json(values: &HashMap<String, Value>) -> Result<JsonValue, String> {
    if values.contains_key("Entries") && values.contains_key("Title") && values.contains_key("Host")
    {
        let mut out = JsonMap::new();
        out.insert("title".to_string(), template_value_to_json(&values["Title"])?);
        out.insert("host".to_string(), template_value_to_json(&values["Host"])?);
        out.insert("entries".to_string(), template_value_to_json(&values["Entries"])?);
        out.insert(
            "report".to_string(),
            values
                .get("Report")
                .map(template_value_to_json)
                .transpose()?
                .unwrap_or(JsonValue::Null),
        );
        return Ok(JsonValue::Object(out));
    }

    if values.contains_key("Level") && values.contains_key("Message") && values.contains_key("Time")
    {
        let mut out = JsonMap::new();
        out.insert("level".to_string(), template_value_to_json(&values["Level"])?);
        out.insert(
            "message".to_string(),
            template_value_to_json(&values["Message"])?,
        );
        out.insert("time".to_string(), template_value_to_json(&values["Time"])?);
        out.insert(
            "data".to_string(),
            values
                .get("Data")
                .map(template_value_to_json)
                .transpose()?
                .unwrap_or(JsonValue::Null),
        );
        return Ok(JsonValue::Object(out));
    }

    if values.contains_key("Scanned")
        && values.contains_key("Updated")
        && values.contains_key("Failed")
        && values.contains_key("Skipped")
        && values.contains_key("Stale")
        && values.contains_key("Fresh")
    {
        let mut out = JsonMap::new();
        for (template_key, json_key) in [
            ("Scanned", "scanned"),
            ("Updated", "updated"),
            ("Failed", "failed"),
            ("Skipped", "skipped"),
            ("Stale", "stale"),
            ("Fresh", "fresh"),
        ] {
            out.insert(json_key.to_string(), template_value_to_json(&values[template_key])?);
        }
        return Ok(JsonValue::Object(out));
    }

    if values.contains_key("ID")
        && values.contains_key("Name")
        && values.contains_key("CurrentImageID")
        && values.contains_key("LatestImageID")
        && values.contains_key("ImageName")
        && values.contains_key("State")
    {
        let mut out = JsonMap::new();
        out.insert(
            "id".to_string(),
            template_short_id_to_json(&values["ID"])?,
        );
        out.insert("name".to_string(), template_value_to_json(&values["Name"])?);
        out.insert(
            "currentImageId".to_string(),
            template_short_id_to_json(&values["CurrentImageID"])?,
        );
        out.insert(
            "latestImageId".to_string(),
            template_short_id_to_json(&values["LatestImageID"])?,
        );
        out.insert(
            "imageName".to_string(),
            template_value_to_json(&values["ImageName"])?,
        );
        out.insert("state".to_string(), template_value_to_json(&values["State"])?);
        if let Some(error) = values.get("Error").and_then(value_as_non_empty_string) {
            out.insert("error".to_string(), JsonValue::String(error.to_string()));
        }
        return Ok(JsonValue::Object(out));
    }

    let mut out = JsonMap::new();
    for (key, value) in values {
        out.insert(key.clone(), template_value_to_json(value)?);
    }
    Ok(JsonValue::Object(out))
}

fn template_short_id_to_json(value: &Value) -> Result<JsonValue, String> {
    match value {
        Value::Object(values) | Value::Map(values) => values
            .get("ShortID")
            .map(template_value_to_json)
            .transpose()?
            .ok_or_else(|| "missing ShortID".to_string()),
        _ => template_value_to_json(value),
    }
}

fn value_as_non_empty_string(value: &Value) -> Option<&str> {
    match value {
        Value::String(value) if !value.is_empty() => Some(value.as_str()),
        _ => None,
    }
}

fn list_value<I>(items: I) -> Value
where
    I: IntoIterator<Item = Value>,
{
    Value::from(items.into_iter().collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{self, Receiver, SyncSender};
    use std::time::Duration;

    use crate::types::{ContainerID, ImageID};

    #[derive(Default)]
    struct RecordingRegistry {
        hooks: Vec<Arc<dyn NotificationLogHook>>,
    }

    impl NotificationHookRegistry for RecordingRegistry {
        fn add_hook(&mut self, hook: Arc<dyn NotificationLogHook>) {
            self.hooks.push(hook);
        }
    }

    #[derive(Default)]
    struct RecordingDiagnostics {
        template_fallbacks: Mutex<Vec<String>>,
        send_failures: Mutex<Vec<String>>,
        empty_skips: Mutex<usize>,
        waiting_calls: Mutex<usize>,
        fatal_errors: Mutex<Vec<String>>,
    }

    impl NotificationDiagnostics for RecordingDiagnostics {
        fn template_fallback(&self, error: &str) {
            self.template_fallbacks
                .lock()
                .unwrap()
                .push(error.to_string());
        }

        fn send_failure(&self, service: &str, index: usize, error: &NotificationDeliveryError) {
            self.send_failures
                .lock()
                .unwrap()
                .push(format!("{service}:{index}:{error}"));
        }

        fn skip_empty_message(&self) {
            *self.empty_skips.lock().unwrap() += 1;
        }

        fn waiting_for_worker(&self) {
            *self.waiting_calls.lock().unwrap() += 1;
        }

        fn fatal_template_error(&self, error: &str) {
            self.fatal_errors.lock().unwrap().push(error.to_string());
        }
    }

    #[derive(Default)]
    struct RecordingRouter {
        sent: Mutex<Vec<(String, Option<String>)>>,
        failures: Mutex<Vec<Result<(), NotificationDeliveryError>>>,
    }

    impl RecordingRouter {
        fn with_failures(failures: Vec<Result<(), NotificationDeliveryError>>) -> Self {
            Self {
                sent: Mutex::new(Vec::new()),
                failures: Mutex::new(failures),
            }
        }
    }

    impl ShoutrrrRouter for RecordingRouter {
        fn send(&self, message: &str, params: &ShoutrrrParams) -> DeliveryResults {
            self.sent
                .lock()
                .unwrap()
                .push((message.to_string(), params.title().map(str::to_string)));
            self.failures.lock().unwrap().clone()
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn notifier<R, D>(
        urls: Vec<String>,
        level: NotificationLogLevel,
        template: &str,
        legacy: bool,
        data: StaticData,
        router: R,
        diagnostics: D,
    ) -> Arc<ShoutrrrNotifier>
    where
        R: ShoutrrrRouter,
        D: NotificationDiagnostics,
    {
        ShoutrrrNotifier::create_with_router(
            urls,
            level,
            template,
            legacy,
            data,
            router,
            diagnostics,
            Duration::ZERO,
        )
    }

    fn sample_entry(message: &str) -> NotificationEntry {
        NotificationEntry::new("info", message, None, "2026-06-20T00:00:00Z")
    }

    fn make_report(
        id: &str,
        name: &str,
        current: &str,
        latest: &str,
        image: &str,
        error: Option<&str>,
        state: &str,
    ) -> ContainerReport {
        ContainerReport {
            id: ContainerID::from(id),
            name: name.to_string(),
            current_image_id: ImageID::from(current),
            latest_image_id: ImageID::from(latest),
            image_name: image.to_string(),
            error: error.map(str::to_string),
            state: state.to_string(),
        }
    }

    #[test]
    fn add_log_hook_registers_once() {
        let notifier = notifier(
            vec!["logger://".to_string()],
            NotificationLogLevel::Trace,
            "",
            true,
            StaticData::default(),
            RecordingRouter::default(),
            RecordingDiagnostics::default(),
        );
        let mut registry = RecordingRegistry::default();

        notifier.add_log_hook(&mut registry);
        notifier.add_log_hook(&mut registry);

        assert_eq!(registry.hooks.len(), 1);
        notifier.close().expect("close should succeed");
    }

    #[test]
    fn build_message_uses_default_legacy_template() {
        let notifier = notifier(
            Vec::new(),
            NotificationLogLevel::Trace,
            "",
            true,
            StaticData::default(),
            RecordingRouter::default(),
            RecordingDiagnostics::default(),
        );

        let message = notifier
            .build_message(Data::new(
                StaticData::default(),
                vec![sample_entry("foo bar")],
                None,
            ))
            .expect("template should render");

        assert_eq!(message, "foo bar\n");
    }

    #[test]
    fn build_message_uses_custom_template_and_title_host_bindings() {
        let notifier = notifier(
            Vec::new(),
            NotificationLogLevel::Trace,
            "{{ .Title }} {{ .Host }} {{range .Entries}}{{ .Level }}: {{ .Message }}{{end}}",
            false,
            StaticData {
                title: "Watchtower updates on Mock".to_string(),
                host: "Mock".to_string(),
            },
            RecordingRouter::default(),
            RecordingDiagnostics::default(),
        );

        let message = notifier
            .build_message(Data::new(
                StaticData {
                    title: "Watchtower updates on Mock".to_string(),
                    host: "Mock".to_string(),
                },
                vec![sample_entry("foo bar")],
                None,
            ))
            .expect("template should render");

        assert_eq!(message, "Watchtower updates on Mock Mock info: foo bar");
    }

    #[test]
    fn invalid_custom_template_falls_back_to_default() {
        let diagnostics = Arc::new(RecordingDiagnostics::default());
        let notifier = notifier(
            Vec::new(),
            NotificationLogLevel::Trace,
            "{{ intentionalSyntaxError",
            true,
            StaticData::default(),
            RecordingRouter::default(),
            Arc::clone(&diagnostics),
        );

        let message = notifier
            .build_message(Data::new(
                StaticData::default(),
                vec![sample_entry("foo bar")],
                None,
            ))
            .expect("default fallback template should render");

        assert_eq!(message, "foo bar\n");
        assert_eq!(diagnostics.template_fallbacks.lock().unwrap().len(), 1);
        assert!(diagnostics.fatal_errors.lock().unwrap().is_empty());
    }

    #[test]
    fn report_templates_preserve_legacy_fields_and_json_shape() {
        let notifier = notifier(
            Vec::new(),
            NotificationLogLevel::Trace,
            "{{range .Report.All}}{{.Name}}{{end}}|{{ . | ToJSON }}",
            false,
            StaticData {
                title: "Watchtower updates on Mock".to_string(),
                host: "Mock".to_string(),
            },
            RecordingRouter::default(),
            RecordingDiagnostics::default(),
        );
        let report = Report {
            scanned: vec![make_report(
                "sha256:01d1100000000000",
                "updt1",
                "sha256:01d1100000000000",
                "sha256:d0a1100000000000",
                "mock/updt1:latest",
                None,
                "Updated",
            )],
            updated: vec![make_report(
                "sha256:01d1100000000000",
                "updt1",
                "sha256:01d1100000000000",
                "sha256:d0a1100000000000",
                "mock/updt1:latest",
                None,
                "Updated",
            )],
            failed: vec![],
            skipped: vec![],
            stale: vec![],
            fresh: vec![],
        };

        let message = notifier
            .build_message(Data::new(
                StaticData {
                    title: "Watchtower updates on Mock".to_string(),
                    host: "Mock".to_string(),
                },
                vec![sample_entry("foo bar")],
                Some(report),
            ))
            .expect("json template should render");

        let (all_output, json_output) = message.split_once('|').expect("all/json separator should exist");
        assert_eq!(all_output, "updt1");

        let json: JsonValue = serde_json::from_str(json_output).expect("json should parse");
        assert_eq!(json["title"], JsonValue::String("Watchtower updates on Mock".to_string()));
        assert_eq!(json["host"], JsonValue::String("Mock".to_string()));
        assert_eq!(json["entries"][0]["message"], JsonValue::String("foo bar".to_string()));
        assert_eq!(json["report"]["updated"][0]["currentImageId"], JsonValue::String("01d110000000".to_string()));
        assert!(!json["report"]
            .as_object()
            .expect("report should stay an object")
            .contains_key("all"));
    }

    #[test]
    fn start_and_send_empty_batch_does_not_deliver() {
        let router = RecordingRouter::default();
        let diagnostics = Arc::new(RecordingDiagnostics::default());
        let notifier = notifier(
            vec!["logger://".to_string()],
            NotificationLogLevel::Debug,
            "",
            true,
            StaticData::default(),
            router,
            Arc::clone(&diagnostics),
        );

        notifier.start_async_delivery();
        notifier.start_notification();
        notifier
            .send_notification(None)
            .expect("empty batch should succeed");
        notifier.close().expect("close should succeed");

        assert!(notifier.router_as::<RecordingRouter>().sent.lock().unwrap().is_empty());
        assert_eq!(*diagnostics.empty_skips.lock().unwrap(), 0);
    }

    #[test]
    fn queued_entries_are_delivered() {
        let router = RecordingRouter::default();
        let notifier = notifier(
            vec!["logger://".to_string()],
            NotificationLogLevel::Debug,
            "",
            true,
            StaticData::default(),
            router,
            RecordingDiagnostics::default(),
        );

        notifier.start_async_delivery();
        notifier.start_notification();
        notifier.fire(sample_entry("ContainrrrVPN")).expect("fire should succeed");
        notifier
            .send_notification(None)
            .expect("send notification should succeed");
        notifier.close().expect("close should succeed");

        let sent = notifier.router_as::<RecordingRouter>().sent.lock().unwrap().clone();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, "ContainrrrVPN\n");
    }

    #[test]
    fn fire_skips_notify_no_entries() {
        let router = RecordingRouter::default();
        let notifier = notifier(
            vec!["logger://".to_string()],
            NotificationLogLevel::Debug,
            "",
            true,
            StaticData::default(),
            router,
            RecordingDiagnostics::default(),
        );

        notifier.start_async_delivery();
        notifier.start_notification();
        notifier
            .fire(NotificationEntry::new(
                "info",
                "do not notify",
                Some(JsonValue::Object(
                    [("notify".to_string(), JsonValue::String("no".to_string()))]
                        .into_iter()
                        .collect(),
                )),
                "2026-06-20T00:00:00Z",
            ))
            .expect("fire should succeed");
        notifier
            .send_notification(None)
            .expect("send notification should succeed");
        notifier.close().expect("close should succeed");

        assert!(notifier.router_as::<RecordingRouter>().sent.lock().unwrap().is_empty());
    }

    #[test]
    fn immediate_message_buffered_before_worker_start_is_delivered() {
        let router = RecordingRouter::default();
        let notifier = notifier(
            vec!["logger://".to_string()],
            NotificationLogLevel::Debug,
            "",
            true,
            StaticData::default(),
            router,
            RecordingDiagnostics::default(),
        );

        notifier.fire(sample_entry("foo bar")).expect("fire should succeed");
        notifier.start_async_delivery();
        notifier.close().expect("close should succeed");

        let sent = notifier.router_as::<RecordingRouter>().sent.lock().unwrap().clone();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, "foo bar\n");
    }

    #[test]
    fn router_failures_are_reported_with_service_and_index() {
        let diagnostics = Arc::new(RecordingDiagnostics::default());
        let notifier = notifier(
            vec!["slack://".to_string()],
            NotificationLogLevel::Debug,
            "",
            true,
            StaticData::default(),
            RecordingRouter::with_failures(vec![Err(NotificationDeliveryError::new("boom"))]),
            Arc::clone(&diagnostics),
        );

        notifier.start_async_delivery();
        notifier.fire(sample_entry("foo bar")).expect("fire should succeed");
        notifier.close().expect("close should succeed");

        assert_eq!(
            diagnostics.send_failures.lock().unwrap().clone(),
            vec!["slack:0:boom".to_string()]
        );
    }

    #[test]
    fn close_waits_for_blocked_delivery() {
        let diagnostics = Arc::new(RecordingDiagnostics::default());
        let (unlock_tx, unlock_rx) = mpsc::sync_channel(1);
        let (sent_tx, sent_rx) = mpsc::sync_channel(1);
        let router = TestBlockingRouter {
            unlock_rx: Mutex::new(unlock_rx),
            sent_tx,
        };
        let notifier = notifier(
            vec!["logger://".to_string()],
            NotificationLogLevel::Debug,
            "",
            true,
            StaticData::default(),
            router,
            Arc::clone(&diagnostics),
        );

        notifier.start_async_delivery();
        notifier.start_notification();
        notifier.fire(sample_entry("foo bar")).expect("fire should succeed");
        notifier
            .send_notification(None)
            .expect("send notification should succeed");

        let cloned = Arc::clone(&notifier);
        let close_handle = thread::spawn(move || cloned.close());

        assert!(sent_rx.recv_timeout(Duration::from_millis(50)).is_err());
        unlock_tx.send(()).expect("unlock should be sent");
        assert!(sent_rx.recv_timeout(Duration::from_secs(1)).is_ok());
        assert!(close_handle.join().expect("close thread should join").is_ok());
        assert_eq!(*diagnostics.waiting_calls.lock().unwrap(), 1);
    }

    #[test]
    fn levels_match_legacy_order() {
        let notifier = notifier(
            Vec::new(),
            NotificationLogLevel::Warn,
            "",
            true,
            StaticData::default(),
            RecordingRouter::default(),
            RecordingDiagnostics::default(),
        );

        assert_eq!(
            notifier.levels(),
            vec![
                NotificationLogLevel::Panic,
                NotificationLogLevel::Fatal,
                NotificationLogLevel::Error,
                NotificationLogLevel::Warn
            ]
        );
    }

    #[test]
    fn empty_title_does_not_set_param() {
        let notifier = notifier(
            vec!["logger://".to_string()],
            NotificationLogLevel::Trace,
            "",
            true,
            StaticData {
                title: String::new(),
                host: "test.host".to_string(),
            },
            RecordingRouter::default(),
            RecordingDiagnostics::default(),
        );

        assert_eq!(notifier.params().title(), None);
    }

    #[test]
    fn setup_bridge_uses_router_factory() {
        #[derive(Default)]
        struct Factory;

        impl ShoutrrrRouterFactory for Factory {
            type Router = RecordingRouter;
            type Error = std::io::Error;

            fn create(
                &self,
                urls: &[String],
                _stdout: bool,
            ) -> Result<Self::Router, Self::Error> {
                assert_eq!(urls, ["logger://"]);
                Ok(RecordingRouter::default())
            }
        }

        let notifier = NotifierSetup {
            urls: vec!["logger://".to_string()],
            level: NotificationLogLevel::Info,
            template: default_template(true).to_string(),
            legacy_template: true,
            data: StaticData::default(),
            stdout: false,
            delay: Duration::ZERO,
        }
        .into_shoutrrr_notifier(&Factory, RecordingDiagnostics::default())
        .expect("factory bridge should succeed");

        assert_eq!(notifier.get_names(), vec!["logger".to_string()]);
    }

    struct TestBlockingRouter {
        unlock_rx: Mutex<Receiver<()>>,
        sent_tx: SyncSender<()>,
    }

    impl ShoutrrrRouter for TestBlockingRouter {
        fn send(&self, _message: &str, _params: &ShoutrrrParams) -> DeliveryResults {
            let _ = self.unlock_rx.lock().unwrap().recv();
            let _ = self.sent_tx.send(());
            Vec::new()
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    trait TestRouterAccess {
        fn router_as<T: 'static>(&self) -> &T;
    }

    impl TestRouterAccess for ShoutrrrNotifier {
        fn router_as<T: 'static>(&self) -> &T {
            self.router
                .as_ref()
                .as_any()
                .downcast_ref::<T>()
                .expect("router type should match")
        }
    }
}
