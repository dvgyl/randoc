//! Reads one JSON request on each input line and writes one JSON result line:
//! the output (with the final newline that the program adds), the warnings
//! and the error. A script can compare the writer with the oracle on a
//! Pandoc JSON tree without a build of the whole program.
//!
//! A request is `{"doc": TREE, "fmt": "gfm", "opts": {...}}`. The options
//! are `wrap`, `columns`, `tab_stop`, `refloc`, `ascii`, `toc`, `standalone`,
//! `reference_links`, `setext` and `id_prefix`.

use std::io::{BufRead, Write};

use randoc_core::{Context, FormatSpec, LogMessage, ReferenceLocation, WrapOption, WriterOptions};
use serde_json::{Value, json};

fn options(fmt: &str, opts: &Value) -> Result<WriterOptions, String> {
    let spec = FormatSpec::parse(fmt);
    let mut options = WriterOptions::default();
    options.extensions = spec.extensions().map_err(|e| e.to_string())?;
    let flag = |key: &str| opts.get(key).and_then(Value::as_bool).unwrap_or(false);
    if let Some(wrap) = opts.get("wrap").and_then(Value::as_str) {
        options.wrap_text = match wrap {
            "none" => WrapOption::WrapNone,
            "preserve" => WrapOption::WrapPreserve,
            _ => WrapOption::WrapAuto,
        };
    }
    if let Some(columns) = opts.get("columns").and_then(Value::as_i64) {
        options.columns = columns;
    }
    if let Some(tab_stop) = opts.get("tab_stop").and_then(Value::as_i64) {
        options.tab_stop = tab_stop;
    }
    if let Some(location) = opts.get("refloc").and_then(Value::as_str) {
        options.reference_location = match location {
            "block" => ReferenceLocation::EndOfBlock,
            "section" => ReferenceLocation::EndOfSection,
            _ => ReferenceLocation::EndOfDocument,
        };
    }
    if let Some(prefix) = opts.get("id_prefix").and_then(Value::as_str) {
        options.identifier_prefix = prefix.to_owned();
    }
    options.prefer_ascii = flag("ascii");
    options.table_of_contents = flag("toc");
    options.reference_links = flag("reference_links");
    options.setext_headers = flag("setext");
    if flag("standalone") {
        let name = spec.name.as_str();
        let source = randoc_data::default_template(name).ok_or("no default template")?;
        let template =
            randoc_templates::Template::compile(name, source, 64, &mut |_| Err("no partials"))
                .map_err(|e| e.to_string())?;
        options.template = Some(template);
    }
    Ok(options)
}

fn run(line: &str) -> Value {
    let request: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(error) => return json!({ "error": error.to_string() }),
    };
    let fmt = request["fmt"].as_str().unwrap_or("markdown");
    let doc = match randoc_ast::json::from_json_with_limit(&request["doc"].to_string(), 1 << 20) {
        Ok(doc) => doc,
        Err(error) => return json!({ "error": error.to_string() }),
    };
    let options = match options(fmt, &request["opts"]) {
        Ok(options) => options,
        Err(error) => return json!({ "error": error }),
    };
    let name = FormatSpec::parse(fmt).name;
    let Some(writer) = randoc_markdown_writer::writer(&name) else {
        return json!({ "error": "unknown format" });
    };
    let mut context = Context::pure();
    context.limits.max_nesting_depth = 16_384;
    let output = writer.write(&doc, &options, &mut context);
    let warnings: Vec<String> = context.log().iter().map(LogMessage::render).collect();
    match output {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.into_bytes()).into_owned();
            if !text.ends_with('\n') {
                text.push('\n');
            }
            json!({ "out": text, "err": warnings.concat() })
        }
        Err(error) => json!({ "error": error.to_string(), "err": warnings.concat() }),
    }
}

fn main() {
    // The program runs the writer on a large stack too.
    let worker = std::thread::Builder::new().stack_size(1 << 30).spawn(|| {
        let stdin = std::io::stdin();
        let mut stdout = std::io::stdout().lock();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            let result = run(&line);
            if writeln!(stdout, "{result}")
                .and_then(|()| stdout.flush())
                .is_err()
            {
                break;
            }
        }
    });
    if let Ok(handle) = worker {
        let _ = handle.join();
    }
}
