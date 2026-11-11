//! The `randoc` program.

fn main() {
    // Pandoc decodes each argument with replacement characters, so an
    // argument that is not UTF-8 is not an error.
    let args: Vec<String> = std::env::args_os()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    std::process::exit(randoc_cli::main_with_args(args));
}
