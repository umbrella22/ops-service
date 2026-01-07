fn print_version() {
    println!("ops-runner {}", env!("CARGO_PKG_VERSION"));
}

fn main() -> anyhow::Result<()> {
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        print_version();
        return Ok(());
    }

    println!("ops-runner");
    Ok(())
}
