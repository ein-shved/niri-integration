use niri_integration::{Launcher, Parser, error::Result};

fn main() -> Result<()> {
    let args = Launcher::parse();

    args.run()
}
