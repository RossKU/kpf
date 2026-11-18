//! `kpf-artifacts write` regenerates `contracts/artifacts/`; `kpf-artifacts check` compares a fresh compile with it.

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("write") => {
            for f in kpf_artifacts::write() {
                println!("wrote contracts/{f}");
            }
        }
        Some("check") => {
            let d = kpf_artifacts::check();
            if d.is_empty() {
                println!("pinned artifacts match a fresh compile");
            } else {
                for l in d {
                    eprintln!("{l}");
                }
                eprintln!("regenerate with: cargo run -p kpf-artifacts -- write");
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("usage: kpf-artifacts write | check");
            std::process::exit(2);
        }
    }
}
