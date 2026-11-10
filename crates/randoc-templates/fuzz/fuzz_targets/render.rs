//! Compiles arbitrary template text and renders it with a fixed context.

#![no_main]

use libfuzzer_sys::fuzz_target;
use randoc_layout::{Doc, hsep, literal};
use randoc_templates::{Context, Template, Val};

fn context() -> Context {
    let mut ctx = Context::new();
    ctx.insert("x", "hello");
    ctx.insert("e", "");
    ctx.insert("t", true);
    ctx.insert("f", false);
    ctx.insert("ml", "line1\nline2\n");
    ctx.insert("num", "1994");
    ctx.insert("words", hsep(["a", "b", "c", "d"].map(literal)));
    ctx.insert("list", Val::List(vec![Val::text("a"), Val::text("b")]));
    let mut map = Context::new();
    map.insert("k", "v");
    map.insert("list", Val::List(vec![Val::text("1"), Val::text("2")]));
    ctx.insert("map", Val::Map(map.clone()));
    ctx.insert("maps", Val::List(vec![Val::Map(map.clone()), Val::Map(map)]));
    ctx
}

/// Compiles and renders on a thread with a 2 MB stack and the default
/// depth limit, so that a stack overflow at that limit is a crash.
fn run(source: String) {
    let mut load = |path: &str| -> Result<String, ()> {
        match path {
            "p.txt" => Ok("($it$)\n".to_owned()),
            "loop.txt" => Ok("L$loop()$".to_owned()),
            "q.txt" => Ok("$for(list)$$it$$sep$,$endfor$".to_owned()),
            "deep.txt" => Ok("$if(x)$$deep()$$endif$".to_owned()),
            _ => Err(()),
        }
    };
    if let Ok(template) = Template::compile("main.txt", &source, 128, &mut load) {
        let doc: Doc = template.clone().render(&context());
        let _ = doc.render(Some(20));
        let _ = doc.render(None);
    }
}

fuzz_target!(|data: &[u8]| {
    let source = String::from_utf8_lossy(data).into_owned();
    let handle = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || run(source));
    if let Ok(handle) = handle {
        if handle.join().is_err() {
            std::process::abort();
        }
    }
});
