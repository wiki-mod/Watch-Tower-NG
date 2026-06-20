use watchtower_rs::AppConfig;

fn main() -> watchtower_rs::Result<()> {
    init_logging();
    watchtower_rs::run(AppConfig::default())
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
