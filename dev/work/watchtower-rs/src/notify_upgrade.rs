#![forbid(unsafe_code)]

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clap::Parser;
use tokio::signal;
use tokio::time::sleep;

use crate::cli::{NotificationArgs, WatchtowerCli};
use watchtower_rs::cgroup;
use watchtower_rs::notifications::get_scheme;
use watchtower_rs::notifier::{
    new_notifier, EmailNotificationSettings, GotifyNotificationSettings, NotificationLogLevel as LegacyNotificationLogLevel,
    NotifierError, NotifierInput, SlackNotificationSettings, TeamsNotificationSettings,
};

const OUTPUT_COPY_NAME: &str = "./watchtower-notifications.env";
const OUTPUT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

pub async fn run(args: Vec<String>) -> Result<()> {
    if let Err(err) = run_notify_upgrade(args).await {
        logf(format!("Notification upgrade failed: {err}"));
    }
    Ok(())
}

async fn run_notify_upgrade(args: Vec<String>) -> Result<()> {
    let cli = WatchtowerCli::try_parse_from(
        std::iter::once("watchtower".to_string()).chain(args),
    )?;

    let urls = build_notification_urls(&cli.notifications)?;

    logf(format!(
        "Found notification configurations for: {}",
        join_notification_schemes(&urls)
    ));

    let out_file = create_temp_env_file()?;
    logf(format!("Writing notification URLs to {}", out_file.display()));
    logf("");

    write_notification_env(&out_file, &urls);

    log_copy_hint(&out_file);

    match wait_for_shutdown(OUTPUT_TIMEOUT).await? {
        ShutdownReason::TimedOut => logf("Timed out!"),
        ShutdownReason::Stopped => logf("Stopping..."),
    }

    if let Err(err) = fs::remove_file(&out_file) {
        logf(format!(
            "Failed to remove file, it may still be present in the container image! Error: {err}"
        ));
    } else {
        logf("Environment file has been removed.");
    }

    Ok(())
}

fn build_notification_urls(args: &NotificationArgs) -> Result<Vec<String>> {
    // The legacy command translated typed notification flags into shoutrrr URLs
    // before handing the final list to the upgrade flow.
    let input = NotifierInput {
        types: args
            .types
            .iter()
            .flat_map(|chunk| chunk.clone().into_inner())
            .collect(),
        level: match args.level {
            crate::cli::NotificationLogLevel::Panic => LegacyNotificationLogLevel::Panic,
            crate::cli::NotificationLogLevel::Fatal => LegacyNotificationLogLevel::Fatal,
            crate::cli::NotificationLogLevel::Error => LegacyNotificationLogLevel::Error,
            crate::cli::NotificationLogLevel::Warn => LegacyNotificationLogLevel::Warn,
            crate::cli::NotificationLogLevel::Info => LegacyNotificationLogLevel::Info,
            crate::cli::NotificationLogLevel::Debug => LegacyNotificationLogLevel::Debug,
            crate::cli::NotificationLogLevel::Trace => LegacyNotificationLogLevel::Trace,
        },
        delay: args.delay,
        hostname: args.hostname.clone(),
        urls: args
            .urls
            .iter()
            .flat_map(|chunk| chunk.clone().into_inner())
            .collect(),
        report: args.report,
        template: args.template.clone(),
        title_tag: args.title_tag.clone(),
        legacy_email_subject_tag: args.email.subject_tag.clone(),
        skip_title: args.skip_title,
        log_stdout: args.log_stdout,
        email: EmailNotificationSettings {
            from: args.email.from.clone(),
            to: args.email.to.clone(),
            server: args.email.server.clone(),
            user: args.email.user.clone(),
            password: args.email.password.clone(),
            port: args.email.port,
            tls_skip_verify: args.email.tls_skip_verify,
            delay: args.email.delay,
        },
        slack: SlackNotificationSettings {
            hook_url: args.slack.hook_url.clone(),
            identifier: args.slack.identifier.clone(),
            icon_emoji: args.slack.icon_emoji.clone(),
            icon_url: args.slack.icon_url.clone(),
        },
        msteams: TeamsNotificationSettings {
            hook: args.msteams.hook.clone(),
        },
        gotify: GotifyNotificationSettings {
            url: args.gotify.url.clone(),
            token: args.gotify.token.clone(),
            tls_skip_verify: args.gotify.tls_skip_verify,
        },
    };

    let setup = new_notifier(&input).map_err(notifier_blocker)?;
    Ok(setup.urls)
}

fn notifier_blocker(err: NotifierError) -> anyhow::Error {
    match err {
        NotifierError::InvalidNotificationLevel(level) => anyhow::anyhow!(
            "notify-upgrade notifier bridge rejected notification level `{level}`"
        ),
        NotifierError::UnknownNotificationType(notif_type) => anyhow::anyhow!(
            "notify-upgrade notifier bridge rejected notification type `{notif_type}`"
        ),
        NotifierError::NotificationUrl(source) => {
            anyhow::Error::new(source).context(
                "notify-upgrade notifier bridge failed to synthesize a notification URL",
            )
        }
    }
}

fn join_notification_schemes(urls: &[String]) -> String {
    // Legacy `GetNames()` returned the URL scheme for each configured service.
    urls.iter()
        .map(|url| get_scheme(url))
        .collect::<Vec<_>>()
        .join(", ")
}

fn create_temp_env_file() -> Result<PathBuf> {
    // The original command created the file directly under `/` so the copy hint
    // can be used from inside the running container without extra path mapping.
    let pid = process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?
        .as_nanos();

    for attempt in 0..64_u32 {
        let candidate = PathBuf::from(format!("/watchtower-notif-urls-{pid}-{nanos}-{attempt}"));

        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                drop(file);
                return Ok(candidate);
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err).with_context(|| "failed to create output file"),
        }
    }

    Err(anyhow::anyhow!("failed to create a unique temporary env file"))
}

fn write_notification_env(path: &Path, urls: &[String]) {
    // The legacy flow kept going after write/sync errors and only reported them.
    match OpenOptions::new().write(true).truncate(true).open(path) {
        Ok(mut file) => {
            if let Err(err) = file.write_all(render_env_content(urls).as_bytes()) {
                logf(format!("Failed to write to output file: {err}"));
            }
            if let Err(err) = file.sync_all() {
                logf(format!("Failed to sync output file: {err}"));
            }
            drop(file);
        }
        Err(err) => logf(format!("Failed to write to output file: {err}")),
    }
}

fn render_env_content(urls: &[String]) -> String {
    let mut content = String::from("WATCHTOWER_NOTIFICATION_URL=");

    for (index, url) in urls.iter().enumerate() {
        if index != 0 {
            content.push(' ');
        }
        content.push_str(url);
    }

    content
}

fn log_copy_hint(env_file: &Path) {
    // Mirror the old copy hint so the operator can retrieve the temp env file.
    let container_id = match cgroup::get_running_container_id() {
        Ok(Some(id)) => id.short_id(),
        Ok(None) => "<CONTAINER>".to_string(),
        Err(err) => {
            logf(format!("Failed to get running container ID: {err}"));
            "<CONTAINER>".to_string()
        }
    };

    logf("To get the environment file, use:");
    logf(format!(
        "cp {}:{} {}",
        container_id,
        env_file.display(),
        OUTPUT_COPY_NAME
    ));
    logf("");
    logf("Note: This file will be removed in 5 minutes or when this container is stopped!");
}

async fn wait_for_shutdown(timeout: Duration) -> Result<ShutdownReason> {
    // Keep the temp file alive until the process is interrupted or the timeout
    // expires, matching the old signal-and-timeout behavior closely.
    let timeout_sleep = sleep(timeout);
    tokio::pin!(timeout_sleep);

    #[cfg(unix)]
    {
        let mut terminate = signal::unix::signal(signal::unix::SignalKind::terminate())
            .context("failed to subscribe to SIGTERM")?;

        tokio::select! {
            _ = &mut timeout_sleep => Ok(ShutdownReason::TimedOut),
            _ = signal::ctrl_c() => Ok(ShutdownReason::Stopped),
            _ = terminate.recv() => Ok(ShutdownReason::Stopped),
        }
    }

    #[cfg(not(unix))]
    {
        tokio::select! {
            _ = &mut timeout_sleep => Ok(ShutdownReason::TimedOut),
            _ = signal::ctrl_c() => Ok(ShutdownReason::Stopped),
        }
    }
}

fn logf(message: impl Into<String>) {
    eprintln!("{}", message.into());
}

enum ShutdownReason {
    TimedOut,
    Stopped,
}
