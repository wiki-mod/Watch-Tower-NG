mod flags;
mod cli;

use anyhow::Result;

fn main() -> Result<()> {
    cli::execute()
}
