//! The command-line program of Randoc.
//!
//! This first version accepts the options that select the formats, the input
//! files and the output file. The command-line workstream replaces it with
//! the full Pandoc option set.

use std::io::{Read, Write};

use randoc::{
    Context, Error, FormatSpec, Output, Reader, ReaderOptions, Result, Source, Sources, Text,
    WriterOptions, formats,
};

/// The stack size of the conversion thread. Deep documents need a large stack.
/// The system gives memory to the stack only when the program uses it.
const CONVERSION_STACK_BYTES: usize = 1 << 30;

/// The nesting limit for the conversion thread. A debug build uses less than
/// 32 KB of stack for one level, so this depth fits in the stack.
const CLI_MAX_NESTING_DEPTH: usize = 16_384;

/// Runs the program with the given arguments and returns the exit code.
///
/// The first argument is the program name.
pub fn main_with_args(args: Vec<String>) -> i32 {
    let worker = std::thread::Builder::new()
        .stack_size(CONVERSION_STACK_BYTES)
        .spawn(move || run(&args));
    match worker.map(std::thread::JoinHandle::join) {
        Ok(Ok(code)) => code,
        // The thread did not start or it panicked. Pandoc uses 62 for an internal error.
        _ => 62,
    }
}

fn run(args: &[String]) -> i32 {
    match run_checked(args) {
        Ok(()) => 0,
        Err(error) => {
            // A failure to write the message must not change the exit code.
            let _ = writeln!(std::io::stderr(), "{error}");
            error.exit_code()
        }
    }
}

#[derive(Debug, Default, PartialEq)]
struct Invocation {
    from: Option<String>,
    to: Option<String>,
    output: Option<String>,
    inputs: Vec<String>,
    show_version: bool,
}

fn parse_args(args: &[String]) -> Result<Invocation> {
    let mut invocation = Invocation::default();
    let mut rest = args.iter().skip(1);
    let mut options_ended = false;
    while let Some(arg) = rest.next() {
        if options_ended || arg == "-" || !arg.starts_with('-') {
            invocation.inputs.push(arg.clone());
            continue;
        }
        if arg == "--" {
            options_ended = true;
            continue;
        }
        let (name, inline_value) = match arg.split_once('=') {
            Some((name, value)) if arg.starts_with("--") => (name, Some(value.to_owned())),
            _ => (arg.as_str(), None),
        };
        let mut value = |name: &str| -> Result<String> {
            inline_value.clone().or_else(|| rest.next().cloned()).ok_or_else(|| {
                Error::Option(format!(
                    "option `{name}' requires an argument\nTry pandoc --help for more information."
                ))
            })
        };
        match name {
            "-f" | "-r" | "--from" | "--read" => invocation.from = Some(value(name)?),
            "-t" | "-w" | "--to" | "--write" => invocation.to = Some(value(name)?),
            "-o" | "--output" => invocation.output = Some(value(name)?),
            "-v" | "--version" => invocation.show_version = true,
            _ => {
                return Err(Error::Option(format!(
                    "Unknown option {arg}.\nTry pandoc --help for more information."
                )));
            }
        }
    }
    Ok(invocation)
}

fn format_from_extension(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    match extension.as_str() {
        "json" => Some("json"),
        "md" | "markdown" | "txt" | "text" => Some("markdown"),
        "html" | "htm" | "xhtml" => Some("html"),
        "native" | "hs" => Some("native"),
        "tex" | "latex" | "ltx" => Some("latex"),
        _ => None,
    }
}

fn program_name(args: &[String]) -> &str {
    let name = args.first().map_or("randoc", String::as_str);
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

fn version_text(args: &[String]) -> String {
    let randoc_line = format!(
        "Randoc {} (compatible with Pandoc {})",
        randoc::RANDOC_VERSION,
        randoc::PANDOC_VERSION
    );
    if program_name(args) == "pandoc" {
        format!("pandoc {}\n{randoc_line}\n", randoc::PANDOC_VERSION)
    } else {
        format!("randoc {}\n{randoc_line}\n", randoc::RANDOC_VERSION)
    }
}

fn decode(name: &str, bytes: Vec<u8>) -> Result<Text> {
    let text = String::from_utf8(bytes).map_err(|error| {
        let offset = error.utf8_error().valid_up_to();
        Error::Utf8Decoding {
            source: Text::from(name),
            offset: offset as u64,
            byte: error.as_bytes().get(offset).copied().unwrap_or(0),
        }
    })?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    Ok(text.replace('\r', ""))
}

fn read_sources(inputs: &[String], context: &mut Context) -> Result<Sources> {
    if inputs.is_empty() || inputs == ["-"] {
        let mut bytes = Vec::new();
        std::io::stdin()
            .read_to_end(&mut bytes)
            .map_err(|error| Error::Io(format!("<stdin>: {error}")))?;
        return Ok(Sources(vec![Source {
            name: Text::new(),
            text: decode("<stdin>", bytes)?,
        }]));
    }
    let mut sources = Vec::new();
    for path in inputs {
        let bytes = context
            .host()
            .read_file(path)
            .map_err(|error| io_error(path, &error))?;
        sources.push(Source {
            name: path.clone(),
            text: decode(path, bytes)?,
        });
    }
    Ok(Sources(sources))
}

fn io_error(path: &str, error: &std::io::Error) -> Error {
    if error.kind() == std::io::ErrorKind::NotFound {
        return Error::Io(format!(
            "{path}: withBinaryFile: does not exist (No such file or directory)"
        ));
    }
    Error::Io(format!("{path}: {error}"))
}

fn run_checked(args: &[String]) -> Result<()> {
    let invocation = parse_args(args)?;
    if invocation.show_version {
        return write_stdout(version_text(args).as_bytes());
    }

    let from = invocation
        .from
        .clone()
        .or_else(|| {
            invocation
                .inputs
                .first()
                .and_then(|p| format_from_extension(p))
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "markdown".to_owned());
    let to = invocation
        .to
        .clone()
        .or_else(|| {
            invocation
                .output
                .as_deref()
                .and_then(format_from_extension)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "html".to_owned());

    let from_spec = FormatSpec::parse(&from);
    let to_spec = FormatSpec::parse(&to);
    let reader = formats::reader(&from_spec.name).ok_or(Error::UnknownReader(from.clone()))?;
    let writer = formats::writer(&to_spec.name).ok_or(Error::UnknownWriter(to.clone()))?;

    let mut reader_options = ReaderOptions::default();
    reader_options.extensions = from_spec.extensions()?;
    let mut writer_options = WriterOptions::default();
    writer_options.extensions = to_spec.extensions()?;

    let mut context = Context::native();
    context.limits.max_nesting_depth = CLI_MAX_NESTING_DEPTH;
    context.state.input_files = invocation.inputs.clone();
    context.state.output_file = invocation.output.clone();

    let doc = match reader {
        Reader::Text(reader) => {
            let sources = read_sources(&invocation.inputs, &mut context)?;
            reader.read(&sources, &reader_options, &mut context)?
        }
        Reader::Binary(reader) => {
            let path = invocation.inputs.first().map_or("-", String::as_str);
            let bytes = context
                .host()
                .read_file(path)
                .map_err(|error| io_error(path, &error))?;
            reader.read(&bytes, &reader_options, &mut context)?
        }
    };
    let bytes = match writer.write(&doc, &writer_options, &mut context)? {
        Output::Text(text) => text.into_bytes(),
        Output::Binary(bytes) => bytes,
    };
    match &invocation.output {
        Some(path) if path != "-" => context
            .host()
            .write_file(path, &bytes)
            .map_err(|error| Error::Io(format!("{path}: {error}"))),
        _ => write_stdout(&bytes),
    }
}

fn write_stdout(bytes: &[u8]) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(bytes)
        .and_then(|()| stdout.flush())
        .map_err(|error| Error::Io(format!("<stdout>: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn parses_short_long_and_inline_option_forms() {
        let parsed = parse_args(&args(&[
            "pandoc",
            "-f",
            "json",
            "--to=json",
            "-o",
            "out",
            "a",
            "b",
        ]))
        .unwrap();
        assert_eq!(
            parsed,
            Invocation {
                from: Some("json".into()),
                to: Some("json".into()),
                output: Some("out".into()),
                inputs: vec!["a".into(), "b".into()],
                show_version: false,
            }
        );
        let parsed = parse_args(&args(&[
            "pandoc", "--read", "json", "-w", "json", "--", "-x",
        ]))
        .unwrap();
        assert_eq!(parsed.from.as_deref(), Some("json"));
        assert_eq!(parsed.to.as_deref(), Some("json"));
        assert_eq!(parsed.inputs, ["-x"]);
        assert!(
            parse_args(&args(&["pandoc", "--version"]))
                .unwrap()
                .show_version
        );
        assert_eq!(parse_args(&args(&["pandoc", "-"])).unwrap().inputs, ["-"]);
    }

    #[test]
    fn rejects_unknown_options_and_missing_values_with_exit_code_6() {
        let error = parse_args(&args(&["pandoc", "--nosuch"])).unwrap_err();
        assert_eq!(error.exit_code(), 6);
        assert!(error.to_string().starts_with("Unknown option --nosuch."));
        let error = parse_args(&args(&["pandoc", "-f"])).unwrap_err();
        assert_eq!(error.exit_code(), 6);
    }

    #[test]
    fn version_names_pandoc_first_when_the_program_name_is_pandoc() {
        let text = version_text(&args(&["/usr/local/bin/pandoc"]));
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("pandoc 3.13"));
        assert!(lines.next().unwrap().starts_with("Randoc "));
        assert!(version_text(&args(&["randoc"])).starts_with("randoc "));
    }

    #[test]
    fn decode_removes_the_byte_order_mark_and_carriage_returns() {
        assert_eq!(
            decode("f", b"\xef\xbb\xbfa\r\nb\r\n".to_vec()).unwrap(),
            "a\nb\n"
        );
        let error = decode("f", b"ab\xff".to_vec()).unwrap_err();
        assert_eq!(
            error,
            Error::Utf8Decoding {
                source: "f".into(),
                offset: 2,
                byte: 0xff
            }
        );
    }

    #[test]
    fn guesses_formats_from_file_extensions() {
        assert_eq!(format_from_extension("a.JSON"), Some("json"));
        assert_eq!(format_from_extension("dir.d/a.md"), Some("markdown"));
        assert_eq!(format_from_extension("a.unknown"), None);
        assert_eq!(format_from_extension("noextension"), None);
        assert_eq!(program_name(&args(&["/a/b/pandoc"])), "pandoc");
        assert_eq!(program_name(&[]), "randoc");
    }

    #[test]
    fn a_missing_input_file_gives_the_pandoc_message() {
        // Pandoc 3.13: `pandoc nosuchfile.md` prints this text and exits with 1.
        let error = io_error(
            "nosuchfile.md",
            &std::io::Error::from(std::io::ErrorKind::NotFound),
        );
        assert_eq!(
            error.to_string(),
            "nosuchfile.md: withBinaryFile: does not exist (No such file or directory)"
        );
        assert_eq!(error.exit_code(), 1);
    }
}
