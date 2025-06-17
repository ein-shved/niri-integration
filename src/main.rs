use std::io;
use niri_integration::{Launcher, Parser};

fn main() -> io::Result<()> {
    let args = Launcher::parse();

    args.run()
}
