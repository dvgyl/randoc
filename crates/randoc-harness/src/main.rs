//! The `randoc-harness` program. See `conformance/README.md`.

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    randoc_harness::cli_main(&args)
}
