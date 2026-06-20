use std::time::Duration;

use clap::Parser;
use watchtower_rs::AppConfig;

fn main() -> watchtower_rs::Result<()> {
    init_logging();
    let config = AppConfig::from_cli(Cli::parse().into_app_config());
    watchtower_rs::run(config)
}

fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();
}

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    #[arg(long, env)]
    run_once: bool,
    #[arg(long, env)]
    monitor_only: bool,
    #[arg(long, env)]
    cleanup: bool,
    #[arg(long, env)]
    remove_volumes: bool,
    #[arg(long, env)]
    include_stopped: bool,
    #[arg(long, env)]
    revive_stopped: bool,
    #[arg(long, env)]
    include_restarting: bool,
    #[arg(long, env)]
    rolling_restart: bool,
    #[arg(long, env)]
    schedule: Option<String>,
    #[arg(long, env, value_name = "SECONDS")]
    interval: Option<u64>,
    #[arg(long, env)]
    http_api_token: Option<String>,
    #[arg(long, env)]
    enable_http_update_api: bool,
    #[arg(long, env)]
    enable_http_metrics_api: bool,
    #[arg(long, env)]
    unblock_http_api: bool,
    #[arg(long, env)]
    scope: Option<String>,
    #[arg(long, env)]
    health_check: bool,
}

impl Cli {
    fn into_app_config(self) -> AppConfig {
        AppConfig {
            run_once: self.run_once,
            monitor_only: self.monitor_only,
            cleanup: self.cleanup,
            remove_volumes: self.remove_volumes,
            include_stopped: self.include_stopped,
            revive_stopped: self.revive_stopped,
            include_restarting: self.include_restarting,
            rolling_restart: self.rolling_restart,
            schedule: self.schedule,
            interval: self.interval.map(Duration::from_secs),
            http_api_token: self.http_api_token,
            enable_http_update_api: self.enable_http_update_api,
            enable_http_metrics_api: self.enable_http_metrics_api,
            unblock_http_api: self.unblock_http_api,
            scope: self.scope,
            health_check: self.health_check,
        }
    }
}
