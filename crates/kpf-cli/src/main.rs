fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if let Err(m) = kpf_cli::run(&argv) {
        eprintln!("error: {m}");
        // Review 11: `oracle-watch` is an automated watcher's check; exit code 3 means "a claim must be contested now".
        std::process::exit(if m.starts_with(kpf_cli::ocmds::CONTEST_NOW) { 3 } else { 1 });
    }
}
