//! The `randoc` program.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(randoc_cli::main_with_args(args));
}
