//! The generic form of a Haskell value, and the conversion from the document tree.
//!
//! The writer turns the tree into a [`Value`] and prints it. The reader parses
//! text into a [`Value`] and decodes it (see `decode.rs`).

use randoc_ast::json::{show_double, show_string};
use randoc_ast::{
    Alignment, Attr, Block, Caption, Cell, Citation, CitationMode, ColSpec, ColWidth, Format,
    Inline, ListAttributes, ListNumberDelim, ListNumberStyle, MathType, Meta, MetaValue, Pandoc,
    QuoteType, Row, Table, TableBody, TableFoot, TableHead, Target, Text,
};

/// A Haskell value as `show` writes it and `read` reads it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    /// A constructor and its arguments, for example `Str "a"` or `Space`.
    Con(Text, Vec<Value>),
    /// A record: a constructor and its named fields, in declaration order.
    Rec(Text, Vec<(Text, Value)>),
    /// A list.
    List(Vec<Value>),
    /// A tuple.
    Tuple(Vec<Value>),
    /// A string. The text has no escapes.
    Str(Text),
    /// A character literal. Only the reader makes it, for a string as a list of characters.
    Char(char),
    /// A number. `digits` has no sign, for example `5`, `2.5`, `1.0e-3` or `Infinity`.
    Num { negative: bool, digits: Text },
}

impl Value {
    fn con(name: &str, args: Vec<Value>) -> Value {
        Value::Con(Text::from(name), args)
    }

    fn nullary(name: &str) -> Value {
        Value::con(name, Vec::new())
    }

    fn int(value: i64) -> Value {
        Value::Num {
            negative: value < 0,
            digits: value.unsigned_abs().to_string(),
        }
    }

    fn double(value: f64) -> Value {
        let shown = show_double(value);
        match shown.strip_prefix('-') {
            Some(digits) => Value::Num {
                negative: true,
                digits: Text::from(digits),
            },
            None => Value::Num {
                negative: false,
                digits: shown,
            },
        }
    }

    /// Returns `true` when the value needs no parentheses as a constructor argument.
    pub(crate) fn is_atom(&self) -> bool {
        match self {
            Value::Con(_, args) => args.is_empty(),
            Value::Num { negative, .. } => !negative,
            Value::Rec(..) | Value::List(_) | Value::Tuple(_) | Value::Str(_) | Value::Char(_) => {
                true
            }
        }
    }
}

/// Declares the constructor names of an enumeration in both directions.
macro_rules! enum_names {
    ($ty:ident, $to:ident, $from:ident: $($variant:ident),+ $(,)?) => {
        pub(crate) fn $to(value: $ty) -> &'static str {
            match value {
                $($ty::$variant => stringify!($variant),)+
            }
        }

        pub(crate) fn $from(name: &str) -> Option<$ty> {
            match name {
                $(stringify!($variant) => Some($ty::$variant),)+
                _ => None,
            }
        }
    };
}

enum_names!(ListNumberStyle, style_name, style_from: DefaultStyle, Example, Decimal, LowerRoman, UpperRoman, LowerAlpha, UpperAlpha);
enum_names!(ListNumberDelim, delim_name, delim_from: DefaultDelim, Period, OneParen, TwoParens);
enum_names!(QuoteType, quote_name, quote_from: SingleQuote, DoubleQuote);
enum_names!(MathType, math_name, math_from: DisplayMath, InlineMath);
enum_names!(CitationMode, mode_name, mode_from: AuthorInText, SuppressAuthor, NormalCitation);
enum_names!(Alignment, align_name, align_from: AlignLeft, AlignRight, AlignCenter, AlignDefault);

/// The tree is deeper than the nesting limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TooDeep;

type Built = Result<Value, TooDeep>;

/// Converts the document tree into a [`Value`], with a limit on the nesting depth.
pub(crate) struct Builder {
    max_depth: usize,
}

impl Builder {
    pub(crate) fn new(max_depth: usize) -> Self {
        Builder { max_depth }
    }

    fn check(&self, depth: usize) -> Result<usize, TooDeep> {
        if depth >= self.max_depth {
            Err(TooDeep)
        } else {
            Ok(depth + 1)
        }
    }

    pub(crate) fn pandoc(&self, doc: &Pandoc) -> Built {
        Ok(Value::con(
            "Pandoc",
            vec![self.meta(&doc.meta)?, self.blocks(&doc.blocks, 0)?],
        ))
    }

    fn meta(&self, meta: &Meta) -> Built {
        let map = self.meta_map(meta.0.iter(), 0)?;
        Ok(Value::Rec(
            Text::from("Meta"),
            vec![(Text::from("unMeta"), map)],
        ))
    }

    fn meta_map<'a>(
        &self,
        entries: impl Iterator<Item = (&'a Text, &'a MetaValue)>,
        depth: usize,
    ) -> Built {
        let mut pairs = Vec::new();
        for (key, value) in entries {
            pairs.push(Value::Tuple(vec![
                Value::Str(key.clone()),
                self.meta_value(value, depth)?,
            ]));
        }
        Ok(Value::con("fromList", vec![Value::List(pairs)]))
    }

    fn meta_value(&self, value: &MetaValue, depth: usize) -> Built {
        let depth = self.check(depth)?;
        Ok(match value {
            MetaValue::MetaMap(map) => {
                Value::con("MetaMap", vec![self.meta_map(map.iter(), depth)?])
            }
            MetaValue::MetaList(items) => {
                let items = items
                    .iter()
                    .map(|item| self.meta_value(item, depth))
                    .collect::<Result<_, _>>()?;
                Value::con("MetaList", vec![Value::List(items)])
            }
            MetaValue::MetaBool(flag) => Value::con("MetaBool", vec![bool_value(*flag)]),
            MetaValue::MetaString(text) => Value::con("MetaString", vec![Value::Str(text.clone())]),
            MetaValue::MetaInlines(inlines) => {
                Value::con("MetaInlines", vec![self.inlines(inlines, depth)?])
            }
            MetaValue::MetaBlocks(blocks) => {
                Value::con("MetaBlocks", vec![self.blocks(blocks, depth)?])
            }
        })
    }

    pub(crate) fn blocks(&self, blocks: &[Block], depth: usize) -> Built {
        blocks
            .iter()
            .map(|block| self.block(block, depth))
            .collect::<Result<_, _>>()
            .map(Value::List)
    }

    fn block_lists(&self, items: &[Vec<Block>], depth: usize) -> Built {
        items
            .iter()
            .map(|item| self.blocks(item, depth))
            .collect::<Result<_, _>>()
            .map(Value::List)
    }

    pub(crate) fn block(&self, block: &Block, depth: usize) -> Built {
        let depth = self.check(depth)?;
        Ok(match block {
            Block::Plain(inlines) => Value::con("Plain", vec![self.inlines(inlines, depth)?]),
            Block::Para(inlines) => Value::con("Para", vec![self.inlines(inlines, depth)?]),
            Block::LineBlock(lines) => {
                let lines = lines
                    .iter()
                    .map(|line| self.inlines(line, depth))
                    .collect::<Result<_, _>>()?;
                Value::con("LineBlock", vec![Value::List(lines)])
            }
            Block::CodeBlock(attr, text) => Value::con(
                "CodeBlock",
                vec![attr_value(attr), Value::Str(text.clone())],
            ),
            Block::RawBlock(format, text) => Value::con(
                "RawBlock",
                vec![format_value(format), Value::Str(text.clone())],
            ),
            Block::BlockQuote(blocks) => {
                Value::con("BlockQuote", vec![self.blocks(blocks, depth)?])
            }
            Block::OrderedList(attrs, items) => Value::con(
                "OrderedList",
                vec![list_attributes(attrs), self.block_lists(items, depth)?],
            ),
            Block::BulletList(items) => {
                Value::con("BulletList", vec![self.block_lists(items, depth)?])
            }
            Block::DefinitionList(items) => {
                let mut values = Vec::new();
                for (term, definitions) in items {
                    values.push(Value::Tuple(vec![
                        self.inlines(term, depth)?,
                        self.block_lists(definitions, depth)?,
                    ]));
                }
                Value::con("DefinitionList", vec![Value::List(values)])
            }
            Block::Header(level, attr, inlines) => Value::con(
                "Header",
                vec![
                    Value::int(*level),
                    attr_value(attr),
                    self.inlines(inlines, depth)?,
                ],
            ),
            Block::HorizontalRule => Value::nullary("HorizontalRule"),
            Block::Table(table) => self.table(table, depth)?,
            Block::Figure(attr, caption, blocks) => Value::con(
                "Figure",
                vec![
                    attr_value(attr),
                    self.caption(caption, depth)?,
                    self.blocks(blocks, depth)?,
                ],
            ),
            Block::Div(attr, blocks) => {
                Value::con("Div", vec![attr_value(attr), self.blocks(blocks, depth)?])
            }
        })
    }

    fn caption(&self, caption: &Caption, depth: usize) -> Built {
        let short = match &caption.short {
            Some(inlines) => Value::con("Just", vec![self.inlines(inlines, depth)?]),
            None => Value::nullary("Nothing"),
        };
        Ok(Value::con(
            "Caption",
            vec![short, self.blocks(&caption.long, depth)?],
        ))
    }

    fn table(&self, table: &Table, depth: usize) -> Built {
        let Table {
            attr,
            caption,
            col_specs,
            head,
            bodies,
            foot,
        } = table;
        let specs = col_specs.iter().map(col_spec).collect();
        let TableHead {
            attr: head_attr,
            rows: head_rows,
        } = head;
        let head = Value::con(
            "TableHead",
            vec![attr_value(head_attr), self.rows(head_rows, depth)?],
        );
        let mut body_values = Vec::new();
        for TableBody {
            attr,
            row_head_columns,
            head,
            body,
        } in bodies
        {
            body_values.push(Value::con(
                "TableBody",
                vec![
                    attr_value(attr),
                    Value::con("RowHeadColumns", vec![Value::int(*row_head_columns)]),
                    self.rows(head, depth)?,
                    self.rows(body, depth)?,
                ],
            ));
        }
        let TableFoot {
            attr: foot_attr,
            rows: foot_rows,
        } = foot;
        let foot = Value::con(
            "TableFoot",
            vec![attr_value(foot_attr), self.rows(foot_rows, depth)?],
        );
        Ok(Value::con(
            "Table",
            vec![
                attr_value(attr),
                self.caption(caption, depth)?,
                Value::List(specs),
                head,
                Value::List(body_values),
                foot,
            ],
        ))
    }

    fn rows(&self, rows: &[Row], depth: usize) -> Built {
        let mut values = Vec::new();
        for Row { attr, cells } in rows {
            let mut cell_values = Vec::new();
            for Cell {
                attr,
                align,
                row_span,
                col_span,
                content,
            } in cells
            {
                cell_values.push(Value::con(
                    "Cell",
                    vec![
                        attr_value(attr),
                        Value::nullary(align_name(*align)),
                        Value::con("RowSpan", vec![Value::int(*row_span)]),
                        Value::con("ColSpan", vec![Value::int(*col_span)]),
                        self.blocks(content, depth)?,
                    ],
                ));
            }
            values.push(Value::con(
                "Row",
                vec![attr_value(attr), Value::List(cell_values)],
            ));
        }
        Ok(Value::List(values))
    }

    pub(crate) fn inlines(&self, inlines: &[Inline], depth: usize) -> Built {
        inlines
            .iter()
            .map(|inline| self.inline(inline, depth))
            .collect::<Result<_, _>>()
            .map(Value::List)
    }

    pub(crate) fn inline(&self, inline: &Inline, depth: usize) -> Built {
        let depth = self.check(depth)?;
        let wrap = |name: &str, inlines: &[Inline]| -> Built {
            Ok(Value::con(name, vec![self.inlines(inlines, depth)?]))
        };
        Ok(match inline {
            Inline::Str(text) => Value::con("Str", vec![Value::Str(text.clone())]),
            Inline::Emph(inlines) => wrap("Emph", inlines)?,
            Inline::Underline(inlines) => wrap("Underline", inlines)?,
            Inline::Strong(inlines) => wrap("Strong", inlines)?,
            Inline::Strikeout(inlines) => wrap("Strikeout", inlines)?,
            Inline::Superscript(inlines) => wrap("Superscript", inlines)?,
            Inline::Subscript(inlines) => wrap("Subscript", inlines)?,
            Inline::SmallCaps(inlines) => wrap("SmallCaps", inlines)?,
            Inline::Quoted(kind, inlines) => Value::con(
                "Quoted",
                vec![
                    Value::nullary(quote_name(*kind)),
                    self.inlines(inlines, depth)?,
                ],
            ),
            Inline::Cite(citations, inlines) => {
                let citations = citations
                    .iter()
                    .map(|citation| self.citation(citation, depth))
                    .collect::<Result<_, _>>()?;
                Value::con(
                    "Cite",
                    vec![Value::List(citations), self.inlines(inlines, depth)?],
                )
            }
            Inline::Code(attr, text) => {
                Value::con("Code", vec![attr_value(attr), Value::Str(text.clone())])
            }
            Inline::Space => Value::nullary("Space"),
            Inline::SoftBreak => Value::nullary("SoftBreak"),
            Inline::LineBreak => Value::nullary("LineBreak"),
            Inline::Math(kind, text) => Value::con(
                "Math",
                vec![Value::nullary(math_name(*kind)), Value::Str(text.clone())],
            ),
            Inline::RawInline(format, text) => Value::con(
                "RawInline",
                vec![format_value(format), Value::Str(text.clone())],
            ),
            Inline::Link(attr, inlines, target) => Value::con(
                "Link",
                vec![
                    attr_value(attr),
                    self.inlines(inlines, depth)?,
                    target_value(target),
                ],
            ),
            Inline::Image(attr, inlines, target) => Value::con(
                "Image",
                vec![
                    attr_value(attr),
                    self.inlines(inlines, depth)?,
                    target_value(target),
                ],
            ),
            Inline::Note(blocks) => Value::con("Note", vec![self.blocks(blocks, depth)?]),
            Inline::Span(attr, inlines) => Value::con(
                "Span",
                vec![attr_value(attr), self.inlines(inlines, depth)?],
            ),
        })
    }

    fn citation(&self, citation: &Citation, depth: usize) -> Built {
        let field = |name: &str, value: Value| (Text::from(name), value);
        Ok(Value::Rec(
            Text::from("Citation"),
            vec![
                field("citationId", Value::Str(citation.id.clone())),
                field("citationPrefix", self.inlines(&citation.prefix, depth)?),
                field("citationSuffix", self.inlines(&citation.suffix, depth)?),
                field("citationMode", Value::nullary(mode_name(citation.mode))),
                field("citationNoteNum", Value::int(citation.note_num)),
                field("citationHash", Value::int(citation.hash)),
            ],
        ))
    }
}

fn bool_value(flag: bool) -> Value {
    Value::nullary(if flag { "True" } else { "False" })
}

fn attr_value(attr: &Attr) -> Value {
    Value::Tuple(vec![
        Value::Str(attr.id.clone()),
        Value::List(attr.classes.iter().cloned().map(Value::Str).collect()),
        Value::List(
            attr.attributes
                .iter()
                .map(|(key, value)| {
                    Value::Tuple(vec![Value::Str(key.clone()), Value::Str(value.clone())])
                })
                .collect(),
        ),
    ])
}

fn format_value(format: &Format) -> Value {
    Value::con("Format", vec![Value::Str(format.0.clone())])
}

fn target_value(target: &Target) -> Value {
    Value::Tuple(vec![
        Value::Str(target.url.clone()),
        Value::Str(target.title.clone()),
    ])
}

fn list_attributes(attrs: &ListAttributes) -> Value {
    Value::Tuple(vec![
        Value::int(attrs.start),
        Value::nullary(style_name(attrs.style)),
        Value::nullary(delim_name(attrs.delim)),
    ])
}

fn col_spec(spec: &ColSpec) -> Value {
    let width = match spec.width {
        ColWidth::ColWidth(width) => Value::con("ColWidth", vec![Value::double(width)]),
        ColWidth::ColWidthDefault => Value::nullary("ColWidthDefault"),
    };
    Value::Tuple(vec![Value::nullary(align_name(spec.align)), width])
}

/// Writes a value on one line, as the Haskell function `show` does.
pub(crate) fn show_value(value: &Value, in_argument: bool, out: &mut String) {
    match value {
        Value::Con(name, args) if args.is_empty() => out.push_str(name),
        Value::Con(name, args) => {
            parenthesize(in_argument, out, |out| {
                out.push_str(name);
                for arg in args {
                    out.push(' ');
                    show_value(arg, true, out);
                }
            });
        }
        Value::Rec(name, fields) => {
            parenthesize(in_argument, out, |out| {
                out.push_str(name);
                out.push_str(" {");
                for (index, (field, value)) in fields.iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(field);
                    out.push_str(" = ");
                    show_value(value, false, out);
                }
                out.push('}');
            });
        }
        Value::List(items) => show_sequence('[', ']', items, out),
        Value::Tuple(items) => show_sequence('(', ')', items, out),
        Value::Str(text) => out.push_str(&show_string(text)),
        Value::Char(c) => out.push_str(&show_string(&c.to_string())),
        Value::Num { negative, digits } => {
            parenthesize(in_argument && *negative, out, |out| {
                if *negative {
                    out.push('-');
                }
                out.push_str(digits);
            });
        }
    }
}

fn parenthesize(needed: bool, out: &mut String, body: impl FnOnce(&mut String)) {
    if needed {
        out.push('(');
    }
    body(out);
    if needed {
        out.push(')');
    }
}

fn show_sequence(open: char, close: char, items: &[Value], out: &mut String) {
    out.push(open);
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        show_value(item, false, out);
    }
    out.push(close);
}
