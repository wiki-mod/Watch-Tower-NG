mod cli;

use anyhow::Result;
use cli::{LogFormat, LoggingConfig, PollingMode, WatchtowerCli, WatchtowerConfig};
use watchtower_rs::AppConfig;

fn main() -> Result<()> {
    let cli = WatchtowerCli::try_parse_resolved()?;
    init_logging(&cli.logging);
    let config = AppConfig::from_cli(cli);
    watchtower_rs::run(config)?;
    Ok(())
}

fn init_logging(logging: &LoggingConfig) {
    let filter = tracing_subscriber::EnvFilter::new(logging_level(logging).to_string());
    let ansi_enabled = !logging.no_color;

    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_ansi(ansi_enabled);

    match logging.log_format {
        LogFormat::Pretty => builder.pretty().init(),
        LogFormat::Json | LogFormat::Logfmt | LogFormat::Auto => builder.compact().init(),
    }
}

fn logging_level(logging: &LoggingConfig) -> cli::LogLevel {
    if logging.trace {
        cli::LogLevel::Trace
    } else if logging.debug {
        cli::LogLevel::Debug
    } else {
        logging.log_level
    }
}

impl From<WatchtowerConfig> for AppConfig {
    fn from(config: WatchtowerConfig) -> Self {
        let WatchtowerConfig {
            scheduling,
            update,
            selection,
            http_api,
            notifications,
            logging,
            health_check,
            ..
        } = config;

        let (schedule, interval) = match scheduling.mode {
            PollingMode::Interval(duration) => (None, Some(duration)),
            PollingMode::Schedule(schedule) => (Some(schedule), None),
        };

        Self {
            run_once: update.run_once,
            monitor_only: update.monitor_only,
            cleanup: update.cleanup,
            remove_volumes: update.remove_volumes,
            include_stopped: update.include_stopped,
            revive_stopped: update.revive_stopped,
            include_restarting: update.include_restarting,
            rolling_restart: update.rolling_restart,
            schedule,
            interval,
            http_api_token: http_api.token,
            notification_types: notifications.types,
            enable_http_update_api: http_api.update,
            enable_http_metrics_api: http_api.metrics,
            unblock_http_api: scheduling.periodic_polls,
            scope: selection.scope,
            health_check,
            no_startup_message: logging.no_startup_message,
            trace_enabled: logging.trace,
        }
    }
}
