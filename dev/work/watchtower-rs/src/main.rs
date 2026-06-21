mod cli;
mod flags;

use anyhow::Result;

fn main() -> Result<()> {
    cli::execute()
}
