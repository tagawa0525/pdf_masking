use pdf_masking::error::Result;
use std::env;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: pdf_masking <jobs.yaml>...");
        std::process::exit(1);
    }

    // Phase 12: CLI logic to be implemented
    Ok(())
}
