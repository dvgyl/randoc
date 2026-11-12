//! The template of a standalone run and the template variables that come
//! from the command line and the environment. The writer adds the variables
//! that depend on the document and renders the template.

use randoc::ast::{MetaValue, Pandoc};
use randoc::{Context, Error, LogMessage, Result};
use randoc_templates::{CompileError, Context as Variables, Template, Val};

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

/// Returns the default template of an output format: the file of the user
/// data folder, else the template of `randoc-data`. A format without a
/// default template (`json`, `native`) gives an empty template.
pub fn default_template(format: &str, data_dir: &str, context: &mut Context) -> TemplateText {
    let name = crate::info::template_name(format);
    // Pandoc does not look in the user data folder for a format that has no
    // default template.
    if randoc_data::default_template(format).is_none() {
        return TemplateText {
            path: name,
            text: String::new(),
            is_file: false,
        };
    }
    let text = context
        .host()
        .read_file(&crate::convert::data_path(data_dir, &name))
        .ok()
        .and_then(|bytes| String::from_utf8(crate::convert::clean_template(bytes)).ok())
        .or_else(|| {
            randoc_data::data_file(&name).map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        })
        .unwrap_or_default();
    TemplateText {
        path: name,
        text,
        is_file: false,
    }
}

/// Compiles a template. A partial `$name()$` is read next to a template
/// file, then as `templates/FILE` of the user data folder, then from
/// `randoc-data`.
pub fn compile(template: &TemplateText, data_dir: &str, context: &mut Context) -> Result<Template> {
    let max_depth = context.limits.max_nesting_depth;
    let mut load = |path: &str| -> Result<String> {
        let file = path.rsplit('/').next().unwrap_or(path);
        let data_path = format!("templates/{file}");
        let local = template
            .is_file
            .then(|| context.host().read_file(path).ok())
            .flatten();
        let bytes = local
            .or_else(|| {
                context
                    .host()
                    .read_file(&crate::convert::data_path(data_dir, &data_path))
                    .ok()
            })
            .or_else(|| randoc_data::data_file(&data_path).map(<[u8]>::to_vec))
            .ok_or_else(|| Error::CouldNotFindDataFile(data_path.clone()))?;
        String::from_utf8(crate::convert::clean_template(bytes))
            .map_err(|_| Error::CouldNotFindDataFile(data_path))
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
/// `pandoc-version` apply only when no `-V` sets them.
pub fn variables(
    settings: &Settings,
    inputs: &[String],
    includes: &Includes,
    curdir: &str,
) -> Variables {
    let mut vars = Variables::new();
    for (name, value) in &settings.variables {
        match value {
            VarValue::Text(text) => add(&mut vars, name, Val::text(text.as_str())),
            VarValue::Json(json) => {
                vars.insert(name.as_str(), json_val(json));
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
    vars
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

/// Converts a JSON value of `--variable-json` or a defaults file.
fn json_val(json: &serde_json::Value) -> Val {
    use serde_json::Value;
    match json {
        Value::Null => Val::Null,
        Value::Bool(b) => Val::Bool(*b),
        Value::Number(n) => Val::text(number_text(n)),
        Value::String(s) => Val::text(s.as_str()),
        Value::Array(items) => Val::List(items.iter().map(json_val).collect()),
        Value::Object(map) => {
            let mut vars = Variables::new();
            for (key, value) in map {
                vars.insert(key.as_str(), json_val(value));
            }
            Val::Map(vars)
        }
    }
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
        let vars = variables(&settings, &["x.md".to_owned()], includes, "/cwd");
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
        );
        assert_eq!(
            vars.get("sourcefile"),
            Some(&Val::List(vec![Val::text("m.md"), Val::text("abbr")]))
        );
        let vars = variables(&settings, &[], &none, "/c");
        assert_eq!(vars.get("sourcefile"), Some(&Val::text("-")));
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
        let vars = variables(&settings, &[], &Includes::default(), "/c");
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
            let mut vars = variables(&settings, &inputs, &Includes::default(), "/c");
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
        let template = compile(&file("sub/pt.tpl", source), "/u", &mut context).unwrap();
        assert_eq!(
            template.render(&Variables::new()).render(None),
            "p:SUBPART|Q|UP"
        );
        let mut context = context_with(&[
            ("sub/part.tpl", "SUBPART\n"),
            ("sub/sub/q.tpl", "Q\n"),
            ("/u/templates/part.tpl", "DDPART\n"),
        ]);
        let template = compile(&file("sub/pt.tpl", source), "/u", &mut context).unwrap();
        assert_eq!(
            template.render(&Variables::new()).render(None),
            "p:SUBPART|Q|DDPART"
        );
        let error =
            compile(&file("pt.tpl", "p:$part()$"), "/u", &mut context_with(&[])).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Could not find data file 'templates/part.tpl'"
        );
        assert_eq!(error.exit_code(), 97);
    }

    #[test]
    fn a_default_template_reads_partials_only_from_the_data_folders() {
        let mut context = context_with(&[("templates/common.latex", "LOCAL")]);
        let template = default_template("latex", "/u", &mut context);
        assert_eq!(template.path, "templates/default.latex");
        assert!(!template.is_file);
        assert!(compile(&template, "/u", &mut context).is_ok());
        // Pandoc 3.13: `-s -t native --data-dir=dd` ignores `dd/templates/default.native`.
        let mut context = context_with(&[
            ("/u/templates/default.plain", "USER $body$"),
            ("/u/templates/default.native", "$if(x)$"),
        ]);
        assert_eq!(
            default_template("plain", "/u", &mut context).text,
            "USER $body$"
        );
        assert_eq!(default_template("native", "/u", &mut context).text, "");
        let user = TemplateText {
            is_file: false,
            ..file("templates/default.latex", "$common()$")
        };
        let mut context = context_with(&[("templates/common.latex", "LOCAL")]);
        let rendered = compile(&user, "/u", &mut context)
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
            "/u",
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
