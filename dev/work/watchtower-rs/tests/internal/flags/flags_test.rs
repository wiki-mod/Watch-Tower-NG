#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{CommandFactory, Parser};

use watchtower_rs::cli::{
    DockerArgs, EmailNotificationArgs, GotifyNotificationArgs, HttpApiArgs, LogFormat, LogLevel,
    LoggingArgs, LoggingConfig, NotificationArgs, PollingMode, SchedulingArgs, SelectionArgs,
    SlackNotificationArgs, TeamsNotificationArgs, UpdateArgs, WatchtowerCli, WatchtowerConfig,
};
use watchtower_rs::flags::{
    expand_secret_list, is_file_reference, resolve_secret_references, resolved_log_format,
    ResolvedLogFormat, DOCKER_API_MIN_VERSION,
};

#[test]
fn test_env_config_defaults() {
    let config = parse_resolved_cli(["watchtower"]);

    assert_eq!(config.docker.host, "unix:///var/run/docker.sock");
    assert_eq!(config.docker.tlsverify, false);
    assert_eq!(config.docker.api_version, DOCKER_API_MIN_VERSION);
}

#[test]
fn test_env_config_custom() {
    let config = parse_resolved_cli([
        "watchtower",
        "--host",
        "some-custom-docker-host",
        "--tlsverify",
        "--api-version",
        "1.99",
    ]);

    assert_eq!(config.docker.host, "some-custom-docker-host");
    assert_eq!(config.docker.tlsverify, true);
    assert_eq!(config.docker.api_version, "1.99");
}

#[test]
fn test_get_secrets_from_files_with_string() {
    let value = "supersecretstring";

    let mut config = config_with_email_password(Some(value.to_string()))
        .try_into()
        .expect("config should resolve");
    resolve_secret_references(&mut config).expect("secrets should resolve");

    assert_eq!(
        config.notifications.email.password.as_deref(),
        Some(value)
    );
}

#[test]
fn test_get_secrets_from_files_with_file() {
    let value = "megasecretstring";
    let file = write_temp_file("watchtower", value);

    let mut config = config_with_email_password(Some(file.to_string_lossy().into_owned()))
        .try_into()
        .expect("config should resolve");
    resolve_secret_references(&mut config).expect("secrets should resolve");

    assert_eq!(
        config.notifications.email.password.as_deref(),
        Some(value)
    );
}

#[test]
fn test_get_slice_secrets_from_files() {
    let values = "entry2\n\nentry3\n";
    let file = write_temp_file("watchtower", values);

    let values = expand_secret_list(vec![
        "entry1".to_string(),
        file.to_string_lossy().into_owned(),
    ])
    .expect("slice secrets should resolve");

    assert_eq!(values, vec!["entry1".to_string(), "entry2".to_string(), "entry3".to_string()]);
}

#[test]
fn test_http_api_periodic_polls_flag() {
    let config = parse_resolved_cli(["watchtower", "--http-api-periodic-polls"]);

    assert_eq!(config.scheduling.periodic_polls, true);
}

#[test]
fn test_is_file() {
    assert_eq!(is_file_reference("https://google.com"), false);

    let current_exe = std::env::current_exe().expect("current executable path should be available");
    assert_eq!(is_file_reference(current_exe.to_string_lossy().as_ref()), true);
}

#[test]
fn test_process_flag_aliases() {
    let config = parse_resolved_cli(["watchtower", "--porcelain", "v1", "--interval", "10", "--trace"]);

    assert_eq!(
        config.notifications.urls,
        vec!["logger://".to_string()]
    );
    assert_eq!(config.notifications.log_stdout, true);
    assert_eq!(config.notifications.report, true);
    assert_eq!(
        config.notifications.template.as_deref(),
        Some("porcelain.v1.summary-no-log")
    );
    assert_eq!(
        config.scheduling.mode,
        PollingMode::Interval(Duration::from_secs(10))
    );
    assert_eq!(config.logging.log_level, LogLevel::Trace);
}

#[test]
fn test_process_flag_aliases_log_level_from_environment() {
    let cli = WatchtowerCli {
        logging: LoggingArgs {
            debug: true,
            ..LoggingArgs::default()
        },
        ..default_cli()
    };
    let config: WatchtowerConfig = cli.try_into().expect("config should resolve");

    assert_eq!(config.logging.log_level, LogLevel::Debug);
}

#[test]
fn test_log_format_flag() {
    let base = LoggingConfig {
        log_level: LogLevel::Info,
        log_format: LogFormat::Pretty,
        debug: false,
        trace: false,
        no_color: false,
        no_startup_message: false,
    };

    assert_eq!(resolved_log_format(&base), ResolvedLogFormat::Pretty);
    let json = LoggingConfig {
        log_format: LogFormat::Json,
        ..base.clone()
    };
    assert_eq!(resolved_log_format(&json), ResolvedLogFormat::Json);
    let logfmt = LoggingConfig {
        log_format: LogFormat::Logfmt,
        ..base
    };
    assert_eq!(resolved_log_format(&logfmt), ResolvedLogFormat::Logfmt);

    assert!(WatchtowerCli::try_parse_from(["watchtower", "--log-format", "cowsay"]).is_err());
}

#[test]
fn test_log_level_flag() {
    assert!(WatchtowerCli::try_parse_from(["watchtower", "--log-level", "gossip"]).is_err());
}

#[test]
fn test_process_flag_aliases_sched_and_interval() {
    let cli = WatchtowerCli::try_parse_from([
        "watchtower",
        "--schedule",
        "@hourly",
        "--interval",
        "10",
    ])
    .expect("parser should accept both values");

    let result: Result<WatchtowerConfig, _> = cli.try_into();
    assert!(result.is_err());
}

#[test]
fn test_process_flag_aliases_schedule_from_environment() {
    let cli = WatchtowerCli {
        scheduling: SchedulingArgs {
            schedule: Some("@hourly".to_string()),
            ..SchedulingArgs::default()
        },
        ..default_cli()
    };
    let config: WatchtowerConfig = cli.try_into().expect("config should resolve");

    assert_eq!(
        config.scheduling.mode,
        PollingMode::Schedule("@hourly".to_string())
    );
}

#[test]
fn test_process_flag_aliases_invalid_porcelaine_version() {
    assert!(WatchtowerCli::try_parse_from(["watchtower", "--porcelain", "cowboy"]).is_err());
}

#[test]
fn test_flags_are_present_in_documentation() {
    let ignored_envs = BTreeSet::from([
        "WATCHTOWER_NOTIFICATION_SLACK_ICON_EMOJI".to_string(),
        "WATCHTOWER_NOTIFICATION_SLACK_ICON_URL".to_string(),
    ]);

    let ignored_flags = BTreeSet::from([
        "notification-gotify-url".to_string(),
        "notification-slack-icon-emoji".to_string(),
        "notification-slack-icon-url".to_string(),
    ]);

    let docs = load_doc_corpus();
    let command = WatchtowerCli::command();

    let mut missing = Vec::new();
    for arg in command.get_arguments() {
        if let Some(long) = arg.get_long() {
            if !ignored_flags.contains(long) && !docs.contains(&format!("--{long}")) {
                missing.push(format!("Docs does not mention flag long name {long:?}"));
            }
        }

        if let Some(short) = arg.get_short() {
            let short_flag = format!("-{short}");
            if !docs.contains(&short_flag) {
                missing.push(format!(
                    "Docs does not mention flag shorthand {short_flag:?} ({:?})",
                    arg.get_long()
                ));
            }
        }

        if let Some(env) = arg.get_env() {
            let env = env.to_string_lossy().into_owned();
            if !ignored_envs.contains(&env) && !docs.contains(&env) {
                missing.push(format!("Docs does not mention environment variable {env:?}"));
            }
        }
    }

    assert!(missing.is_empty(), "{}", missing.join("\n"));
}

fn parse_resolved_cli(args: &[&str]) -> WatchtowerConfig {
    let cli = WatchtowerCli::try_parse_from(args).expect("CLI should parse");
    let mut config: WatchtowerConfig = cli.try_into().expect("config should resolve");
    resolve_secret_references(&mut config).expect("secrets should resolve");
    config
}

fn load_doc_corpus() -> String {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let doc_files = [
        manifest_dir.join("docs/arguments.md"),
        manifest_dir.join("docs/lifecycle-hooks.md"),
        manifest_dir.join("docs/notifications.md"),
    ];

    let mut corpus = String::new();
    for path in doc_files {
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("Could not load docs file {:?}: {err}", path));
        corpus.push_str(&contents);
        corpus.push('\n');
    }

    corpus
}

fn write_temp_file(name: &str, content: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    path.push(format!("watchtower-rs-flags-{name}-{}-{stamp}.txt", std::process::id()));
    fs::write(&path, content).expect("temp file should be written");
    path
}

fn default_cli() -> WatchtowerCli {
    WatchtowerCli {
        docker: DockerArgs::default(),
        scheduling: SchedulingArgs::default(),
        update: UpdateArgs::default(),
        selection: SelectionArgs::default(),
        http_api: HttpApiArgs::default(),
        notifications: NotificationArgs {
            email: EmailNotificationArgs::default(),
            slack: SlackNotificationArgs::default(),
            msteams: TeamsNotificationArgs::default(),
            gotify: GotifyNotificationArgs::default(),
            ..NotificationArgs::default()
        },
        logging: LoggingArgs::default(),
        containers: Vec::new(),
    }
}

fn config_with_email_password(password: Option<String>) -> WatchtowerCli {
    let mut cli = default_cli();
    cli.notifications.email.password = password;
    cli
}
