fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if let Err(m) = kpf_cli::run(&argv) {
        eprintln!("error: {m}");
        std::process::exit(1);
    }
}
