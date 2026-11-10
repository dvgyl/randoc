//! Tests: the ported `doctemplates` tests and the oracle cases.

use std::collections::BTreeMap;

use randoc_layout::{Doc, hsep, literal};
use serde_json::Value;

use super::*;

const DEPTH: usize = 128;

/// Haskell `show` for a `Double`.
fn show_double(x: f64) -> String {
    if x == 0.0 {
        return "0.0".to_owned();
    }
    let sci = format!("{:e}", x.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let e: i32 = exp.parse::<i32>().unwrap_or(0) + 1;
    let sign = if x < 0.0 { "-" } else { "" };
    let body = if (0.1..1e7).contains(&x.abs()) {
        let e = usize::try_from(e).unwrap_or(0);
        let padded = format!("{digits:0<e$}");
        let (int, frac) = padded.split_at(e);
        let int = if int.is_empty() { "0" } else { int };
        let frac = if frac.is_empty() { "0" } else { frac };
        format!("{int}.{frac}")
    } else {
        let (first, rest) = digits.split_at(1);
        let rest = if rest.is_empty() { "0" } else { rest };
        format!("{first}.{rest}e{}", e - 1)
    };
    format!("{sign}{body}")
}

/// The `FromJSON` instance of `Val` in `doctemplates`, with two oracle
/// rules: `{"@words": "a b"}` is a Doc of words with breakable spaces, and
/// an empty array is an empty map, as the Lua module reads it.
fn json_val(v: &Value) -> Val {
    json_val_with(v, false)
}

fn lua_val(v: &Value) -> Val {
    json_val_with(v, true)
}

fn json_val_with(v: &Value, lua: bool) -> Val {
    match v {
        Value::Null => Val::Null,
        Value::Bool(b) => Val::Bool(*b),
        Value::Number(n) => Val::text(match n.as_i64() {
            Some(i) => i.to_string(),
            None => show_double(n.as_f64().unwrap_or(0.0)),
        }),
        Value::String(s) => Val::text(s.as_str()),
        Value::Array(xs) if lua && xs.is_empty() => Val::Map(Context::default()),
        Value::Array(xs) => Val::List(xs.iter().map(|x| json_val_with(x, lua)).collect()),
        Value::Object(m) => match m.get("@words") {
            Some(Value::String(words)) => Val::Simple(hsep(words.split_whitespace().map(literal))),
            _ => Val::Map(Context(
                m.iter()
                    .map(|(k, x)| (k.clone(), json_val_with(x, lua)))
                    .collect(),
            )),
        },
    }
}

fn json_context(v: &Value) -> Context {
    match json_val(v) {
        Val::Map(c) => c,
        _ => Context::default(),
    }
}

fn lua_context(v: &Value) -> Context {
    match lua_val(v) {
        Val::Map(c) => c,
        _ => Context::default(),
    }
}

fn compile_with(
    path: &str,
    source: &str,
    files: &BTreeMap<String, String>,
) -> Result<Template, CompileError<String>> {
    let mut load = |p: &str| -> Result<String, String> {
        files.get(p).cloned().ok_or_else(|| {
            let name = p.rsplit('/').next().unwrap_or(p);
            format!("Could not find data file 'templates/{name}'")
        })
    };
    Template::compile(path, source, DEPTH, &mut load)
}

fn compile(source: &str) -> Result<Template, CompileError<String>> {
    compile_with("", source, &BTreeMap::new())
}

fn error_text(result: Result<Template, CompileError<String>>) -> String {
    match result {
        Ok(_) => "compiled".to_owned(),
        Err(e) => e.to_string(),
    }
}

fn render(source: &str, ctx: &Context, width: Option<usize>) -> String {
    match compile(source) {
        Ok(t) => t.render(ctx).render(width),
        Err(e) => e.to_string(),
    }
}

const GOLDEN: &str = include_str!("../tests/doctemplates.txt");

fn golden_files() -> BTreeMap<String, String> {
    GOLDEN
        .split('\u{1e}')
        .filter_map(|record| record.split_once('\n'))
        .map(|(name, text)| (format!("test/{name}"), text.to_owned()))
        .collect()
}

#[test]
fn doctemplates_golden_tests() {
    let files = golden_files();
    let mut checked = 0;
    for (name, text) in &files {
        let Some(stem) = name.strip_suffix(".test") else {
            continue;
        };
        let parts: Vec<&str> = text.split("\n.\n").collect();
        let [json, template, expected] = parts.as_slice() else {
            unreachable!("{name} has three parts");
        };
        let value: Value = serde_json::from_str(json).unwrap_or(Value::Null);
        let path = format!("{stem}.txt");
        let template = compile_with(&path, &format!("{template}\n"), &files);
        let output = match template {
            Ok(t) => t.render(&json_context(&value)).render(None),
            Err(e) => e.to_string(),
        };
        assert_eq!(output, *expected, "golden test {name}");
        checked += 1;
    }
    assert_eq!(checked, 22);
}

#[test]
fn doctemplates_compile_failures() {
    assert_eq!(
        error_text(compile("$if(x$and$endif$")),
        "(line 1, column 6):\nunexpected \"$\"\nexpecting \".\", \"/\" or \")\""
    );
    let none = BTreeMap::new();
    assert_eq!(
        error_text(compile_with("foobar.txt", "$sep$", &none)),
        "\"foobar.txt\" (line 1, column 5):\nunexpected \"$\"\nexpecting letter or digit or \"()\""
    );
    assert_eq!(
        error_text(compile_with("foobar.txt", "$foo/nope$", &none)),
        "\"foobar.txt\" (line 1, column 10):\nunexpected \"$\"\nexpecting letter, letter or digit or \"()\"\nUnknown pipe nope"
    );
    assert_eq!(
        error_text(compile_with("foobar.txt", "$foo/left$", &none)),
        "\"foobar.txt\" (line 1, column 10):\nunexpected \"$\"\nexpecting letter, integer parameter for pipe, letter or digit or \"()\""
    );
    assert_eq!(
        error_text(compile_with("foobar.txt", "$foo/left a$", &none)),
        "\"foobar.txt\" (line 1, column 11):\nunexpected \"a\"\nexpecting integer parameter for pipe"
    );
    assert_eq!(
        error_text(compile_with("test/foobar.txt", "$bad()$", &golden_files())),
        "\"test/bad.txt\" (line 2, column 7):\nunexpected \"s\"\nexpecting \"$\""
    );
    assert_eq!(compile("$-- hi").map(|t| t.node), Ok(render::Node::Empty));
}

#[test]
fn doctemplates_unit_renders() {
    let mut ctx = Context::new();
    ctx.insert("foo", "42");
    assert_eq!(
        render(
            "not breakable and$~$ this is breakable\nok? $foo$$~$",
            &ctx,
            Some(10)
        ),
        "not breakable and\nthis is\nbreakable\nok? 42"
    );
    let words = ["hello", "this", "is", "a", "test", "of", "the", "wrapping"];
    let mut ctx = Context::new();
    ctx.insert("foo", hsep(words.map(literal)));
    assert_eq!(
        render("$foo/nowrap$\n$foo$", &ctx, Some(10)),
        "hello this is a test of the wrapping\nhello this\nis a test\nof the\nwrapping"
    );
    let mut ctx = Context::new();
    for (k, v) in [
        ("one", "1"),
        ("twentysix", "26"),
        ("twentyseven", "27"),
        ("zero", "0"),
    ] {
        ctx.insert(k, v);
    }
    assert_eq!(
        render(
            "$one/alpha$ $twentysix/alpha$ $twentyseven/alpha$ $zero/alpha$",
            &ctx,
            None
        ),
        "a z a 0"
    );
}

const ORACLE: &str = include_str!("../tests/oracle.json");

fn oracle() -> Value {
    serde_json::from_str(ORACLE).unwrap_or(Value::Null)
}

fn string_map(v: &Value) -> BTreeMap<String, String> {
    v.as_object()
        .map(|m| {
            m.iter()
                .map(|(k, x)| (k.clone(), x.as_str().unwrap_or("").to_owned()))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn oracle_cases() {
    let data = oracle();
    let context_text = data["context"].as_str().unwrap_or("{}");
    let ctx = lua_context(&serde_json::from_str(context_text).unwrap_or(Value::Null));
    let files = string_map(&data["partials"]);
    let cases = data["cases"].as_array().cloned().unwrap_or_default();
    let mut failures = Vec::new();
    for case in &cases {
        let id = case["id"].as_str().unwrap_or("?");
        let source = case["template"].as_str().unwrap_or("");
        let width = case["width"].as_u64().and_then(|w| usize::try_from(w).ok());
        let expected = case["output"]
            .as_str()
            .or_else(|| case["error"].as_str())
            .unwrap_or("");
        let actual = match compile_with("main.txt", source, &files) {
            Ok(t) => t.render(&ctx).render(width),
            Err(e) => e.to_string(),
        };
        if actual != expected {
            failures.push(format!(
                "{id}:\n  expected {expected:?}\n  actual   {actual:?}"
            ));
        }
    }
    assert!(cases.len() >= 750, "{} cases", cases.len());
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn default_templates_match_oracle() {
    let data = oracle();
    let contexts: Vec<Context> = data["default_contexts"]
        .as_array()
        .map(|xs| xs.iter().map(lua_context).collect())
        .unwrap_or_default();
    let defaults = data["defaults"].as_array().cloned().unwrap_or_default();
    let mut load = |p: &str| -> Result<String, String> {
        randoc_data::data_file(p)
            .and_then(|b| std::str::from_utf8(b).ok())
            .map(str::to_owned)
            .ok_or_else(|| p.to_owned())
    };
    let mut failures = Vec::new();
    for entry in &defaults {
        let format = entry["format"].as_str().unwrap_or("?");
        let Some(source) = randoc_data::default_template(format) else {
            failures.push(format!("{format}: no template"));
            continue;
        };
        let path = format!("templates/default.{format}");
        let template = match Template::compile(&path, source, DEPTH, &mut load) {
            Ok(t) => t,
            Err(e) => {
                failures.push(format!("{format}: {e}"));
                continue;
            }
        };
        for (i, ctx) in contexts.iter().enumerate() {
            let expected = entry["outputs"][i].as_str().unwrap_or("");
            let actual = template.render(ctx).render(Some(72));
            if actual != expected {
                failures.push(format!("{format} context {i}"));
            }
        }
    }
    assert_eq!(defaults.len(), 69);
    assert!(
        failures.is_empty(),
        "{} failures: {}",
        failures.len(),
        failures.join(", ")
    );
}

#[test]
fn haskell_show_double() {
    assert_eq!(show_double(7.3), "7.3");
    assert_eq!(show_double(0.01), "1.0e-2");
    assert_eq!(show_double(1.0e7), "1.0e7");
    assert_eq!(show_double(-2.5), "-2.5");
    assert_eq!(show_double(123.0), "123.0");
}

#[test]
fn meta_conversion() {
    use randoc_ast::{Inline, MetaValue};
    let mut meta = Meta::default();
    meta.insert("s", MetaValue::MetaString("a  b\nc".to_owned()));
    meta.insert("b", MetaValue::MetaBool(true));
    meta.insert(
        "l",
        MetaValue::MetaList(vec![MetaValue::MetaInlines(vec![Inline::Str(
            "x".to_owned(),
        )])]),
    );
    let mut nested = BTreeMap::new();
    nested.insert("k".to_owned(), MetaValue::MetaBlocks(Vec::new()));
    meta.insert("m", MetaValue::MetaMap(nested));
    let mut inlines = |xs: &[Inline]| -> Doc {
        literal(
            xs.iter()
                .map(|x| match x {
                    Inline::Str(s) => s.clone(),
                    Inline::Space => "_".to_owned(),
                    Inline::SoftBreak => "~".to_owned(),
                    _ => "?".to_owned(),
                })
                .collect::<String>(),
        )
    };
    let mut blocks = |_: &[randoc_ast::Block]| literal("BLOCKS");
    let ctx = Context::from_meta(&meta, &mut blocks, &mut inlines);
    let t = compile("$s$|$b$|$l$|$m.k$").unwrap_or_default();
    assert_eq!(t.render(&ctx).render(None), "a_b~c|true|x|BLOCKS");
    assert_eq!(t.source(), "$s$|$b$|$l$|$m.k$");
    assert_eq!(ctx.get("b"), Some(&Val::Bool(true)));
}

#[test]
fn errors_and_limits() {
    let deep = "$if(x)$".repeat(200);
    assert_eq!(compile(&deep), Err(CompileError::TooDeep));
    let chain = format!("$if(a)${}$endif$", "x$elseif(a)$".repeat(300));
    assert_eq!(compile(&chain), Err(CompileError::TooDeep));
    let missing = compile_with("t.txt", "$nothere()$", &BTreeMap::new());
    assert_eq!(
        missing,
        Err(CompileError::Partial(
            "Could not find data file 'templates/nothere.txt'".to_owned()
        ))
    );
    assert_eq!(
        CompileError::<String>::TooDeep.to_string(),
        "the template is nested too deeply"
    );
    assert_eq!(Val::from(false), Val::Bool(false));
    assert_eq!(Val::from(Doc::default()), Val::Simple(Doc::default()));
}

/// Runs a test on a thread with a 2 MB stack, the stack of a spawned
/// thread in many programs.
fn on_small_stack(test: impl FnOnce() + Send + 'static) {
    let handle = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(test);
    assert!(
        handle.is_ok_and(|h| h.join().is_ok()),
        "the test thread failed"
    );
}

fn nested(open: &str, close: &str, depth: usize) -> String {
    format!("{}X{}", open.repeat(depth), close.repeat(depth))
}

#[test]
fn default_depth_fits_a_small_stack() {
    on_small_stack(|| {
        let mut ctx = Context::new();
        ctx.insert("x", "yes");
        let cases = [
            nested("$if(x)$", "$endif$", 127),
            nested("$for(x)$", "$endfor$", 127),
            nested("$if(y)$$else$", "$endif$", 127),
            nested("$for(x)$$sep$", "$endfor$", 127),
            nested("$^$", "", 127),
            nested("$for(x/uppercase/pairs)$", "$endfor$", 127),
            format!("$if(y)${}X$endif$", "a$elseif(y)$".repeat(126)),
        ];
        for source in &cases {
            let template = compile_with("main.txt", source, &BTreeMap::new());
            let Ok(template) = template else {
                unreachable!("{} levels compile", source.len());
            };
            let copy = template.clone();
            let out = copy.render(&ctx).render(Some(72));
            assert!(out == "X" || out.is_empty() || out.contains('X'), "{out:?}");
            drop(copy);
            drop(template);
        }
        let partials = BTreeMap::from([("p.txt".to_owned(), "$if(x)$$p()$$endif$".to_owned())]);
        let template = compile_with("main.txt", "$p()$", &partials);
        assert_eq!(
            template.map(|t| t.render(&ctx).render(None)),
            Ok("(loop)".to_owned())
        );
    });
}

#[test]
fn excess_depth_gives_too_deep_on_a_small_stack() {
    on_small_stack(|| {
        for depth in [128, 10_000, 100_000] {
            for (open, close) in [
                ("$if(x)$", "$endif$"),
                ("$for(x)$", "$endfor$"),
                ("$^$", ""),
            ] {
                let source = nested(open, close, depth);
                assert_eq!(
                    compile(&source),
                    Err(CompileError::TooDeep),
                    "{open} {depth}"
                );
            }
        }
        for depth in [10_000, 100_000] {
            let cases = [
                nested("$if(y)$$else$", "$endif$", depth),
                nested("$for(x)$$sep$", "$endfor$", depth),
                nested("$for(x/uppercase/pairs)$", "$endfor$", depth),
                format!("$if(y)${}X$endif$", "a$elseif(y)$".repeat(depth)),
            ];
            for source in &cases {
                assert_eq!(compile(source), Err(CompileError::TooDeep), "{depth}");
            }
        }
        let sequence = "$if(a)$1$elseif(b)$2$else$3$endif$".repeat(300);
        assert!(
            compile(&sequence).is_ok(),
            "a sequence of conditionals is not nested"
        );
        let partials = BTreeMap::from([(
            "p.txt".to_owned(),
            "$if(x)$$if(x)$$p()$$endif$$endif$".to_owned(),
        )]);
        assert_eq!(
            compile_with("main.txt", "$p()$", &partials),
            Err(CompileError::TooDeep)
        );
    });
}

fn list_context(n: usize) -> Context {
    let mut ctx = Context::new();
    let items = (0..n).map(|i| Val::text(i.to_string())).collect();
    ctx.insert("list", Val::List(items));
    ctx
}

/// The renderer counts its steps in test builds (node visits, scope
/// lookups and value copies), so these tests need no clock (standing order
/// 21a). A copy of the context for each item makes the count quadratic.
#[test]
fn loop_over_a_long_list_is_linear() {
    let Ok(t) = compile("$for(list)$$it$$endfor$") else {
        unreachable!("the template compiles");
    };
    let small = list_context(1_000);
    let large = list_context(4_000);
    let small_steps = steps::count(|| drop(t.render(&small)));
    let large_steps = steps::count(|| drop(t.render(&large)));
    assert!(small_steps >= 1_000, "{small_steps}");
    assert!(
        large_steps <= small_steps * 5,
        "4 times the items took {large_steps} steps against {small_steps}"
    );
    assert_eq!(t.render(&list_context(20000)).render(None).len(), 88890);
}

#[test]
fn loop_work_does_not_depend_on_other_values() {
    let Ok(t) = compile("$for(author)$$author$$sep$; $endfor$") else {
        unreachable!("the template compiles");
    };
    let with_others = |others: usize| {
        let mut ctx = Context::new();
        ctx.insert("body", "x".repeat(100_000).as_str());
        for i in 0..others {
            ctx.insert(
                format!("v{i}"),
                Val::List(vec![Val::text("a"), Val::text("b")]),
            );
        }
        let authors = (0..1000).map(|i| Val::text(format!("A{i}"))).collect();
        ctx.insert("author", Val::List(authors));
        ctx
    };
    let few = with_others(10);
    let many = with_others(1000);
    let few_steps = steps::count(|| drop(t.render(&few)));
    let many_steps = steps::count(|| drop(t.render(&many)));
    assert_eq!(few_steps, many_steps);
    let out = t.render(&many).render(None);
    assert!(out.starts_with("A0; A1; ") && out.ends_with("; A999"));
}

/// A context with `n` items at `list`, `a.b` and `a.b.c` (each item a map
/// with a field), an outer value and a large `body`.
fn scope_context(n: usize) -> Context {
    let items = || {
        Val::List(
            (0..n)
                .map(|i| {
                    let mut m = Context::new();
                    m.insert("name", format!("n{i}").as_str());
                    Val::Map(m)
                })
                .collect(),
        )
    };
    let mut b2 = Context::new();
    b2.insert("c", items());
    b2.insert("d", "D");
    let mut a2 = Context::new();
    a2.insert("b", Val::Map(b2));
    a2.insert("e", "E");
    let mut a1 = Context::new();
    a1.insert("b", items());
    a1.insert("e", "E");
    let mut ctx = Context::new();
    ctx.insert("list", items());
    ctx.insert("a", Val::Map(a1));
    ctx.insert("deep", Val::Map(a2));
    ctx.insert("outer", "O");
    ctx.insert("body", "x".repeat(100_000).as_str());
    ctx
}

#[test]
fn loop_lookups_are_linear() {
    let templates = [
        "$for(list)$$list.name$$it.name$$it$$outer$$list/length$$endfor$",
        "$for(a.b)$$a.b.name$$a/length$$a$$a.e$$it.name$$outer$$a.b/length$$endfor$",
        "$for(a.b)$$if(a)$$a/first/length$$endif$$a/reverse/length$$endfor$",
        "$for(deep.b.c)$$deep/length$$deep.b/length$$deep.b.d$$deep.b.c.name$$it.name$$deep.e$$endfor$",
        "$for(a.b)$$for(list/first)$$a/length$$list.name$$it.name$$endfor$$endfor$",
        "$for(list)$$deep.b.c/first$$deep.b.c/last$$deep.b.c/length$$endfor$",
    ];
    for source in templates {
        let Ok(t) = compile(source) else {
            unreachable!("{source} compiles");
        };
        let small = scope_context(1_000);
        let large = scope_context(4_000);
        let small_steps = steps::count(|| drop(t.render(&small)));
        let large_steps = steps::count(|| drop(t.render(&large)));
        assert!(small_steps >= 1_000, "{source}: {small_steps}");
        assert!(
            large_steps <= small_steps * 5,
            "{source}: 4 times the items took {large_steps} steps against {small_steps}"
        );
    }
}

/// The context of the cost matrix: lists of `n` items at several depths,
/// a map of `n` entries and a large `body`.
fn matrix_context(n: usize) -> Context {
    let texts = || Val::List((0..n).map(|i| Val::text(i.to_string())).collect());
    let maps = || {
        Val::List(
            (0..n)
                .map(|i| {
                    let mut m = Context::new();
                    m.insert("name", format!("n{i}").as_str());
                    m.insert("k", "v");
                    Val::Map(m)
                })
                .collect(),
        )
    };
    let mut big = Context::new();
    for i in 0..n {
        big.insert(format!("k{i}"), format!("v{i}").as_str());
    }
    let mut x = Context::new();
    x.insert("y", maps());
    x.insert("w", texts());
    x.insert("z", "zz");
    let mut d = Context::new();
    d.insert("x", Val::Map(x));
    let mut a = Context::new();
    a.insert("b", maps());
    a.insert("c", texts());
    a.insert("k", "v");
    let mut ctx = Context::new();
    ctx.insert("l", maps());
    ctx.insert("l2", maps());
    ctx.insert("l3", texts());
    ctx.insert("m", Val::Map(big));
    ctx.insert("a", Val::Map(a));
    ctx.insert("deep", Val::Map(d));
    ctx.insert("body", "x".repeat(100_000).as_str());
    ctx
}

/// The templates of the cost matrix: each loop source with each body
/// form. Values of size `n` outside the loop are used with pipes that give
/// one item, so that the output of each template grows linearly.
fn matrix_templates() -> Vec<String> {
    let sources: [(&str, &str, &[&str], &str); 5] = [
        ("$for(l)$", "l", &[], "$endfor$"),
        ("$for(a.b)$", "a.b", &["a"], "$endfor$"),
        (
            "$for(deep.x.y)$",
            "deep.x.y",
            &["deep.x", "deep"],
            "$endfor$",
        ),
        ("$for(a.b)$$for(a)$", "a", &["a.k"], "$endfor$$endfor$"),
        ("$for(l2/rest/reverse)$", "l2", &[], "$endfor$"),
    ];
    let mut outer = Vec::new();
    for chain in [
        "length",
        "first",
        "last",
        "rest/first",
        "allbutlast/last",
        "reverse/first",
        "pairs/first",
        "pairs/last",
        "rest/rest/last",
        "reverse/rest/first",
    ] {
        outer.push(format!("l2/{chain}"));
    }
    for chain in [
        "uppercase/first",
        "lowercase/last",
        "chomp/first",
        "alpha/first",
        "roman/first",
        "nowrap/first",
        "left 3/first",
        "center 3/first",
        "right 3/first",
        "uppercase/rest/first",
    ] {
        outer.push(format!("l3/{chain}"));
    }
    for chain in [
        "length",
        "first",
        "pairs/first",
        "pairs/last",
        "uppercase",
        "reverse",
        "rest",
    ] {
        outer.push(format!("m/{chain}"));
    }
    let mut out = Vec::new();
    for (open, var, prefixes, close) in sources {
        let mut elements = vec![var.to_owned(), "it".to_owned(), "it.name".to_owned()];
        elements.extend(prefixes.iter().map(|p| (*p).to_owned()));
        elements.extend(outer.iter().cloned());
        for e in &elements {
            for body in [
                format!("${e}$"),
                format!("$if({e})$y$endif$"),
                format!("${e}:p()$"),
                format!("$for({e})$[$it$]$endfor$"),
            ] {
                out.push(format!("{open}{body}{close}"));
            }
        }
    }
    for review in [
        "$for(a.b)$$if(a/uppercase)$y$endif$$endfor$",
        "$for(l)$$l2/rest/first$$endfor$",
        "$for(l)$$l2/allbutlast/first$$endfor$",
        "$for(a.b)$$for(a)$$it.k$$endfor$$endfor$",
        "$for(deep.x.y)$$for(deep.x)$$it.z$$endfor$$endfor$",
        "$for(a.b)$$a:p()$$endfor$",
        "$for(l)$$if(l2)$y$endif$$endfor$",
    ] {
        out.push(review.to_owned());
    }
    out
}

/// Standing orders 21a and 21c: each cell of the matrix takes less than 6
/// times the steps for 4 times the items.
#[test]
fn loop_cost_matrix_is_linear() {
    let files = BTreeMap::from([("p.txt".to_owned(), "($it$)".to_owned())]);
    let small = matrix_context(1_000);
    let large = matrix_context(4_000);
    let templates = matrix_templates();
    assert!(templates.len() >= 500, "{} templates", templates.len());
    for source in &templates {
        let Ok(t) = compile_with("main.txt", source, &files) else {
            unreachable!("{source} compiles");
        };
        let small_steps = steps::count(|| drop(t.render(&small).render(Some(72))));
        let large_steps = steps::count(|| drop(t.render(&large).render(Some(72))));
        assert!(
            large_steps < small_steps * 6,
            "{source}: 4 times the items took {large_steps} steps against {small_steps}"
        );
    }
}

#[test]
fn each_partial_is_read_one_time() {
    let mut calls = 0;
    let mut load = |path: &str| -> Result<String, String> {
        calls += 1;
        Ok(format!("({path}:$it$)"))
    };
    let source = "$p()$".repeat(1000) + "$list:p()[,]$";
    let Ok(t) = Template::compile("main.txt", &source, DEPTH, &mut load) else {
        unreachable!("the template compiles");
    };
    assert_eq!(calls, 1);
    let mut ctx = Context::new();
    ctx.insert("list", Val::List(vec![Val::text("a"), Val::text("b")]));
    let out = t.render(&ctx).render(None);
    assert!(out.starts_with("(p.txt:)(p.txt:)") && out.ends_with("(p.txt:a),(p.txt:b)"));
}

/// Codex review, round 4 (TPL-R4-01): `pairs` of the result of `pairs` on
/// a map. The step counter counts each visited map entry; 4 times the
/// entries may take less than 5 times the steps.
#[test]
fn pairs_of_map_pairs_are_linear() {
    let map = |n: usize| {
        let mut m = Context::new();
        for i in 0..n {
            m.insert(format!("k{i:05}"), format!("v{i}").as_str());
        }
        let mut ctx = Context::new();
        ctx.insert("m", Val::Map(m));
        ctx
    };
    let small = map(1_000);
    let large = map(4_000);
    for source in [
        "$for(m/pairs/pairs)$$it.key$$endfor$",
        "$for(m/pairs/reverse/pairs)$$it.key$$endfor$",
        "$for(m/pairs/rest/pairs)$$it.key$$endfor$",
        "$for(m/pairs/uppercase/pairs)$$it.key$$endfor$",
        "$for(m/pairs/pairs/pairs)$$it.key$$endfor$",
        "$m/pairs/pairs$",
        "$for(m/pairs/pairs)$$for(it/pairs)$$it.key$=$it.value$$endfor$$endfor$",
    ] {
        let Ok(t) = compile(source) else {
            unreachable!("{source} compiles");
        };
        let small_steps = steps::count(|| drop(t.render(&small).render(None)));
        let large_steps = steps::count(|| drop(t.render(&large).render(None)));
        eprintln!("{source}: {small_steps} {large_steps}");
        assert!(
            large_steps < small_steps * 5,
            "{source}: 4 times the entries took {large_steps} steps against {small_steps}"
        );
    }
}

/// The oracle gives `[key=a][value=x];[key=b][value=y];` (`pandoc lua`).
#[test]
fn pairs_of_a_pairs_item() {
    let mut m = Context::new();
    m.insert("a", "x");
    m.insert("b", "y");
    let mut ctx = Context::new();
    ctx.insert("m", Val::Map(m));
    assert_eq!(
        render(
            "$for(m/pairs)$$for(it/pairs)$[$it.key$=$it.value$]$endfor$;$endfor$",
            &ctx,
            None
        ),
        "[key=a][value=x];[key=b][value=y];"
    );
}

/// The oracle gives `[B1|B1][B1|B1][B2|B2][B2|B2]` (Opus review, round 3).
#[test]
fn sibling_loops_with_a_pipe_in_the_third() {
    let mut a3 = Context::new();
    a3.insert("b", Val::List(vec![Val::text("b1"), Val::text("b2")]));
    a3.insert("c", Val::List(vec![Val::text("c1"), Val::text("c2")]));
    let mut ctx = Context::new();
    ctx.insert("a3", Val::Map(a3));
    assert_eq!(
        render(
            "$for(a3.b)$$for(a3.c)$$for(a3.b/uppercase)$[$a3.b$|$it$]$endfor$$endfor$$endfor$",
            &ctx,
            None
        ),
        "[B1|B1][B1|B1][B2|B2][B2|B2]"
    );
}

#[test]
/// Expected text from the oracle: `pandoc -f markdown -t plain` with the
/// metadata `l: []` and `m: [a, b]` (Lua cannot make an empty list).
fn pipes_on_an_empty_list() {
    let mut ctx = Context::new();
    ctx.insert("l", Val::List(Vec::new()));
    ctx.insert("m", Val::List(vec![Val::text("a"), Val::text("b")]));
    assert_eq!(
        render(
            "[$l/first$|$l/last$|$l/length$|$m/first$|$m/last$]",
            &ctx,
            None
        ),
        "[||0|a|b]"
    );
}

#[test]
fn loop_scopes_follow_add_to_context() {
    let mut inner = Context::new();
    inner.insert("b", Val::List(vec![Val::text("1"), Val::text("2")]));
    inner.insert("c", "C");
    let mut ctx = Context::new();
    ctx.insert("a", Val::Map(inner));
    ctx.insert("it", "top");
    let cases = [
        ("$for(a.b)$[$a.b$ $a.c$ $it$]$endfor$", "[1 C 1][2 C 2]"),
        (
            "$for(a.b)$$for(a)$<$a.b$|$it.c$>$endfor$$endfor$",
            "<1|C><2|C>",
        ),
        ("$for(a.b)$$a/length$$endfor$|$it$", "22|top"),
        ("$for(missing/length)$[$missing$|$it$]$endfor$", "[|0]"),
        ("$for(a.zz/length)$[$a.zz$]$endfor$", "[]"),
    ];
    for (source, expected) in cases {
        assert_eq!(render(source, &ctx, None), expected, "{source}");
    }
}

#[test]
fn errors_after_pipes_and_partials() {
    let none = BTreeMap::new();
    assert_eq!(
        error_text(compile_with("main.txt", "$s/uppercase.x$", &none)),
        "\"main.txt\" (line 1, column 13):\nunexpected \".\"\nexpecting \"/\", \"(\", \":\", \"[\" or \"$\""
    );
    assert_eq!(
        error_text(compile_with("main.txt", "$s/left 3 x$", &none)),
        "\"main.txt\" (line 1, column 11):\nunexpected \"x\"\nexpecting \"/\", \"(\", \":\", \"[\" or \"$\""
    );
    assert_eq!(
        error_text(compile_with("main.txt", "$x/left 3.5$", &none)),
        "\"main.txt\" (line 1, column 10):\nunexpected \".\"\nexpecting \"/\", \"(\", \":\", \"[\" or \"$\""
    );
    let files = BTreeMap::from([
        ("a2.txt".to_owned(), "x\ny".to_owned()),
        ("p.txt".to_owned(), "($it$)".to_owned()),
    ]);
    assert_eq!(
        error_text(compile_with("main.txt", "$a2()[a]x$", &files)),
        "\"a2.txt\" (line 2, column 2):\nunexpected end of input\nexpecting \"$--\", \"\\n\", \"\\r\\n\", \"\\r\", \"$\", \"${\", \"$$\" or end of input"
    );
    assert_eq!(
        error_text(compile_with("main.txt", "$p()[a]x$", &files)),
        "\"main.txt\" (line 1, column 8):\nunexpected \"x\"\nexpecting \"/\" or \"$\""
    );
}

const REPEATED: [&str; 16] = [
    "$x$",
    "${x}",
    "$$",
    "$-- c\n",
    "$if(x)$a$endif$",
    "$if(y)$a$elseif(x)$b$endif$",
    "$for(list)$a$sep$,$endfor$",
    "$list[,]$",
    "$x/uppercase/left 3$",
    "$~$a b",
    "\n  $ml$",
    "$^$a\n",
    "$item()$",
    "$list:item()[,]$",
    "plain text ",
    "$if(x$",
];

fn repeated_context() -> (Context, BTreeMap<String, String>) {
    let files = BTreeMap::from([("item.txt".to_owned(), "($it$)".to_owned())]);
    let mut ctx = Context::new();
    ctx.insert("x", "yes");
    ctx.insert("ml", "a\nb");
    ctx.insert("list", Val::List(vec![Val::text("a"), Val::text("b")]));
    (ctx, files)
}

/// Standing orders 21a and 21c: the parser and the renderer count their
/// steps (character tests, string tests, backtracking points, node visits,
/// lookups and value copies). Four times the repetitions of one construct
/// may take at most 4.5 times the steps.
#[test]
fn work_per_construct_is_linear() {
    let (ctx, files) = repeated_context();
    for construct in REPEATED {
        let work = |count: usize| {
            let source = construct.repeat(count);
            steps::count(|| {
                if let Ok(t) = compile_with("main.txt", &source, &files) {
                    drop(t.render(&ctx).render(Some(72)));
                }
            })
        };
        let small = work(200);
        let large = work(800);
        assert!(
            large * 2 <= small * 9,
            "{construct:?}: {large} steps against {small}"
        );
    }
}

/// Standing order 21c: each construct repeated 12,500 and 50,000 times.
/// Run with `cargo test -p randoc-templates repeated_constructs -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement for the report"]
fn repeated_constructs_time() {
    let (ctx, files) = repeated_context();
    for construct in REPEATED {
        let mut times = Vec::new();
        for count in [12_500, 50_000] {
            let source = construct.repeat(count);
            let start = std::time::Instant::now();
            let template = compile_with("main.txt", &source, &files);
            let compiled = start.elapsed();
            let start = std::time::Instant::now();
            let len = template.map(|t| t.render(&ctx).render(Some(72)).len());
            times.push(format!(
                "{count}: compile {compiled:?}, render {:?} ({len:?})",
                start.elapsed()
            ));
        }
        eprintln!("{construct:?}: {}", times.join(", "));
    }
}
