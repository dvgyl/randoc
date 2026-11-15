//! The template of a standalone run and the template variables that come
//! from the command line and the environment. The writer adds the variables
//! that depend on the document and renders the template.

use randoc::ast::{MetaValue, Pandoc};
use randoc::{Context, Error, LogMessage, Result};
use randoc_templates::{CompileError, Context as Variables, Template, Val};

use std::collections::VecDeque;

use crate::settings::{Settings, VarValue};

/// The version that `pandoc-version` gives.
const PANDOC_VERSION: &str = "3.13";

/// Where the text of a template comes from. Partials of a template file are
/// first searched next to the file; partials of a default template only in
/// the data folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateText {
    /// The path that error messages name and that partial paths start from.
    pub path: String,
    /// The text, without a byte order mark and carriage returns.
    pub text: String,
    /// True for a file that `--template` names.
    pub is_file: bool,
}

/// Where a run reads a template and its partials. Pandoc reads them with
/// full file access, so `--sandbox` does not limit these reads.
#[derive(Debug, Clone, Copy)]
pub struct TemplateFiles<'a> {
    /// True with `--sandbox`: the reads go past the host of the sandbox.
    pub native: bool,
    /// The user data folder.
    pub data_dir: &'a str,
    /// The resource path, for a template file and its partials.
    pub resource_path: &'a VecDeque<String>,
}

impl<'a> TemplateFiles<'a> {
    /// Returns where the run of `settings` reads templates.
    pub fn new(settings: &'a Settings, data_dir: &'a str) -> Self {
        TemplateFiles {
            native: settings.sandbox,
            data_dir,
            resource_path: &settings.resource_path,
        }
    }

    /// Reads a template file or a partial.
    pub fn read(&self, path: &str, context: &mut Context) -> std::io::Result<Vec<u8>> {
        if self.native {
            randoc::Host::read_file(&mut randoc::NativeHost, path)
        } else {
            context.host().read_file(path)
        }
    }
}

/// Returns the default template of an output format: the file of the user
/// data folder, else the template of `randoc-data`. A format without a
/// default template (`json`, `native`) gives an empty template. A file of
/// the user data folder that is not UTF-8 is an error; a missing file or a
/// folder gives the template of `randoc-data`.
pub fn default_template(
    format: &str,
    files: &TemplateFiles,
    context: &mut Context,
) -> Result<TemplateText> {
    let name = crate::info::template_name(format);
    // Pandoc names the template by the output format for partial paths and
    // syntax errors: `-t html` reads `default.html5` as `default.html`.
    let path = format!("templates/default.{format}");
    // Pandoc does not look in the user data folder for a format that has no
    // default template.
    if randoc_data::default_template(format).is_none() {
        return Ok(TemplateText {
            path,
            text: String::new(),
            is_file: false,
        });
    }
    let text = match files.read(&crate::convert::data_path(files.data_dir, &name), context) {
        Ok(bytes) => {
            crate::convert::check_utf8(&name, &bytes)?;
            String::from_utf8_lossy(&crate::convert::clean_template(bytes)).into_owned()
        }
        Err(_) => randoc_data::data_file(&name)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .unwrap_or_default(),
    };
    Ok(TemplateText {
        path,
        text,
        is_file: false,
    })
}

/// Compiles a template. A partial `$name()$` of a template file is read in
/// the folders of the resource path, then as `templates/FILE` of the user
/// data folder, then from `randoc-data`; a partial of a default template only
/// in the last two. A partial that is not UTF-8 is an error that names the
/// partial path.
pub fn compile(
    template: &TemplateText,
    files: &TemplateFiles,
    context: &mut Context,
) -> Result<Template> {
    let max_depth = context.limits.max_nesting_depth;
    let mut load = |path: &str| -> Result<String> {
        let file = path.rsplit('/').next().unwrap_or(path);
        let data_path = format!("templates/{file}");
        let mut candidates: Vec<(String, bool)> = if template.is_file {
            crate::convert::resource_candidates(path, files.resource_path)
                .into_iter()
                .map(|candidate| (candidate, true))
                .collect()
        } else {
            Vec::new()
        };
        candidates.push((crate::convert::data_path(files.data_dir, &data_path), false));
        for (candidate, is_local) in candidates {
            match files.read(&candidate, context) {
                Ok(bytes) => {
                    if is_local {
                        context.report(LogMessage::LoadedResource(path.to_owned(), candidate));
                    }
                    crate::convert::check_utf8(path, &bytes)?;
                    return Ok(
                        String::from_utf8_lossy(&crate::convert::clean_template(bytes))
                            .into_owned(),
                    );
                }
                Err(error) if is_local => unreadable_absolute_path(&candidate, &error)?,
                Err(_) => {}
            }
        }
        randoc_data::data_file(&data_path)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .ok_or(Error::CouldNotFindDataFile(data_path))
    };
    Template::compile(&template.path, &template.text, max_depth, &mut load).map_err(|error| {
        match error {
            CompileError::Syntax(text) => Error::Template(format!("'{text}'")),
            CompileError::Partial(error) => error,
            CompileError::TooDeep => Error::Template(format!(
                "'{}': the template is nested too deeply",
                template.path
            )),
        }
    })
}

/// Pandoc reads an absolute template path or partial path at once, so a
/// path that exists and cannot be read (a folder) is an error. A relative
/// path that cannot be read sends the search on.
pub fn unreadable_absolute_path(path: &str, error: &std::io::Error) -> Result<()> {
    if std::path::Path::new(path).is_absolute() && error.kind() != std::io::ErrorKind::NotFound {
        return Err(crate::convert::io_error(path, "withBinaryFile", error));
    }
    Ok(())
}

/// The include files of a run, as text, in the order of the command line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Includes {
    /// `--include-in-header`.
    pub header: Vec<String>,
    /// `--include-before-body`.
    pub before: Vec<String>,
    /// `--include-after-body`.
    pub after: Vec<String>,
}

/// Builds the template variables of the command line and the environment.
///
/// `-V` adds a value to the earlier values of the same name; `--variable-json`
/// and the `variables` key of a defaults file replace them. The options
/// `--css` and the include files add their values after the `-V` values.
/// `sourcefile` adds each input file; `outputfile`, `curdir` and
/// `pandoc-version` apply only when no `-V` sets them. A JSON value with
/// more than `max_depth` levels is an error.
pub fn variables(
    settings: &Settings,
    inputs: &[String],
    includes: &Includes,
    curdir: &str,
    max_depth: usize,
) -> Result<Variables> {
    let mut vars = Variables::new();
    for (name, value) in &settings.variables {
        match value {
            VarValue::Text(text) => add(&mut vars, name, Val::text(text.as_str())),
            VarValue::Json(json) => {
                let value = json_val(json, max_depth).ok_or_else(|| {
                    Error::App(format!(
                        "The value of the template variable '{name}' has more than {max_depth} levels."
                    ))
                })?;
                vars.insert(name.as_str(), value);
            }
        }
    }
    let lists: [(&str, &Vec<String>); 4] = [
        ("css", &settings.css),
        ("header-includes", &includes.header),
        ("include-before", &includes.before),
        ("include-after", &includes.after),
    ];
    for (name, values) in lists {
        for value in values {
            add(&mut vars, name, Val::text(value.as_str()));
        }
    }
    let sources: Vec<&str> = if inputs.is_empty() {
        vec!["-"]
    } else {
        inputs.iter().map(String::as_str).collect()
    };
    for source in sources {
        add(&mut vars, "sourcefile", Val::text(source));
    }
    let output = settings.output_file.as_deref().unwrap_or("-");
    for (name, value) in [
        ("outputfile", output),
        ("curdir", curdir),
        ("pandoc-version", PANDOC_VERSION),
    ] {
        if vars.get(name).is_none() {
            vars.insert(name, Val::text(value));
        }
    }
    Ok(vars)
}

/// Adds a value to a variable: the first value stays a single value, more
/// values make a list.
fn add(vars: &mut Variables, name: &str, value: Val) {
    let new = match vars.0.remove(name) {
        None | Some(Val::Null) => value,
        Some(Val::List(mut items)) => {
            items.push(value);
            Val::List(items)
        }
        Some(old) => Val::List(vec![old, value]),
    };
    vars.insert(name, new);
}

/// Converts a JSON value of `--variable-json` or a defaults file, or gives
/// `None` for a value with more than `depth` levels of lists and maps.
fn json_val(json: &serde_json::Value, depth: usize) -> Option<Val> {
    use serde_json::Value;
    Some(match json {
        Value::Null => Val::Null,
        Value::Bool(b) => Val::Bool(*b),
        Value::Number(n) => Val::text(number_text(n)),
        Value::String(s) => Val::text(s.as_str()),
        Value::Array(items) => {
            let depth = depth.checked_sub(1)?;
            Val::List(
                items
                    .iter()
                    .map(|item| json_val(item, depth))
                    .collect::<Option<_>>()?,
            )
        }
        Value::Object(map) => {
            let depth = depth.checked_sub(1)?;
            let mut vars = Variables::new();
            for (key, value) in map {
                vars.insert(key.as_str(), json_val(value, depth)?);
            }
            Val::Map(vars)
        }
    })
}

/// The text of a JSON number: an integer without a fraction.
fn number_text(n: &serde_json::Number) -> String {
    match n.as_f64() {
        Some(f) if n.is_f64() && f.fract() == 0.0 && f.abs() < 1e15 => format!("{f:.0}"),
        _ => n.to_string(),
    }
}

/// Sets `pagetitle` for an HTML format when the document has no title and
/// no `pagetitle`, from the first `sourcefile` without folder and extension.
pub fn page_title_fallback(vars: &mut Variables, doc: &Pandoc, context: &mut Context) {
    let has_meta = |key: &str| {
        doc.meta
            .0
            .get(key)
            .is_some_and(|value| !matches!(value, MetaValue::MetaString(s) if s.is_empty()))
    };
    if vars.get("pagetitle").is_some() || has_meta("title") || has_meta("pagetitle") {
        return;
    }
    let first = match vars.get("sourcefile") {
        Some(Val::List(items)) => items.first().cloned(),
        other => other.cloned(),
    };
    let source = first.map(|v| val_text(&v)).unwrap_or_default();
    let fallback = base_name(&source);
    context.report(LogMessage::NoTitleElement(fallback.clone()));
    vars.insert("pagetitle", Val::text(fallback));
}

fn val_text(val: &Val) -> String {
    match val {
        Val::Simple(doc) => doc.render(None),
        _ => String::new(),
    }
}

/// The file name of a path without its extension, as Haskell's `takeBaseName`.
fn base_name(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.rfind('.')
        .map_or(file, |index| &file[..index])
        .to_owned()
}

#[cfg(test)]
mod tests {
    use randoc::PureHost;

    use super::*;

    /// Renders `source` with the variables of a command line, as the writer
    /// renders the template of `WriterOptions`.
    fn render(source: &str, args: &[&str], includes: &Includes) -> String {
        let settings = settings(args);
        let vars = variables(&settings, &["x.md".to_owned()], includes, "/cwd", 128).unwrap();
        let mut none = |_: &str| -> std::result::Result<String, ()> { Err(()) };
        let template = Template::compile("t", source, 128, &mut none).unwrap();
        template.render(&vars).render(None)
    }

    struct NoDefaults;

    impl crate::cli::Environment for NoDefaults {
        fn warn(&mut self, _: &str) {}
        fn apply_defaults(&mut self, _: &mut Settings, _: &str) -> Result<()> {
            Ok(())
        }
    }

    fn settings(args: &[&str]) -> Settings {
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        crate::settings_from_args(&args, &mut NoDefaults).unwrap().0
    }

    const LIST: &str = "$for(x)$<$x$>$endfor$";

    // Expected values: Pandoc 3.13 with `-t plain --template=t.tpl`.
    #[test]
    fn variable_options_add_or_replace_values() {
        let none = Includes::default();
        let cases: &[(&[&str], &str)] = &[
            (&["-V", "x=1", "-V", "x=2"], "<1><2>"),
            (&["-V", "x=1", "--variable-json", "x=2"], "<2>"),
            (&["--variable-json", "x=1", "-V", "x=2"], "<1><2>"),
            (&["--variable-json", "x:[2,3]"], "<2><3>"),
            (&["--variable-json", "x=[2,3]"], "<2><3>"),
            (
                &[
                    "-V",
                    "x=1",
                    "-V",
                    "x=2",
                    "--variable-json",
                    "x=[3]",
                    "-V",
                    "x=4",
                ],
                "<3><4>",
            ),
            (&["-V", "x=1", "-V", "x"], "<1><true>"),
            (&["-V", "x="], "<>"),
            (&["-V", "x:a=b"], "<a=b>"),
            (&["--variable-json", "x"], "<true>"),
            (&["--variable-json", "x=null"], ""),
            (&["--variable-json", "x=1.50"], "<1.5>"),
            (&["--variable-json", "x=false"], "<false>"),
            (&["--variable-json", "x=[[1,2],3]"], "<12><3>"),
            (&["--variable-json", "x={\"a\":1}"], "<true>"),
        ];
        for (args, expected) in cases {
            assert_eq!(render(LIST, args, &none), *expected, "{args:?}");
        }
    }

    #[test]
    fn option_values_follow_the_variables() {
        let includes = Includes {
            header: vec!["H".to_owned()],
            before: vec!["B".to_owned()],
            after: vec!["A".to_owned()],
        };
        let source = "css=[$for(css)$$css$;$endfor$] h=[$for(header-includes)$$header-includes$;$endfor$] \
                      b=[$for(include-before)$$include-before$;$endfor$] \
                      a=[$for(include-after)$$include-after$;$endfor$] \
                      tp=[$for(title-prefix)$$title-prefix$;$endfor$]";
        assert_eq!(
            render(
                source,
                &[
                    "--css=a.css",
                    "-V",
                    "css=z",
                    "-V",
                    "header-includes=V",
                    "-V",
                    "include-before=V",
                    "-V",
                    "include-after=V",
                    "--title-prefix=O",
                    "-V",
                    "title-prefix=V"
                ],
                &includes
            ),
            "css=[z;a.css;] h=[V;H;] b=[V;B;] a=[V;A;] tp=[O;V;]"
        );
        assert_eq!(
            render(
                source,
                &["-V", "title-prefix=V", "--title-prefix=O"],
                &Includes::default()
            ),
            "css=[] h=[] b=[] a=[] tp=[V;O;]"
        );
    }

    #[test]
    fn environment_variables_yield_to_the_options() {
        let none = Includes::default();
        let source = "s=[$for(sourcefile)$$sourcefile$;$endfor$] o=$outputfile$ c=$curdir$ v=$pandoc-version$";
        assert_eq!(render(source, &[], &none), "s=[x.md;] o=- c=/cwd v=3.13");
        assert_eq!(
            render(source, &["-o", "out.txt"], &none),
            "s=[x.md;] o=out.txt c=/cwd v=3.13"
        );
        assert_eq!(
            render(
                source,
                &[
                    "-V",
                    "sourcefile=SF",
                    "-V",
                    "outputfile=OF",
                    "-V",
                    "curdir=CD",
                    "-V",
                    "pandoc-version=PV"
                ],
                &none
            ),
            "s=[SF;x.md;] o=OF c=CD v=PV"
        );
        let settings = settings(&[]);
        let vars = variables(
            &settings,
            &["m.md".to_owned(), "abbr".to_owned()],
            &none,
            "/c",
            128,
        )
        .unwrap();
        assert_eq!(
            vars.get("sourcefile"),
            Some(&Val::List(vec![Val::text("m.md"), Val::text("abbr")]))
        );
        let vars = variables(&settings, &[], &none, "/c", 128).unwrap();
        assert_eq!(vars.get("sourcefile"), Some(&Val::text("-")));
    }

    /// Nests `depth` lists or maps around a string, without recursion.
    fn nested_json(depth: usize, map: bool) -> serde_json::Value {
        let mut value = serde_json::Value::String("x".to_owned());
        for _ in 0..depth {
            value = if map {
                serde_json::Value::Object(serde_json::Map::from_iter([("k".to_owned(), value)]))
            } else {
                serde_json::Value::Array(vec![value])
            };
        }
        value
    }

    /// Takes a nested value apart without recursion, so that its drop is safe.
    fn take_apart(value: serde_json::Value) {
        let mut stack = vec![value];
        while let Some(value) = stack.pop() {
            match value {
                serde_json::Value::Array(items) => stack.extend(items),
                serde_json::Value::Object(map) => stack.extend(map.into_iter().map(|(_, v)| v)),
                _ => {}
            }
        }
    }

    // Review cli-R9-01: the conversion of a deep value overflowed the stack.
    #[test]
    fn deep_json_variables_give_the_depth_error_on_a_small_stack() {
        let run = || {
            for map in [false, true] {
                for (depth, ok) in [(127, true), (128, true), (10_000, false), (100_000, false)] {
                    let mut settings = Settings::default();
                    settings
                        .variables
                        .push(("x".to_owned(), VarValue::Json(nested_json(depth, map))));
                    let result = variables(&settings, &[], &Includes::default(), "/c", 128);
                    assert_eq!(result.is_ok(), ok, "{depth} {map}");
                    if let Err(error) = result {
                        assert_eq!(
                            error.to_string(),
                            "The value of the template variable 'x' has more than 128 levels."
                        );
                    }
                    for (_, value) in std::mem::take(&mut settings.variables) {
                        if let VarValue::Json(json) = value {
                            take_apart(json);
                        }
                    }
                }
            }
        };
        std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(run)
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn a_defaults_file_replaces_the_variables() {
        let mut settings = settings(&["-V", "x=1"]);
        settings.variables.push((
            "x".to_owned(),
            VarValue::Json(serde_json::json!(["d1", "d2"])),
        ));
        settings
            .variables
            .push(("x".to_owned(), VarValue::Text("1".to_owned())));
        let vars = variables(&settings, &[], &Includes::default(), "/c", 128).unwrap();
        let mut none = |_: &str| -> std::result::Result<String, ()> { Err(()) };
        let template = Template::compile("t", LIST, 128, &mut none).unwrap();
        assert_eq!(template.render(&vars).render(None), "<d1><d2><1>");
    }

    // Pandoc 3.13: `-s -t html` gives `<title>x</title>` for `x.md`, `-` for
    // standard input, `a.b` for `./a.b.md`, and the document title when it has one.
    #[test]
    fn html_output_without_a_title_takes_the_source_file_name() {
        let title = |sources: &[&str], meta: Option<&str>, args: &[&str]| {
            let settings = settings(args);
            let inputs: Vec<String> = sources.iter().map(|s| (*s).to_owned()).collect();
            let mut vars = variables(&settings, &inputs, &Includes::default(), "/c", 128).unwrap();
            let mut doc = Pandoc::default();
            if let Some(text) = meta {
                doc.meta
                    .0
                    .insert("title".to_owned(), MetaValue::MetaString(text.to_owned()));
            }
            let mut context = Context::pure();
            page_title_fallback(&mut vars, &doc, &mut context);
            (vars.get("pagetitle").cloned(), context.log().to_vec())
        };
        let (value, log) = title(&["x.md"], None, &[]);
        assert_eq!(value, Some(Val::text("x")));
        assert_eq!(log, [LogMessage::NoTitleElement("x".to_owned())]);
        assert_eq!(title(&[], None, &[]).0, Some(Val::text("-")));
        assert_eq!(title(&["./a.b.md"], None, &[]).0, Some(Val::text("a.b")));
        assert_eq!(title(&["x.md", "abbr"], None, &[]).0, Some(Val::text("x")));
        assert_eq!(
            title(&["x.md"], None, &["-V", "sourcefile="]).0,
            Some(Val::text(""))
        );
        let (value, log) = title(&["x.md"], Some("T"), &[]);
        assert_eq!((value, log.len()), (None, 0));
        assert_eq!(
            title(&["x.md"], None, &["-V", "pagetitle=PV"]).0,
            Some(Val::text("PV"))
        );
    }

    fn context_with(files: &[(&str, &str)]) -> Context {
        let mut host = PureHost::default();
        for (path, text) in files {
            host.files
                .insert((*path).to_owned(), text.as_bytes().to_vec());
        }
        Context::new(Box::new(host))
    }

    static RESOURCE_PATH: std::sync::LazyLock<VecDeque<String>> =
        std::sync::LazyLock::new(|| VecDeque::from([".".to_owned()]));

    fn files() -> TemplateFiles<'static> {
        TemplateFiles {
            native: false,
            data_dir: "/u",
            resource_path: &RESOURCE_PATH,
        }
    }

    fn file(path: &str, text: &str) -> TemplateText {
        TemplateText {
            path: path.to_owned(),
            text: text.to_owned(),
            is_file: true,
        }
    }

    // Pandoc 3.13: `--template=sub/pt.tpl` with `$part()$|$sub/q()$|$../part()$`
    // gives `SUBPART|Q|UP`, and the data folder when the file next to the
    // template is missing; a missing partial gives exit 97.
    #[test]
    fn partials_are_read_next_to_the_template_then_in_the_data_folders() {
        let source = "p:$part()$|$sub/q()$|$../part()$";
        let mut context = context_with(&[
            ("sub/part.tpl", "SUBPART\n"),
            ("sub/sub/q.tpl", "Q\n"),
            ("sub/../part.tpl", "UP\n"),
        ]);
        let template = compile(&file("sub/pt.tpl", source), &files(), &mut context).unwrap();
        assert_eq!(
            template.render(&Variables::new()).render(None),
            "p:SUBPART|Q|UP"
        );
        let mut context = context_with(&[
            ("sub/part.tpl", "SUBPART\n"),
            ("sub/sub/q.tpl", "Q\n"),
            ("/u/templates/part.tpl", "DDPART\n"),
        ]);
        let template = compile(&file("sub/pt.tpl", source), &files(), &mut context).unwrap();
        assert_eq!(
            template.render(&Variables::new()).render(None),
            "p:SUBPART|Q|DDPART"
        );
        let error = compile(
            &file("pt.tpl", "p:$part()$"),
            &files(),
            &mut context_with(&[]),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Could not find data file 'templates/part.tpl'"
        );
        assert_eq!(error.exit_code(), 97);
    }

    #[test]
    fn a_default_template_reads_partials_only_from_the_data_folders() {
        let mut context = context_with(&[("templates/common.latex", "LOCAL")]);
        let template = default_template("latex", &files(), &mut context).unwrap();
        assert_eq!(template.path, "templates/default.latex");
        assert!(!template.is_file);
        assert!(compile(&template, &files(), &mut context).is_ok());
        // Pandoc 3.13: `-s -t native --data-dir=dd` ignores `dd/templates/default.native`.
        let mut context = context_with(&[
            ("/u/templates/default.plain", "USER $body$"),
            ("/u/templates/default.native", "$if(x)$"),
        ]);
        assert_eq!(
            default_template("plain", &files(), &mut context)
                .unwrap()
                .text,
            "USER $body$"
        );
        assert_eq!(
            default_template("native", &files(), &mut context)
                .unwrap()
                .text,
            ""
        );
        let user = TemplateText {
            is_file: false,
            ..file("templates/default.latex", "$common()$")
        };
        let mut context = context_with(&[("templates/common.latex", "LOCAL")]);
        let rendered = compile(&user, &files(), &mut context)
            .unwrap()
            .render(&Variables::new())
            .render(None);
        assert_ne!(rendered, "LOCAL");
    }

    // Pandoc 3.13: `--template=bad.tpl` with `a $if(x)$ b` gives exit 5.
    #[test]
    fn a_syntax_error_names_the_template() {
        let error = compile(
            &file("bad.tpl", "a $if(x)$ b\n"),
            &files(),
            &mut context_with(&[]),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Error compiling template '\"bad.tpl\" (line 2, column 1):\nunexpected end of input\n\
             expecting \" \", \"\\t\", \"$--\", \"\\n\", \"\\r\\n\", \"\\r\", \"$\", \"${\" or \"$$\"'"
        );
        assert_eq!(error.exit_code(), 5);
    }
}
