//! Regression tests with document trees made for the LaTeX writer.

use randoc_ast::{
    Attr, Block, Caption, Citation, CitationMode, MathType, MetaValue, Pandoc, QuoteType, Table,
};
use randoc_ast::{Cell, ColSpec, Inline, Row, TableBody, Target};
use randoc_core::{
    CaptionPosition, CiteMethod, Context, WrapOption, WriterOptions, default_extensions,
};
use randoc_templates::{Template, Val};

use crate::{write_beamer, write_latex};

fn options() -> WriterOptions {
    let mut options = WriterOptions::default();
    options.extensions = default_extensions("latex");
    options.wrap_text = WrapOption::WrapNone;
    options
}

fn text(s: &str) -> Inline {
    Inline::Str(s.into())
}

fn paragraph(s: &str) -> Block {
    Block::Para(vec![text(s)])
}

fn document(blocks: Vec<Block>) -> Pandoc {
    Pandoc {
        blocks,
        ..Pandoc::default()
    }
}

fn latex(blocks: Vec<Block>, options: &WriterOptions) -> String {
    write_latex(&document(blocks), options, &mut Context::pure()).unwrap()
}

fn attr(id: &str, classes: &[&str], pairs: &[(&str, &str)]) -> Attr {
    Attr::new(
        id,
        classes.iter().map(|class| (*class).into()).collect(),
        pairs
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect(),
    )
}

fn row(inlines: Vec<Inline>) -> Row {
    Row {
        cells: vec![Cell {
            content: vec![Block::Plain(inlines)],
            ..Cell::default()
        }],
        ..Row::default()
    }
}

fn sample_table(attr: Attr, caption: Vec<Block>) -> Table {
    Table {
        attr,
        caption: Caption {
            short: None,
            long: caption,
        },
        col_specs: vec![ColSpec::default()],
        bodies: vec![TableBody {
            body: vec![row(vec![text("24")])],
            ..TableBody::default()
        }],
        ..Table::default()
    }
}

#[test]
fn float_with_placement_and_caption() {
    let mut table = sample_table(
        attr("data", &["float"], &[("latex-placement", "!ht")]),
        vec![paragraph("Measurements")],
    );
    table.head.rows = vec![row(vec![text("Length")])];
    assert_eq!(
        latex(vec![Block::Table(Box::new(table.clone()))], &options()),
        r"\begin{table}[!ht]
\centering
\caption{Measurements}\label{data}\tabularnewline
\begin{tabular}{@{}l@{}}
\toprule\noalign{}
Length \\
\midrule\noalign{}
24 \\
\bottomrule\noalign{}
\end{tabular}
\end{table}"
    );

    let mut options = options();
    options.table_caption_position = CaptionPosition::CaptionBelow;
    let doc = document(vec![Block::Table(Box::new(table))]);
    let out = write_beamer(&doc, &options, &mut Context::pure()).unwrap();
    assert!(out.starts_with("\\begin{frame}\n\\begin{table}[!ht]"));
    assert!(out.ends_with(
        "\\end{tabular}\n\\caption{Measurements}\\label{data}\\tabularnewline\n\\end{table}\n\\end{frame}"
    ));
}

#[test]
fn float_with_no_header() {
    let table = sample_table(attr("data", &["float"], &[]), vec![paragraph("Length")]);
    assert_eq!(
        latex(vec![Block::Table(Box::new(table))], &options()),
        r"\begin{table}[]
\centering
\caption{Length}\label{data}\tabularnewline
\begin{tabular}{@{}l@{}}
\toprule\noalign{}
24 \\
\bottomrule\noalign{}
\end{tabular}
\end{table}"
    );
}

#[test]
fn float_with_notes_and_footer() {
    let mut table = sample_table(attr("", &["float"], &[]), vec![paragraph("Lengths")]);
    table.head.rows = vec![row(vec![
        text("cm"),
        Inline::Note(vec![paragraph("Centimeters.")]),
    ])];
    table.foot.rows = vec![row(vec![text("Total")])];
    assert_eq!(
        latex(vec![Block::Table(Box::new(table))], &options()),
        r"\begin{table}[]
\centering
\caption{Lengths}\tabularnewline
\begin{tabular}{@{}l@{}}
\toprule\noalign{}
cm\footnotemark{} \\
\midrule\noalign{}
24 \\
\midrule\noalign{}
Total \\
\bottomrule\noalign{}
\end{tabular}
\end{table}
\footnotetext{Centimeters.}"
    );
}

#[test]
fn unnumbered_table_suppresses_an_empty_caption_and_label() {
    let mut table = sample_table(attr("length", &["unnumbered"], &[]), vec![]);
    table.head.rows = vec![row(vec![text("Depth")])];
    let render = |table: &Table| latex(vec![Block::Table(Box::new(table.clone()))], &options());
    assert_eq!(
        render(&table),
        r"{\def\LTcaptype{none} % do not increment counter
\begin{longtable}[]{@{}l@{}}
\toprule\noalign{}
Depth \\
\midrule\noalign{}
\endhead
\bottomrule\noalign{}
\endlastfoot
24 \\
\end{longtable}
}"
    );

    table.caption.long.push(paragraph("Length"));
    let named = render(&table);
    assert!(named.starts_with(r"{\def\LTcaptype{none}"));
    assert!(named.contains(r"\caption{Length}\label{length}\tabularnewline"));

    table.caption.long.clear();
    table.attr.classes.clear();
    let numbered = render(&table);
    assert!(numbered.starts_with(r"\begin{longtable}[]"));
    assert!(numbered.contains(r"\caption{}\label{length}\tabularnewline"));
}

#[test]
fn float_without_a_caption_uses_the_unnumbered_group() {
    let table = sample_table(attr("", &["float"], &[]), vec![]);
    assert_eq!(
        latex(vec![Block::Table(Box::new(table))], &options()),
        r"{\def\LTcaptype{none} % do not increment counter
\begin{table}[]
\centering
\begin{tabular}{@{}l@{}}
\toprule\noalign{}
24 \\
\bottomrule\noalign{}
\end{tabular}
\end{table}
}"
    );
}

#[test]
fn pdf_standard_sets_the_document_metadata() {
    let mut options = options();
    let template = include_str!("../../randoc-data/data/templates/document-metadata.latex");
    options.template = Some(
        Template::compile("document-metadata.latex", template, 16, &mut |_| {
            Err::<String, ()>(())
        })
        .unwrap(),
    );
    for (input, version, standards, tagging) in [
        (&["A-2B"][..], Some("1.7"), "a-2b", false),
        (&["ua-1"][..], None, "ua-1", true),
        (&["a-3a", "ua-2", "1.7"][..], Some("1.7"), "a-3a,ua-2", true),
        (&["a-1b", "a-2b"][..], Some("1.4"), "a-1b,a-2b", false),
        (&["a-4f"][..], None, "a-4f", false),
        (&["ua-2", "2"][..], Some("2.0"), "ua-2", true),
        (&["a-2a", "1.5", "1.6"][..], Some("1.5"), "a-2a", true),
    ] {
        let mut doc = Pandoc::default();
        let value = |s: &str| MetaValue::MetaInlines(vec![text(s)]);
        let metadata = match input {
            [single] => value(single),
            _ => MetaValue::MetaList(input.iter().map(|item| value(item)).collect()),
        };
        doc.meta.insert("pdfstandard", metadata);
        let mut expected = String::from("\\DocumentMetadata{\n");
        if let Some(version) = version {
            expected.push_str(&format!("  pdfversion={version},\n"));
        }
        expected.push_str(&format!("  pdfstandard={{{standards}}},\n"));
        if tagging {
            expected.push_str("  tagging=on,\n");
        }
        expected.push_str("  xmp=true}");
        assert_eq!(
            write_latex(&doc, &options, &mut Context::pure())
                .unwrap()
                .trim_end(),
            expected,
            "{input:?}"
        );
    }
}

#[test]
fn pdf_standard_variable_overrides_metadata_and_reports_unknown_values() {
    let mut options = options();
    options.template = Some(
        Template::compile(
            "test",
            "$for(pdfstandard.standards)$$it$$sep$,$endfor$",
            16,
            &mut |_| Err::<String, ()>(()),
        )
        .unwrap(),
    );
    options.variables.insert(
        "pdfstandard",
        Val::List(["X-6", "X-3", "A-1A"].map(Val::text).into()),
    );
    let mut doc = Pandoc::default();
    doc.meta
        .insert("pdfstandard", MetaValue::MetaString("a-1b".into()));
    let mut context = Context::pure();
    assert_eq!(write_latex(&doc, &options, &mut context).unwrap(), "x-6");
    assert_eq!(
        context
            .log()
            .iter()
            .map(|message| message.text())
            .collect::<Vec<_>>(),
        [
            "PDF standard 'x-3' is not supported by LaTeX and will be ignored.",
            "PDF standard 'a-1a' is not supported by LaTeX and will be ignored.",
        ]
    );
}

#[test]
fn definition_labels_with_equals_signs_use_braces() {
    let list = Block::DefinitionList(vec![
        (
            vec![text("x = 3")],
            vec![vec![Block::Plain(vec![text("First")])]],
        ),
        (
            vec![text("Mode [A]")],
            vec![vec![Block::Plain(vec![text("Second")])]],
        ),
    ]);
    assert_eq!(
        latex(vec![list], &options()),
        r"\begin{description}
\tightlist
\item[{x = 3}]
First
\item[Mode {[}A{]}]
Second
\end{description}"
    );
}

#[test]
fn definition_labels_ignore_equals_signs_in_nested_inlines() {
    for (term, label) in [
        (Inline::Math(MathType::InlineMath, "x=1".into()), r"\(x=1\)"),
        (Inline::Code(Attr::default(), "a=b".into()), r"\texttt{a=b}"),
        (
            Inline::Link(
                Attr::default(),
                vec![text("a=b")],
                Target::new("http://a?b=c", ""),
            ),
            r"\href{http://a?b=c}{a=b}",
        ),
        (Inline::Emph(vec![text("a=b")]), r"\emph{a=b}"),
        (
            Inline::Quoted(QuoteType::DoubleQuote, vec![text("a=b")]),
            "\x60\x60a=b''",
        ),
        (
            Inline::Span(attr("", &["x"], &[]), vec![text("a=b")]),
            "{a=b}",
        ),
    ] {
        let list = Block::DefinitionList(vec![(vec![term], vec![vec![paragraph("Definition.")]])]);
        let output = latex(vec![list], &options());
        assert!(output.contains(&format!("\\item[{label}]\n")), "{output}");
    }
}

#[test]
fn single_section_note_has_no_frame() {
    let mut options = options();
    options.slide_level = Some(2);
    let note = Block::Div(
        attr("", &["notes"], &[]),
        vec![paragraph("Check the timer.")],
    );
    let doc = document(vec![
        Block::Header(1, attr("setup", &[], &[]), vec![text("Setup")]),
        note.clone(),
        Block::Header(2, attr("run", &[], &[]), vec![text("Run")]),
        paragraph("Start."),
    ]);
    assert_eq!(
        write_beamer(&doc, &options, &mut Context::pure()).unwrap(),
        r"\section{Setup}\label{setup}

\note{Check the timer.}

\begin{frame}{Run}
\protect\phantomsection\label{run}
Start.
\end{frame}"
    );
    let doc = document(vec![note]);
    assert_eq!(
        write_beamer(&doc, &options, &mut Context::pure()).unwrap(),
        "\\begin{frame}\n\\note{Check the timer.}\n\\end{frame}"
    );
}

#[test]
fn listings_preserve_inline_and_block_options_in_order() {
    let code_attr = attr(
        "counter",
        &["cpp", "numberLines"],
        &[
            ("style", "compact"),
            ("exports", "both"),
            ("startFrom", "4"),
            ("tangle", "yes"),
            ("caption", "First, second"),
            ("results", "out"),
            ("value", "x=y"),
        ],
    );
    let mut options = options();
    options.listings = true;
    let blocks = vec![
        Block::Para(vec![Inline::Code(code_attr.clone(), "n++;".into())]),
        Block::CodeBlock(code_attr, "n++;".into()),
    ];
    assert_eq!(
        latex(blocks, &options),
        r"\passthrough{\lstinline[language={C++}, style=compact, startFrom=4, caption=First, second, value=x=y]!n++;!}

\begin{lstlisting}[language={C++}, numbers=left, style=compact, firstnumber=4, caption={First, second}, value={x=y}, label=counter]
n++;
\end{lstlisting}
"
    );
}

#[test]
fn display_math_keeps_its_environment_and_spacing() {
    for (env, body) in [
        ("eqnarray", "u & = & v"),
        ("equation", "  t^3"),
        ("align", "  q &= r \\\\"),
        ("equation", "    s + t"),
        ("align", "x &= 2 \\\\\ny &= 3"),
        ("dmath*", "k + n"),
        ("dgroup*", "m = n"),
    ] {
        let tex = format!(" \t\\begin{{{env}}}\n{body}\n  \\end{{{env}}}");
        let blocks = vec![Block::Para(vec![Inline::Math(
            MathType::DisplayMath,
            tex.clone(),
        )])];
        assert_eq!(latex(blocks, &options()), tex);
    }
    let tex = r"\begin{aligned}a&=b\end{aligned}";
    let blocks = vec![Block::Para(vec![Inline::Math(
        MathType::DisplayMath,
        tex.into(),
    )])];
    assert_eq!(latex(blocks, &options()), format!("\\[{tex}\\]"));
}

#[test]
fn display_math_keeps_its_environment_in_cells_and_underlines() {
    let equation = r"\begin{equation}x\end{equation}";
    let mut table = sample_table(Attr::default(), vec![]);
    table.bodies[0].body = vec![row(vec![Inline::Math(
        MathType::DisplayMath,
        equation.into(),
    )])];
    let output = latex(vec![Block::Table(Box::new(table))], &options());
    assert!(output.contains(&format!("{equation} \\\\")), "{output}");

    let align = r"\begin{align}x\end{align}";
    let underline = Inline::Underline(vec![Inline::Math(MathType::DisplayMath, align.into())]);
    assert_eq!(
        latex(vec![Block::Para(vec![underline])], &options()),
        format!("\\ul{{{align}}}")
    );
}

#[test]
fn biblatex_groups_only_contiguous_unqualified_keys() {
    let mut options = options();
    options.cite_method = CiteMethod::Biblatex;
    let affix = |s: &str| if s.is_empty() { vec![] } else { vec![text(s)] };
    for (affixes, expected) in [
        (
            [("see", ""), ("", ""), ("compare", ""), ("", "")],
            r"\autocites[see][]{a,b}[compare][]{c,d}",
        ),
        (
            [("see", ""), ("see", ""), ("", ""), ("compare", "")],
            r"\autocites[see][]{a}[see][]{b,c}[compare][]{d}",
        ),
        (
            [("see", "p.\u{a0}9"), ("", ""), ("", ""), ("compare", "")],
            r"\autocites[see][9]{a}{b,c}[compare][]{d}",
        ),
        (
            [("see", ""), ("", "ch. 7"), ("", ""), ("", "")],
            r"\autocites[see][]{a}[ch. 7]{b}{c,d}",
        ),
        (
            [("see", "note"), ("", "ch. 7"), ("", ""), ("compare", "")],
            r"\autocites[see][note]{a}[ch. 7]{b}{c}[compare][]{d}",
        ),
    ] {
        let citations = ["a", "b", "c", "d"]
            .into_iter()
            .zip(affixes)
            .map(|(id, (prefix, suffix))| Citation {
                id: id.into(),
                prefix: affix(prefix),
                suffix: affix(suffix),
                mode: CitationMode::NormalCitation,
                note_num: 0,
                hash: 0,
            })
            .collect();
        assert_eq!(
            latex(
                vec![Block::Para(vec![Inline::Cite(citations, vec![])])],
                &options
            ),
            expected
        );
    }
}

#[test]
fn punctuation_before_opening_quotes_gets_a_kern() {
    let mut options = options();
    options.wrap_text = WrapOption::WrapPreserve;
    let mut inlines = Vec::new();
    for s in ["What?“", "Look!“", "What?‘", "Look!‘", "Stop!"] {
        if !inlines.is_empty() {
            inlines.push(Inline::SoftBreak);
        }
        inlines.push(text(s));
    }
    let expected = "What?{\\kern0pt}\x60\x60\nLook!{\\kern0pt}\x60\x60\nWhat?{\\kern0pt}\x60\nLook!{\\kern0pt}\x60\nStop!";
    assert_eq!(latex(vec![Block::Para(inlines)], &options), expected);
}

#[test]
fn a_quote_at_either_edge_of_a_nested_span_gets_a_thin_space() {
    let quoted = Inline::Quoted(QuoteType::DoubleQuote, vec![text("inner")]);
    let span = Inline::Span(
        Attr::default(),
        vec![Inline::Span(Attr::default(), vec![quoted])],
    );
    let outer = Inline::Quoted(QuoteType::SingleQuote, vec![span]);
    assert_eq!(
        latex(vec![Block::Para(vec![outer])], &options()),
        "\x60\\,{{\x60\x60inner''}}\\,'"
    );
}

#[test]
fn csquotes_applies_to_nested_quotes_in_a_fragment() {
    let inner = Inline::Quoted(QuoteType::SingleQuote, vec![text("inner")]);
    let outer = Inline::Quoted(QuoteType::DoubleQuote, vec![inner]);
    let mut doc = document(vec![Block::Para(vec![outer])]);
    let mut options = options();
    doc.meta.insert("csquotes", MetaValue::MetaBool(false));
    let render = |doc: &Pandoc, options: &WriterOptions| {
        write_latex(doc, options, &mut Context::pure()).unwrap()
    };
    assert_eq!(render(&doc, &options), r"\enquote{\enquote*{inner}}");
    doc.meta.0.clear();
    options.variables.insert("csquotes", Val::Bool(true));
    assert_eq!(render(&doc, &options), r"\enquote{\enquote*{inner}}");
    options.variables.insert("csquotes", Val::text("false"));
    assert_eq!(render(&doc, &options), "\x60\x60\\,\x60inner'\\,''");
}

#[test]
fn ascii_option_escapes_latin_accents_and_dotless_i() {
    let mut options = options();
    options.prefer_ascii = true;
    let blocks = vec![paragraph("ëóıůẞ ñ Łódź İ")];
    assert_eq!(
        latex(blocks.clone(), &options),
        r#"\"{e}\'{o}\i\r{u}ẞ \~{n} \L\'{o}d\'{z} \.{I}"#
    );
    options.prefer_ascii = false;
    assert_eq!(latex(blocks, &options), "ëóıůẞ ñ Łódź İ");
}

#[test]
fn ascii_option_escapes_extended_latin_accents_and_symbols() {
    let mut options = options();
    options.prefer_ascii = true;
    assert_eq!(
        latex(
            vec![paragraph(
                "ǹ Ǎ ǎ ǰ ȁ ȩ ẽ ỳ ẁ ẅ Ḃ ḍ ṭ ḏ ả ȃ e\u{301} æe\u{301} € £ © ® ™"
            )],
            &options,
        ),
        r#"\`{n} \v{A} \v{a} \v{j} \G{a} \c{e} \~{e} \`{y} \`{w} \"{w} \.{B} \d{d} \d{t} \b{d} \h{a} \f{a} \'{e} \ae\'{e} \euro{} \pounds{} \copyright{} ® ™"#
    );
    let code = Inline::Code(Attr::default(), "ǎḍȁe\u{301}€£©".into());
    assert_eq!(
        latex(vec![Block::Para(vec![code])], &options),
        "\\texttt{a\u{30c}d\u{323}a\u{30f}e\u{301}\\euro{}\\pounds{}\\copyright{}}"
    );
}

#[test]
fn highlights_in_figure_and_table_captions_get_protect() {
    let code = Inline::Code(attr("", &["python"], &[]), "total = 8".into());
    let caption = vec![Block::Plain(vec![code.clone()])];
    let figure = Block::Figure(
        Attr::default(),
        Caption {
            short: None,
            long: caption.clone(),
        },
        vec![Block::Plain(vec![text("Figure contents")])],
    );
    let table = Block::Table(Box::new(sample_table(Attr::default(), caption)));
    let out = latex(vec![Block::Para(vec![code]), figure, table], &options());
    let highlight = r"\VERB|\NormalTok{total }\OperatorTok{=} \DecValTok{8}|";
    assert!(out.starts_with(highlight));
    assert_eq!(
        out.matches(&format!(r"\caption{{\protect{highlight}}}"))
            .count(),
        2
    );
}

#[test]
fn discarded_raw_nodes_are_reported_in_render_order() {
    use randoc_ast::Format;
    use randoc_core::LogMessage;
    let blocks = vec![
        Block::Header(
            1,
            Attr::default(),
            vec![
                text("topic"),
                Inline::RawInline(Format::new("html"), "<x>".into()),
                Inline::Note(vec![paragraph("note")]),
            ],
        ),
        Block::RawBlock(Format::new("markdown"), "x".into()),
    ];
    let mut context = Context::pure();
    write_latex(&document(blocks), &options(), &mut context).unwrap();
    assert_eq!(
        context.log(),
        [
            LogMessage::InlineNotRendered(r#"RawInline (Format "html") "<x>""#.into()),
            LogMessage::InlineNotRendered(r#"RawInline (Format "html") "<x>""#.into()),
            LogMessage::BlockNotRendered(r#"RawBlock (Format "markdown") "x""#.into()),
        ]
    );
}

#[test]
fn a_fragment_keeps_the_final_line_of_a_verbatim_environment() {
    assert_eq!(
        latex(
            vec![Block::CodeBlock(Attr::default(), "v".into())],
            &options()
        ),
        "\\begin{verbatim}\nv\n\\end{verbatim}\n"
    );
    assert_eq!(latex(vec![paragraph("v")], &options()), "v");
}

#[test]
fn multirow_content_uses_the_column_alignment() {
    use randoc_ast::Alignment;
    for (align, prefix) in [
        (Alignment::AlignCenter, "\\centering\\arraybackslash "),
        (Alignment::AlignRight, "\\raggedright\\arraybackslash "),
        (Alignment::AlignLeft, ""),
        (Alignment::AlignDefault, ""),
    ] {
        let mut table = sample_table(Attr::default(), vec![]);
        table.col_specs[0].align = align;
        table.bodies[0].body[0].cells[0].row_span = 2;
        table.bodies[0].body.push(row(vec![]));
        let output = latex(vec![Block::Table(Box::new(table))], &options());
        assert!(
            output.contains(&format!("\\multirow{{2}}{{*}}{{{prefix}24}}")),
            "{output}"
        );
    }
}
