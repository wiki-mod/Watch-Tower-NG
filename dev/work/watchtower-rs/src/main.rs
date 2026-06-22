#![forbid(unsafe_code)]

fn main() -> anyhow::Result<()> {
    watchtower_rs::cli::execute()
}
