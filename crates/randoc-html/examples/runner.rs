//! Reads one JSON request on each input line and writes one JSON result line:
//! the HTML output (with the final newline that the program adds), the
//! warnings and the error. `tools/differential.py` uses it to compare the
//! writer with the oracle for options that the program does not accept yet.
//!
//! A request is `{"doc": TREE, "fmt": "html", "opts": {...}}`. The option
//! names are those of `tools/differential.py`.

use std::io::{BufRead, Write};

use randoc_core::{
    Context, FormatSpec, HtmlMathMethod, LogMessage, ObfuscationMethod, ReferenceLocation,
    WrapOption, WriterOptions,
};
use serde_json::{Value, json};

fn options(fmt: &str, opts: &Value) -> Result<WriterOptions, String> {
    let spec = FormatSpec::parse(fmt);
    let mut options = WriterOptions::default();
    options.extensions = spec.extensions().map_err(|e| e.to_string())?;
    let get = |key: &str| opts.get(key);
    if let Some(wrap) = get("wrap").and_then(Value::as_str) {
        options.wrap_text = match wrap {
            "none" => WrapOption::WrapNone,
            "preserve" => WrapOption::WrapPreserve,
            _ => WrapOption::WrapAuto,
        };
    }
    if let Some(columns) = get("columns").and_then(Value::as_i64) {
        options.columns = columns;
    }
    options.section_divs = get("section_divs")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    options.number_sections = get("number_sections")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if let Some(offsets) = get("number_offset").and_then(Value::as_array) {
        options.number_offset = offsets.iter().filter_map(Value::as_i64).collect();
        options.number_sections = true;
    }
    if let Some(prefix) = get("id_prefix").and_then(Value::as_str) {
        options.identifier_prefix = prefix.to_owned();
    }
    options.html_q_tags = get("q_tags").and_then(Value::as_bool).unwrap_or(false);
    options.prefer_ascii = get("ascii").and_then(Value::as_bool).unwrap_or(false);
    options.incremental = get("incremental").and_then(Value::as_bool).unwrap_or(false);
    if get("email").and_then(Value::as_str) == Some("references") {
        options.email_obfuscation = ObfuscationMethod::ReferenceObfuscation;
    }
    if let Some(location) = get("refloc").and_then(Value::as_str) {
        options.reference_location = match location {
            "block" => ReferenceLocation::EndOfBlock,
            "section" => ReferenceLocation::EndOfSection,
            _ => ReferenceLocation::EndOfDocument,
        };
    }
    if let Some(math) = get("math").and_then(Value::as_str) {
        options.html_math_method = match math {
            "mathjax" => HtmlMathMethod::MathJax(String::new()),
            "katex" => HtmlMathMethod::KaTex(String::new()),
            "gladtex" => HtmlMathMethod::GladTex,
            "plain" => HtmlMathMethod::PlainMath,
            "webtex" => HtmlMathMethod::WebTex("https://latex.codecogs.com/png.latex?".into()),
            _ => HtmlMathMethod::MathMl,
        };
    }
    Ok(options)
}

fn run(line: &str) -> Value {
    let request: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(error) => return json!({ "error": error.to_string() }),
    };
    let fmt = request["fmt"].as_str().unwrap_or("html");
    let doc = match randoc_ast::json::from_json_with_limit(&request["doc"].to_string(), 1 << 20) {
        Ok(doc) => doc,
        Err(error) => return json!({ "error": error.to_string() }),
    };
    let options = match options(fmt, &request["opts"]) {
        Ok(options) => options,
        Err(error) => return json!({ "error": error }),
    };
    let name = FormatSpec::parse(fmt).name;
    let Some(writer) = randoc_html::writer(&name) else {
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
