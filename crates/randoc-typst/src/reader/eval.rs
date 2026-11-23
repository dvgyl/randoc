use std::collections::BTreeMap;
use std::ops::Deref;
use std::rc::Rc;

use super::lower::{self, Flow, Item, finish, inline_text, wrap};
use super::parser::{self, Arg, Expr, Kind, Mark};
use super::*;

pub(super) type Scope<'a> = BTreeMap<Rc<str>, Value<'a>>;
type Args<'a> = (Vec<Value<'a>>, Scope<'a>);
type EvalResult<'a, T> = std::result::Result<T, Interrupt<'a>>;

enum Interrupt<'a> {
    Error(Error),
    Control(Control, Value<'a>, usize),
}

impl From<Error> for Interrupt<'_> {
    fn from(error: Error) -> Self {
        Self::Error(error)
    }
}

#[derive(Clone, Copy)]
enum Control {
    Break,
    Continue,
    Return,
    ReturnValue,
}

#[derive(Clone, Default)]
struct Bindings<'a> {
    // Each level stores up to twice as many writes as the previous level.
    // A snapshot shares the maps. A write merges only the occupied low levels.
    levels: Vec<Option<Rc<Scope<'a>>>>,
}

impl<'a> Bindings<'a> {
    fn get(&self, name: &str) -> Option<&Value<'a>> {
        self.levels.iter().flatten().find_map(|map| map.get(name))
    }

    fn insert(&mut self, name: &str, value: Value<'a>, fuel: &mut usize) -> Option<()> {
        let merge_cost = self
            .levels
            .iter()
            .map_while(Option::as_ref)
            .map(|map| map.len())
            .sum();
        *fuel = fuel.checked_sub(merge_cost)?;
        let mut pending = Scope::from([(Rc::from(name), value)]);
        for level in &mut self.levels {
            let Some(older) = level.take() else {
                *level = Some(Rc::new(pending));
                return Some(());
            };
            let mut older = Rc::unwrap_or_clone(older);
            older.extend(pending);
            pending = older;
        }
        self.levels.push(Some(Rc::new(pending)));
        Some(())
    }
}

pub(super) struct Measured<T> {
    data: T,
    depth: usize,
}

impl<T> Deref for Measured<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.data
    }
}

pub(super) struct Dictionary<'a> {
    items: Scope<'a>,
    order: Vec<Rc<str>>,
}

impl<'a> Deref for Dictionary<'a> {
    type Target = Scope<'a>;

    fn deref(&self) -> &Self::Target {
        &self.items
    }
}

#[derive(Clone)]
pub(super) enum Value<'a> {
    None,
    Bool(bool),
    Number(f64, &'a str),
    Float(f64),
    Text(Rc<str>),
    Label(&'a str),
    Array(Rc<Measured<Vec<Value<'a>>>>),
    Dict(Rc<Measured<Dictionary<'a>>>),
    Content(Rc<Flow>),
    Builtin(String),
    Function(Rc<Function<'a>>),
    Element(Rc<Measured<Element<'a>>>),
}

pub(super) struct Element<'a> {
    pub name: String,
    pub pos: Vec<Value<'a>>,
    pub named: Scope<'a>,
}

pub(super) struct Function<'a> {
    params: &'a Expr<'a>,
    body: &'a Expr<'a>,
    scope: Vec<Bindings<'a>>,
    name: Option<&'a str>,
}

impl Drop for Function<'_> {
    fn drop(&mut self) {
        // A later closure can hold each earlier closure through its snapshot.
        // Release unique references in a loop to keep that chain off the stack.
        let mut scopes = std::mem::take(&mut self.scope);
        let mut values = Vec::new();
        loop {
            if let Some(scope) = scopes.pop() {
                for map in scope
                    .levels
                    .into_iter()
                    .flatten()
                    .filter_map(Rc::into_inner)
                {
                    values.extend(map.into_values());
                }
            } else if let Some(value) = values.pop() {
                match value {
                    Value::Function(function) => {
                        if let Some(mut function) = Rc::into_inner(function) {
                            scopes.append(&mut function.scope);
                        }
                    }
                    Value::Array(items) => {
                        if let Some(items) = Rc::into_inner(items) {
                            values.extend(items.data);
                        }
                    }
                    Value::Dict(items) => {
                        if let Some(items) = Rc::into_inner(items) {
                            values.extend(items.data.items.into_values());
                        }
                    }
                    Value::Element(element) => {
                        if let Some(element) = Rc::into_inner(element) {
                            values.extend(element.data.pos);
                            values.extend(element.data.named.into_values());
                        }
                    }
                    _ => {}
                }
            } else {
                break;
            }
        }
    }
}

#[derive(Clone)]
struct Rule<'a> {
    selector: Rc<str>,
    transform: Value<'a>,
}

struct Budget {
    active: Vec<String>,
    fuel: usize,
    expansion: usize,
}

pub(super) struct Evaluator<'a, 'c> {
    source: &'a str,
    path: &'a str,
    budget: &'c mut Budget,
    options: &'c ReaderOptions,
    pub context: &'c mut Context,
    pub meta: Meta,
    pub settings: Rc<BTreeMap<Rc<str>, Rc<Scope<'a>>>>,
    scopes: Vec<Bindings<'a>>,
    rules: Vec<Rule<'a>>,
    max_depth: usize,
}

pub(super) fn evaluate<'a>(
    source: &'a str,
    path: &'a str,
    marks: &'a [Mark<'a>],
    depth: usize,
    options: &ReaderOptions,
    context: &mut Context,
) -> Result<Pandoc> {
    let mut budget = Budget {
        active: vec![path.to_owned()],
        fuel: 1_000_000,
        expansion: (16 * 1024 * 1024).max(source.len().saturating_mul(16)),
    };
    let (flow, meta) = evaluate_file(source, path, marks, depth, options, context, &mut budget)?;
    Ok(Pandoc {
        meta,
        blocks: finish(flow),
    })
}

fn evaluate_file<'a>(
    source: &'a str,
    path: &'a str,
    marks: &'a [Mark<'a>],
    depth: usize,
    options: &ReaderOptions,
    context: &mut Context,
    budget: &mut Budget,
) -> Result<(Flow, Meta)> {
    let mut eval = Evaluator {
        source,
        path,
        budget,
        options,
        context,
        meta: Meta::default(),
        settings: Rc::default(),
        scopes: vec![Bindings::default()],
        rules: Vec::new(),
        max_depth: depth,
    };
    let flow = eval
        .markup(marks, depth)
        .map_err(|interrupt| match interrupt {
            Interrupt::Error(error) => error,
            Interrupt::Control(_, _, pos) => eval.fail(pos, "Unexpected Typst control statement."),
        })?;
    if flow.depth() > depth {
        return Err(limit_error(0, source));
    }
    Ok((flow, eval.meta))
}

impl<'a> Value<'a> {
    fn text(&self) -> String {
        match self {
            Self::Bool(b) => b.to_string(),
            Self::Number(n, unit) => format!("{n}{unit}"),
            Self::Float(n) if *n == f64::INFINITY => "Infinity".into(),
            Self::Float(n) if *n == f64::NEG_INFINITY => "-Infinity".into(),
            Self::Float(n) => format!("{n:?}"),
            Self::Text(text) => text.to_string(),
            Self::Label(id) => (*id).into(),
            Self::Content(flow) => randoc_core::shared::stringify_blocks(&finish((**flow).clone())),
            Self::Builtin(name) => name.clone(),
            _ => String::new(),
        }
    }

    fn depth(&self) -> usize {
        match self {
            Self::Array(items) => items.depth,
            Self::Dict(items) => items.depth,
            Self::Element(element) => element.depth,
            Self::Content(flow) => flow.depth().max(1),
            _ => 1,
        }
    }

    fn array(data: Vec<Self>) -> Self {
        let depth = child_depth(data.iter());
        Self::Array(Rc::new(Measured { data, depth }))
    }

    fn dict(items: Scope<'a>, order: Vec<Rc<str>>) -> Self {
        let depth = child_depth(items.values());
        Self::Dict(Rc::new(Measured {
            data: Dictionary { items, order },
            depth,
        }))
    }

    fn element(name: &str, pos: Vec<Self>, named: Scope<'a>) -> Self {
        let depth = child_depth(pos.iter().chain(named.values()));
        let data = Element {
            name: name.into(),
            pos,
            named,
        };
        Self::Element(Rc::new(Measured { data, depth }))
    }

    pub fn number(&self) -> Option<f64> {
        self.numeric().map(|(number, _, _)| number)
    }

    fn numeric(&self) -> Option<(f64, &'a str, bool)> {
        match self {
            Self::Number(number, unit) => Some((*number, unit, false)),
            Self::Float(number) => Some((*number, "", true)),
            _ => None,
        }
    }

    fn truth(&self) -> bool {
        matches!(self, Self::Bool(true))
    }

    fn weight(&self) -> usize {
        match self {
            Self::Text(text) => text.len(),
            Self::Array(items) => items.len().saturating_mul(32),
            Self::Dict(items) => items.len().saturating_mul(64),
            Self::Content(content) => content.weight(),
            Self::Label(id) => id.len(),
            Self::Builtin(name) => name.len(),
            _ => 32,
        }
    }
}

fn child_depth<'v, 'a: 'v>(values: impl Iterator<Item = &'v Value<'a>>) -> usize {
    1 + values.map(Value::depth).max().unwrap_or(0)
}

impl<'a> Evaluator<'a, '_> {
    fn new_content(&mut self, flow: Flow, pos: usize) -> Result<Value<'a>> {
        if flow.depth() > self.max_depth {
            return Err(limit_error(pos, self.source));
        }
        Ok(Value::Content(Rc::new(flow)))
    }

    pub(super) fn text(&mut self, value: &Value<'a>, pos: usize) -> Result<String> {
        let weight = value.weight();
        self.work(weight.div_ceil(64), pos)?;
        if matches!(
            value,
            Value::Content(_) | Value::Text(_) | Value::Label(_) | Value::Builtin(_)
        ) {
            self.spend(weight, pos)?;
        }
        Ok(value.text())
    }

    pub(super) fn work(&mut self, cost: usize, pos: usize) -> Result<()> {
        self.budget.fuel = self
            .budget
            .fuel
            .checked_sub(cost)
            .ok_or_else(|| limit_error(pos, self.source))?;
        Ok(())
    }

    fn rule(
        &mut self,
        pos: usize,
        cost: usize,
        matches: impl Fn(&Rule<'a>) -> bool,
    ) -> Result<Option<Rule<'a>>> {
        for index in (0..self.rules.len()).rev() {
            self.work(cost, pos)?;
            if matches(&self.rules[index]) {
                return Ok(Some(self.rules[index].clone()));
            }
        }
        Ok(None)
    }

    pub fn fail(&self, pos: usize, message: &str) -> Error {
        parser::with_source_name(parser::error(self.source, pos, message), self.path)
    }

    fn step(&mut self, pos: usize, depth: usize) -> Result<()> {
        if self.budget.fuel == 0 || depth == 0 {
            return Err(limit_error(pos, self.source));
        }
        self.budget.fuel -= 1;
        Ok(())
    }

    pub fn spend(&mut self, value: usize, pos: usize) -> Result<()> {
        self.budget.expansion = self
            .budget
            .expansion
            .checked_sub(value)
            .ok_or_else(|| limit_error(pos, self.source))?;
        Ok(())
    }

    fn markup(&mut self, marks: &'a [Mark<'a>], depth: usize) -> EvalResult<'a, Flow> {
        if depth == 0 {
            return Err(limit_error(0, self.source).into());
        }
        let mut out = Flow::new();
        let mut index = 0;
        while let Some(mark) = marks.get(index) {
            self.step(0, depth)?;
            index += 1;
            let result = if let Mark::Expression(expr) = mark {
                if let Kind::Show(None, transform) = &expr.kind {
                    self.show_rest(transform, &marks[index..], &mut out, depth, expr.pos)
                        .map(|()| marks.len())
                } else {
                    self.markup_expr(expr, marks.get(index), &mut out, depth)
                        .map(|used_label| index + usize::from(used_label))
                }
            } else {
                self.mark(mark, marks, index, &mut out, depth)
            };
            match result {
                Ok(next) => index = next,
                Err(Interrupt::Control(control, value, pos))
                    if !matches!(control, Control::ReturnValue) =>
                {
                    out.append(self.content(value, depth, pos)?);
                    let value = self.new_content(out, pos)?;
                    return Err(Interrupt::Control(control, value, pos));
                }
                Err(interrupt) => return Err(interrupt),
            }
        }
        if self
            .options
            .extensions
            .contains(Extension::EastAsianLineBreaks)
        {
            lower::remove_east_asian_breaks(&mut out);
        }
        Ok(out)
    }

    fn mark(
        &mut self,
        mark: &'a Mark<'a>,
        marks: &'a [Mark<'a>],
        mut index: usize,
        out: &mut Flow,
        depth: usize,
    ) -> EvalResult<'a, usize> {
        match mark {
            Mark::Text(text) => self.markup_text(text, out, depth)?,
            Mark::Escape(c) => out.push(Item::Inline(Inline::Str(c.to_string()))),
            Mark::Space => out.push(Item::Inline(Inline::Space)),
            Mark::Break(_) => out.push(Item::Inline(Inline::SoftBreak)),
            Mark::Paragraph(_) => out.push(Item::Paragraph),
            Mark::Styled(name, body) => {
                let flow = self.markup(body, depth - 1)?;
                if *name == "linebreak" {
                    out.push(Item::Inline(Inline::LineBreak));
                } else {
                    out.append(wrap(flow, |xs| lower::style(name, xs)));
                }
            }
            Mark::Heading(level, title, id) => {
                if self.markup_heading(*level, title, id, out, depth)?
                    && matches!(marks.get(index), Some(Mark::Break(_)))
                {
                    index += 1;
                }
            }
            Mark::Label(id, _) => {
                let anchor = Inline::Span(Attr::new(*id, vec![], vec![]), vec![]);
                if matches!(out.last(), Some(Item::Block(Block::Header(..)))) {
                    out.push(Item::Block(Block::Para(vec![anchor])));
                } else {
                    out.push(Item::Inline(anchor));
                }
            }
            Mark::Reference(id, body) => {
                let mut body = if body.is_empty() {
                    Vec::new()
                } else {
                    lower::inlines(self.markup(body, depth - 1)?, true)
                };
                if body.is_empty() {
                    body.push(Inline::Str(format!("[{id}]")));
                }
                out.push(Item::Inline(lower::reference(id, body)));
            }
            Mark::Raw(lang, code, block) => out.push(self.raw(lang, code, *block, 0)?),
            Mark::Math(source, pos) => out.push(self.equation(source, depth, *pos)?),
            Mark::Link(url) => {
                let value = self.call_builtin(
                    "link",
                    vec![Value::Text(Rc::from(*url))],
                    Scope::new(),
                    depth,
                    0,
                )?;
                out.append(self.content(value, depth, 0)?);
            }
            Mark::List(marker, indent, contents) => {
                let (block, end) = self.list(marks, index, marker, *indent, contents, depth)?;
                out.push(Item::Block(block));
                index = end;
            }
            Mark::Term(indent, term, definition) => {
                let term = lower::inlines(self.markup(term, depth - 1)?, true);
                let mut definition = self.markup(definition, depth - 1)?;
                let end = continuation(marks, index, *indent);
                definition.append(self.markup(&marks[index..end], depth - 1)?);
                index = end;
                let definition = finish(definition);
                if matches!(out.last(), Some(Item::Inline(Inline::SoftBreak))) {
                    out.pop();
                }
                out.definition(term, definition);
            }
            Mark::Expression(_) => {}
        }
        Ok(index)
    }

    fn show_rest(
        &mut self,
        transform: &'a Expr<'a>,
        marks: &'a [Mark<'a>],
        out: &mut Flow,
        depth: usize,
        pos: usize,
    ) -> EvalResult<'a, ()> {
        let transform = self.eval(transform, depth)?;
        let flow = self.markup(marks, depth - 1)?;
        let value = self.new_content(flow, pos)?;
        let shown = self.transform(transform, value, depth, pos)?;
        out.append(self.content(shown, depth, pos)?);
        Ok(())
    }

    fn markup_expr(
        &mut self,
        expr: &'a Expr<'a>,
        next: Option<&Mark<'a>>,
        out: &mut Flow,
        depth: usize,
    ) -> EvalResult<'a, bool> {
        let value = self.eval(expr, depth)?;
        let mut body = self.content(value, depth, expr.pos)?;
        if let Some(Mark::Label(id, true)) = next
            && body.attach_label(id)
        {
            out.append(body);
            return Ok(true);
        }
        out.append(body);
        Ok(false)
    }

    fn markup_text(&mut self, text: &str, out: &mut Flow, depth: usize) -> EvalResult<'a, ()> {
        self.spend(text.len(), 0)?;
        self.work(text.len() / 64, 0)?;
        let mut text = text
            .replace("---", "—")
            .replace("--", "–")
            .replace("...", "…")
            .replace('~', "\u{a0}");
        if self
            .settings
            .get("smartquote")
            .and_then(|s| s.get("enabled"))
            .is_none_or(Value::truth)
        {
            text = smart_quotes(
                &text,
                !matches!(
                    out.last(),
                    None | Some(Item::Inline(Inline::Space | Inline::SoftBreak))
                        | Some(Item::Paragraph)
                ),
            );
        }
        if let Some(rule) = self.rule(0, 1 + text.len() / 64, |rule| {
            rule.selector.len() <= text.len() && text.contains(rule.selector.as_ref())
        })? {
            for (i, part) in text.split(rule.selector.as_ref()).enumerate() {
                if i > 0 {
                    let value = self.transform(
                        rule.transform.clone(),
                        Value::Text(rule.selector.clone()),
                        depth,
                        0,
                    )?;
                    out.append(self.content(value, depth, 0)?);
                }
                out.append(inline_text(part));
            }
        } else {
            out.append(inline_text(&text));
        }
        Ok(())
    }

    fn markup_heading(
        &mut self,
        level: i64,
        title: &'a [Mark<'a>],
        id: &str,
        out: &mut Flow,
        depth: usize,
    ) -> EvalResult<'a, bool> {
        let title = self.markup(title, depth - 1)?;
        let heading = Item::Block(Block::Header(
            level,
            Attr::new(id, vec![], vec![]),
            lower::inlines(title, true),
        ));
        let selector = format!("heading:{level}");
        if let Some(rule) = self.rule(0, 1, |rule| {
            rule.selector.as_ref() == "heading" || rule.selector.as_ref() == selector
        })? {
            let value = self.new_content(Flow::from([heading]), 0)?;
            let changed = self.transform(rule.transform, value, depth, 0)?;
            let body = self.content(changed, depth, 0)?;
            out.append(body);
            return Ok(true);
        } else {
            out.push(heading);
        }
        Ok(false)
    }

    fn list(
        &mut self,
        marks: &'a [Mark<'a>],
        mut index: usize,
        marker: &str,
        indent: usize,
        contents: &'a [Mark<'a>],
        depth: usize,
    ) -> EvalResult<'a, (Block, usize)> {
        self.step(0, depth)?;
        let mut items = vec![finish(self.markup(contents, depth - 1)?)];
        while let Some((at, next, margin, body)) = next_list(marks, index) {
            if margin < indent || is_bullet(marker) != is_bullet(next) && margin == indent {
                break;
            }
            if margin > indent {
                let (nested, end) = self.list(marks, at + 1, next, margin, body, depth - 1)?;
                if let Some(last) = items.last_mut() {
                    last.push(nested);
                }
                index = end;
            } else {
                items.push(finish(self.markup(body, depth - 1)?));
                index = at + 1;
            }
        }
        let block = if is_bullet(marker) {
            Block::BulletList(items)
        } else {
            let settings = self.settings.get("enum").cloned();
            let mut attr = self.list_attr(None, settings.as_deref(), 0)?;
            if let Some(n) = marker.strip_suffix('.').and_then(|n| n.parse().ok()) {
                attr.start = n;
            }
            Block::OrderedList(attr, items)
        };
        Ok((block, index))
    }

    fn raw(&mut self, lang: &str, code: &str, block: bool, pos: usize) -> Result<Item> {
        let len = lang.len().saturating_add(code.len());
        self.spend(len, pos)?;
        self.work(len / 64, pos)?;
        Ok(lower::raw(lang, code, block))
    }

    fn equation(&mut self, source: &str, depth: usize, pos: usize) -> Result<Item> {
        let kind =
            if source.starts_with(char::is_whitespace) && source.ends_with(char::is_whitespace) {
                MathType::DisplayMath
            } else {
                MathType::InlineMath
            };
        let tex = math::convert(source, depth, &mut |name| {
            let Some(value) = self.lookup(name, pos)? else {
                return Ok(None);
            };
            let text = if let Value::Content(content) = value {
                self.spend(content.weight().saturating_add(2), pos)?;
                self.work(content.weight().div_ceil(64), pos)?;
                let mut text = String::from("{");
                for inline in lower::inlines(content.as_ref().clone(), true) {
                    if let Inline::Math(_, tex) = inline {
                        text.push_str(&tex);
                    }
                }
                text.push('}');
                text
            } else {
                self.text(&value, pos)?
            };
            Ok(Some(text))
        })
        .map_err(|error| match error {
            math::Error::Depth => self.fail(pos, "The Typst equation is nested too deeply."),
            math::Error::Incomplete => {
                self.fail(pos + source.len() + 1, "The Typst equation is incomplete.")
            }
            math::Error::Evaluation(error) => error,
        })?;
        Ok(Item::Inline(Inline::Math(kind, tex)))
    }

    fn lookup(&mut self, name: &str, pos: usize) -> Result<Option<Value<'a>>> {
        for index in (0..self.scopes.len()).rev() {
            let cost = 1 + self.scopes[index].levels.len() / 4 + name.len() / 64;
            self.work(cost, pos)?;
            if let Some(value) = self.scopes[index].get(name) {
                return Ok(Some(value.clone()));
            }
        }
        Ok(None)
    }

    fn define(&mut self, name: &str, value: Value<'a>, pos: usize) -> Result<()> {
        self.work(name.len().div_ceil(64), pos)?;
        if let Some(scope) = self.scopes.last_mut() {
            scope
                .insert(name, value, &mut self.budget.fuel)
                .ok_or_else(|| limit_error(pos, self.source))?;
        }
        Ok(())
    }

    fn eval(&mut self, expr: &'a Expr<'a>, depth: usize) -> EvalResult<'a, Value<'a>> {
        let pos = expr.pos;
        self.step(pos, depth)?;
        let value = match &expr.kind {
            Kind::Call(callee, args) => self.eval_call(callee, args, depth, pos),
            Kind::Content(marks) => self.eval_content(marks, depth, pos),
            Kind::Binary(left, operators) => self.chain(left, operators, depth, pos),
            Kind::If(condition, then, otherwise) => {
                self.eval_if(condition, then, otherwise.as_deref(), depth)
            }
            Kind::Block(exprs) => self.eval_block(exprs, depth, pos),
            Kind::While(cond, body) => self.eval_while(cond, body, depth, pos),
            Kind::For(pattern, items, body) => self.eval_for(pattern, items, body, depth, pos),
            kind => self.eval_expression(kind, depth, pos),
        }?;
        if value.depth() > self.max_depth {
            return Err(limit_error(pos, self.source).into());
        }
        Ok(value)
    }

    fn eval_expression(
        &mut self,
        kind: &'a Kind<'a>,
        depth: usize,
        pos: usize,
    ) -> EvalResult<'a, Value<'a>> {
        Ok(match kind {
            Kind::Literal(text) => self.literal(text, pos)?,
            Kind::Name(name) => {
                if let Some(value) = self.lookup(name, pos)? {
                    value
                } else {
                    match *name {
                        "none" => Value::None,
                        "true" => Value::Bool(true),
                        "false" => Value::Bool(false),
                        _ if builtin(name) => Value::Builtin((*name).into()),
                        _ => {
                            return Err(self
                                .fail(pos, &format!("Identifier {name:?} not found"))
                                .into());
                        }
                    }
                }
            }
            Kind::Field(base, fields) => self.fields(base, fields, depth, pos)?,
            Kind::Args(args) => {
                let mut order = Vec::new();
                let (values, named) = self.args(args, depth, Some(&mut order))?;
                if named.is_empty() {
                    Value::array(values)
                } else {
                    Value::dict(named, order)
                }
            }
            Kind::Raw(lang, code, block) => {
                let raw = self.raw(lang, code, *block, pos)?;
                self.new_content(Flow::from([raw]), pos)?
            }
            Kind::Math(source) => {
                let equation = self.equation(source, depth, pos)?;
                self.new_content(Flow::from([equation]), pos)?
            }
            Kind::Let(binding, value) => self.eval_let(binding, value.as_deref(), depth)?,
            Kind::Closure(params, body) => Value::Function(Rc::new(Function {
                params,
                body,
                scope: self.scopes.clone(),
                name: None,
            })),
            Kind::Unary(op, target) => {
                let value = self.eval(target, depth)?;
                match (*op, value) {
                    ("not", value) => Value::Bool(!value.truth()),
                    ("-", Value::Number(n, unit)) => Value::Number(-n, unit),
                    ("-", Value::Float(n)) => Value::Float(-n),
                    (_, value) => value,
                }
            }
            Kind::Break => return Err(Interrupt::Control(Control::Break, Value::None, pos)),
            Kind::Continue => return Err(Interrupt::Control(Control::Continue, Value::None, pos)),
            Kind::Return(expr) => {
                let (control, value) = if let Some(expr) = expr {
                    (Control::ReturnValue, self.eval(expr, depth)?)
                } else {
                    (Control::Return, Value::None)
                };
                return Err(Interrupt::Control(control, value, pos));
            }
            Kind::Set(rule, condition) => self.eval_set(rule, condition.as_deref(), depth, pos)?,
            Kind::Show(Some(selector), transform) => {
                let selector = self.eval(selector, depth)?;
                let selector = self.text(&selector, pos)?;
                let transform = self.eval(transform, depth)?;
                if !selector.is_empty() && !matches!(transform, Value::None) {
                    self.rules.push(Rule {
                        selector: selector.into(),
                        transform,
                    });
                }
                Value::None
            }
            Kind::Show(None, _)
            | Kind::Call(..)
            | Kind::Content(_)
            | Kind::Binary(..)
            | Kind::If(..)
            | Kind::Block(_)
            | Kind::While(..)
            | Kind::For(..) => Value::None,
            Kind::Include(path) => {
                let path = self.eval(path, depth)?;
                let path = self.text(&path, pos)?;
                let flow = self.include(&path, depth, pos)?;
                self.new_content(flow, pos)?
            }
            Kind::Import(path, names) => {
                let path = self.eval(path, depth)?;
                let path = self.text(&path, pos)?;
                self.include(&path, depth, pos)?;
                if !names.is_empty() {
                    return Err(self
                        .fail(pos, "Typst import bindings are not supported.")
                        .into());
                }
                Value::None
            }
        })
    }

    fn eval_if(
        &mut self,
        condition: &'a Expr<'a>,
        then: &'a Expr<'a>,
        otherwise: Option<&'a Expr<'a>>,
        depth: usize,
    ) -> EvalResult<'a, Value<'a>> {
        if self.eval(condition, depth)?.truth() {
            self.eval(then, depth)
        } else if let Some(otherwise) = otherwise {
            self.eval(otherwise, depth)
        } else {
            Ok(Value::None)
        }
    }

    fn eval_while(
        &mut self,
        cond: &'a Expr<'a>,
        body: &'a Expr<'a>,
        depth: usize,
        pos: usize,
    ) -> EvalResult<'a, Value<'a>> {
        let mut out = Flow::new();
        while self.eval(cond, depth)?.truth() {
            self.step(pos, depth)?;
            if !self.loop_body(body, &mut out, depth, pos)? {
                break;
            }
        }
        self.new_content(out, pos).map_err(Interrupt::from)
    }

    fn eval_set(
        &mut self,
        rule: &'a Expr<'a>,
        condition: Option<&'a Expr<'a>>,
        depth: usize,
        pos: usize,
    ) -> EvalResult<'a, Value<'a>> {
        if condition.map_or(Ok(true), |condition| {
            self.eval(condition, depth).map(|v| v.truth())
        })? && let Kind::Call(callee, args) = &rule.kind
        {
            let value = self.eval(callee, depth)?;
            let name = self.text(&value, pos)?;
            let (_, named) = self.args(args, depth, None)?;
            if name == "document" {
                self.document(&named, depth, pos)?;
            } else {
                self.set(name, named, pos)?;
            }
        }
        Ok(Value::None)
    }

    fn eval_for(
        &mut self,
        pattern: &'a Expr<'a>,
        items: &'a Expr<'a>,
        body: &'a Expr<'a>,
        depth: usize,
        pos: usize,
    ) -> EvalResult<'a, Value<'a>> {
        let items = self.eval(items, depth)?;
        let values: Box<dyn Iterator<Item = Value<'a>>> = match &items {
            Value::Array(values) => Box::new(values.iter().cloned()),
            Value::Text(text) => Box::new(text.chars().map(|c| {
                let mut bytes = [0; 4];
                Value::Text(Rc::from(&*c.encode_utf8(&mut bytes)))
            })),
            Value::Dict(dict) => Box::new(dict.order.iter().filter_map(|key| {
                Some(Value::array(vec![
                    Value::Text(key.clone()),
                    dict.get(key)?.clone(),
                ]))
            })),
            _ => {
                return Err(self
                    .fail(pos, "For expression requires an Array or Dictionary")
                    .into());
            }
        };
        self.scopes.push(Bindings::default());
        let mut out = Flow::new();
        let result = (|| {
            for value in values {
                self.step(pos, depth)?;
                self.bind(pattern, value)?;
                if !self.loop_body(body, &mut out, depth, pos)? {
                    break;
                }
            }
            self.new_content(out, pos).map_err(Interrupt::from)
        })();
        self.scopes.pop();
        result
    }

    fn eval_block(
        &mut self,
        exprs: &'a [Expr<'a>],
        depth: usize,
        pos: usize,
    ) -> EvalResult<'a, Value<'a>> {
        self.scopes.push(Bindings::default());
        let settings = self.settings.clone();
        let rules = self.rules.len();
        let result = self.sequence(exprs, depth, pos);
        self.scopes.pop();
        self.settings = settings;
        self.rules.truncate(rules);
        result
    }

    fn eval_let(
        &mut self,
        binding: &'a Expr<'a>,
        value: Option<&'a Expr<'a>>,
        depth: usize,
    ) -> EvalResult<'a, Value<'a>> {
        if let Kind::Call(callee, _) = &binding.kind
            && let Kind::Name(name) = callee.kind
            && let Some(body) = value
        {
            let function = Function {
                params: binding,
                body,
                scope: self.scopes.clone(),
                name: Some(name),
            };
            let value = Value::Function(Rc::new(function));
            self.define(name, value, binding.pos)?;
        } else {
            let value = value
                .as_ref()
                .map_or(Ok(Value::None), |value| self.eval(value, depth))?;
            self.bind(binding, value)?;
        }
        Ok(Value::None)
    }

    fn eval_call(
        &mut self,
        callee: &'a Expr<'a>,
        args: &'a [Arg<'a>],
        depth: usize,
        pos: usize,
    ) -> EvalResult<'a, Value<'a>> {
        let (values, named) = self.args(args, depth, None)?;
        if let Kind::Field(base, fields) = &callee.kind
            && let Some((key, fields)) = fields.split_last()
        {
            let base = self.fields(base, fields, depth, pos)?;
            if let Value::Builtin(module) = base {
                self.call_builtin(&format!("{module}.{key}"), values, named, depth, pos)
            } else {
                self.method(base, key, values, named, depth, pos)
            }
        } else {
            let function = self.eval(callee, depth)?;
            self.call(function, values, named, depth, pos)
        }
    }

    fn eval_content(
        &mut self,
        marks: &'a [Mark<'a>],
        depth: usize,
        pos: usize,
    ) -> EvalResult<'a, Value<'a>> {
        let settings = self.settings.clone();
        let rules = self.rules.len();
        self.scopes.push(Bindings::default());
        let flow = self.markup(marks, depth - 1);
        self.scopes.pop();
        self.settings = settings;
        self.rules.truncate(rules);
        self.new_content(flow?, pos).map_err(Interrupt::from)
    }

    fn literal(&mut self, text: &'a str, pos: usize) -> EvalResult<'a, Value<'a>> {
        if text.starts_with('"') {
            self.spend(text.len(), pos)?;
            self.work(text.len() / 64, pos)?;
            return Ok(Value::Text(decode_string(text).into()));
        }
        if let Some(id) = text.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
            return Ok(Value::Label(id));
        }
        for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
            if let Some(text) = text.strip_prefix(prefix) {
                let end = text
                    .find(|c: char| !c.is_digit(radix))
                    .unwrap_or(text.len());
                let number = u64::from_str_radix(&text[..end], radix)
                    .map_err(|_| self.fail(pos, "Invalid Typst integer."))?;
                return Ok(Value::Number(number as f64, &text[end..]));
            }
        }
        let end = text
            .find(|c: char| c.is_ascii_alphabetic() || c == '%')
            .unwrap_or(text.len());
        let number = text[..end].parse().unwrap_or(0.);
        if end == text.len() && text.contains('.') {
            Ok(Value::Float(number))
        } else {
            Ok(Value::Number(number, &text[end..]))
        }
    }

    fn fields(
        &mut self,
        base: &'a Expr<'a>,
        fields: &[&str],
        depth: usize,
        pos: usize,
    ) -> EvalResult<'a, Value<'a>> {
        let mut value = self.eval(base, depth)?;
        for &key in fields {
            self.step(pos, depth)?;
            value = match value {
                Value::Dict(dict) => dict
                    .get(key)
                    .cloned()
                    .ok_or_else(|| self.fail(pos, &format!("Field {key:?} not found")))?,
                Value::Builtin(module) if module == "sys" && key == "inputs" => {
                    self.work(self.options.typst_inputs.len(), pos)?;
                    let items = self
                        .options
                        .typst_inputs
                        .iter()
                        .map(|(key, value)| {
                            (
                                Rc::from(key.as_str()),
                                Value::Text(Rc::from(value.as_str())),
                            )
                        })
                        .collect();
                    let order = self
                        .options
                        .typst_inputs
                        .iter()
                        .map(|(key, _)| Rc::from(key.as_str()))
                        .collect();
                    Value::dict(items, order)
                }
                Value::Builtin(name) => {
                    let len = name.len().saturating_add(key.len()).saturating_add(1);
                    self.spend(len, pos)?;
                    Value::Builtin(format!("{name}.{key}"))
                }
                Value::Content(content) if key == "body" => match content.as_slice() {
                    [Item::Block(Block::Header(_, _, xs))] => {
                        self.spend(content.weight(), pos)?;
                        let flow = xs.iter().cloned().map(Item::Inline).collect();
                        self.new_content(flow, pos)?
                    }
                    _ => Value::Content(content),
                },
                _ if matches!(key, "height" | "width") => Value::Number(0., "pt"),
                _ => return Err(self.fail(pos, &format!("Field {key:?} not found")).into()),
            };
        }
        Ok(value)
    }

    fn chain(
        &mut self,
        left: &'a Expr<'a>,
        operators: &'a [(&'a str, Expr<'a>)],
        depth: usize,
        pos: usize,
    ) -> EvalResult<'a, Value<'a>> {
        if let [("=", right)] = operators {
            let value = self.eval(right, depth)?;
            self.assign(left, value)?;
            return Ok(Value::None);
        }
        let mut value = self.eval(left, depth)?;
        for (op, right) in operators {
            self.step(pos, depth)?;
            if *op == "and" && !value.truth() || *op == "or" && value.truth() {
                continue;
            }
            let rhs = self.eval(right, depth)?;
            if let Some(op) = op
                .strip_suffix('=')
                .filter(|op| matches!(*op, "+" | "-" | "*" | "/"))
            {
                let assigned = self.binary(op, value, rhs, depth, pos)?;
                self.assign(left, assigned)?;
                value = Value::None;
            } else {
                value = self.binary(op, value, rhs, depth, pos)?;
            }
        }
        Ok(value)
    }

    fn set(&mut self, name: String, named: Scope<'a>, pos: usize) -> Result<()> {
        let shared = Rc::strong_count(&self.settings) > 1;
        if shared {
            self.work(self.settings.len(), pos)?;
        }
        if let Some(settings) = self.settings.get(name.as_str())
            && (shared || Rc::strong_count(settings) > 1)
        {
            self.work(settings.len(), pos)?;
        }
        let settings = Rc::make_mut(&mut self.settings)
            .entry(name.into())
            .or_default();
        Rc::make_mut(settings).extend(named);
        Ok(())
    }

    fn join(
        &mut self,
        a: Value<'a>,
        b: Value<'a>,
        depth: usize,
        pos: usize,
    ) -> EvalResult<'a, Value<'a>> {
        match (&a, &b) {
            (Value::None, _) => Ok(b),
            (_, Value::None) => Ok(a),
            _ => self.binary("+", a, b, depth, pos),
        }
    }

    fn sequence(
        &mut self,
        exprs: &'a [Expr<'a>],
        depth: usize,
        pos: usize,
    ) -> EvalResult<'a, Value<'a>> {
        let mut result = Value::None;
        for expr in exprs {
            match self.eval(expr, depth) {
                Ok(next) => result = self.join(result, next, depth, pos)?,
                Err(Interrupt::Control(control, value, at))
                    if !matches!(control, Control::ReturnValue) =>
                {
                    let value = self.join(result, value, depth, pos)?;
                    return Err(Interrupt::Control(control, value, at));
                }
                Err(interrupt) => return Err(interrupt),
            }
        }
        Ok(result)
    }

    fn loop_body(
        &mut self,
        body: &'a Expr<'a>,
        out: &mut Flow,
        depth: usize,
        pos: usize,
    ) -> EvalResult<'a, bool> {
        let (value, keep_going) = match self.eval(body, depth) {
            Ok(value) | Err(Interrupt::Control(Control::Continue, value, _)) => (value, true),
            Err(Interrupt::Control(Control::Break, value, _)) => (value, false),
            Err(Interrupt::Control(Control::Return, value, at)) => {
                out.append(self.content(value, depth, pos)?);
                let value = self.new_content(std::mem::take(out), pos)?;
                return Err(Interrupt::Control(Control::Return, value, at));
            }
            Err(interrupt) => return Err(interrupt),
        };
        out.append(self.content(value, depth, pos)?);
        Ok(keep_going)
    }

    fn include(&mut self, path: &str, depth: usize, pos: usize) -> Result<Flow> {
        if depth == 0
            || self.budget.active.len() >= 12
            || path.starts_with('@')
            || path.split('/').any(|part| part == "..")
            || path.starts_with('/')
        {
            return Err(self.fail(
                pos,
                "The Typst include path is outside the permitted scope.",
            ));
        }
        let path = if let Some((directory, _)) = self.path.rsplit_once('/') {
            format!("{directory}/{path}")
        } else {
            path.to_owned()
        };
        if self.budget.active.contains(&path) {
            return Err(self.fail(pos, "The Typst include is recursive."));
        }
        let max = usize::try_from(self.context.limits.max_input_size)
            .unwrap_or(usize::MAX)
            .min(self.budget.expansion);
        let bytes = self
            .context
            .read_file_limited(&path, max)
            .map_err(|err| self.fail(pos, &err.to_string()))?;
        self.spend(bytes.len(), pos)?;
        let source = std::str::from_utf8(&bytes).map_err(|err| self.fail(pos, &err.to_string()))?;
        let marks = parser::parse(source, &path, depth)?;
        self.budget.active.push(path.clone());
        let result = evaluate_file(
            source,
            &path,
            &marks,
            depth,
            self.options,
            self.context,
            self.budget,
        );
        self.budget.active.pop();
        let (flow, _) = result?;
        self.spend(flow.weight(), pos)?;
        Ok(flow)
    }

    fn args(
        &mut self,
        args: &'a [Arg<'a>],
        depth: usize,
        mut order: Option<&mut Vec<Rc<str>>>,
    ) -> EvalResult<'a, Args<'a>> {
        let mut values = Vec::new();
        let mut named = Scope::new();
        for arg in args {
            let value = self.eval(&arg.expr, depth)?;
            if let Some(key) = arg.name {
                if let Some(order) = order.as_mut()
                    && !named.contains_key(key)
                {
                    order.push(key.into());
                }
                named.insert(key.into(), value);
            } else if arg.spread {
                self.spend(value.weight(), arg.expr.pos)?;
                match value {
                    Value::Array(items) => values.extend(items.iter().cloned()),
                    Value::Dict(items) => {
                        if let Some(order) = order.as_mut() {
                            order.extend(
                                items
                                    .order
                                    .iter()
                                    .filter(|key| !named.contains_key(*key))
                                    .cloned(),
                            );
                        }
                        named.extend(
                            items
                                .iter()
                                .map(|(key, value)| (key.clone(), value.clone())),
                        );
                    }
                    _ => {
                        return Err(self
                            .fail(arg.expr.pos, "Expected an array or dictionary to spread.")
                            .into());
                    }
                }
            } else {
                values.push(value);
            }
        }
        Ok((values, named))
    }

    fn bind(&mut self, pattern: &'a Expr<'a>, value: Value<'a>) -> Result<()> {
        match &pattern.kind {
            Kind::Name(name) if *name != "_" => self.define(name, value, pattern.pos)?,
            Kind::Args(args) => match value {
                Value::Array(items) => {
                    for (pat, value) in args.iter().zip(items.iter()) {
                        self.bind(&pat.expr, value.clone())?;
                    }
                }
                Value::Dict(items) => {
                    for pat in args {
                        let key = pat.name.or({
                            if let Kind::Name(name) = pat.expr.kind {
                                Some(name)
                            } else {
                                None
                            }
                        });
                        if let Some(value) = key.and_then(|k| items.get(k)) {
                            self.bind(&pat.expr, value.clone())?;
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
        Ok(())
    }

    fn assign(&mut self, pattern: &'a Expr<'a>, value: Value<'a>) -> Result<()> {
        if let Kind::Name(name) = pattern.kind
            && let Some(scope) = self.scopes.iter_mut().rev().find(|s| s.get(name).is_some())
        {
            return scope
                .insert(name, value, &mut self.budget.fuel)
                .ok_or_else(|| limit_error(pattern.pos, self.source));
        }
        self.bind(pattern, value)
    }

    fn transform(
        &mut self,
        function: Value<'a>,
        value: Value<'a>,
        depth: usize,
        pos: usize,
    ) -> EvalResult<'a, Value<'a>> {
        if matches!(function, Value::Function(_) | Value::Builtin(_)) {
            self.call(function, vec![value], Scope::new(), depth, pos)
        } else {
            Ok(function)
        }
    }

    fn call(
        &mut self,
        function: Value<'a>,
        values: Vec<Value<'a>>,
        mut named: Scope<'a>,
        depth: usize,
        pos: usize,
    ) -> EvalResult<'a, Value<'a>> {
        self.step(pos, depth)?;
        match function {
            Value::Builtin(name) => self.call_builtin(&name, values, named, depth - 1, pos),
            Value::Function(function) => {
                let depth = depth - 1;
                let saved = std::mem::replace(&mut self.scopes, function.scope.clone());
                let result = (|| {
                    self.scopes.push(Bindings::default());
                    if let Some(name) = function.name {
                        self.define(name, Value::Function(function.clone()), pos)?;
                    }
                    self.scopes.push(Bindings::default());
                    let mut values = values.into_iter();
                    match &function.params.kind {
                        Kind::Args(args) | Kind::Call(_, args) => {
                            for arg in args {
                                let value = if arg.spread {
                                    Value::array(values.by_ref().collect())
                                } else if let Some(name) = arg.name {
                                    let value = match named.remove(name) {
                                        Some(value) => value,
                                        None => self.eval(&arg.expr, depth)?,
                                    };
                                    self.define(name, value, pos)?;
                                    continue;
                                } else {
                                    values.next().unwrap_or(Value::None)
                                };
                                self.bind(&arg.expr, value)?;
                            }
                        }
                        _ => self.bind(function.params, values.next().unwrap_or(Value::None))?,
                    }
                    self.eval(function.body, depth)
                })();
                self.scopes = saved;
                match result {
                    Err(Interrupt::Control(Control::Return | Control::ReturnValue, value, _)) => {
                        Ok(value)
                    }
                    Err(Interrupt::Control(_, _, pos)) => Err(self
                        .fail(pos, "Unexpected Typst loop control statement.")
                        .into()),
                    result => result,
                }
            }
            _ => Err(self.fail(pos, "Expected a Typst function.").into()),
        }
    }

    fn binary(
        &mut self,
        op: &str,
        a: Value<'a>,
        b: Value<'a>,
        depth: usize,
        pos: usize,
    ) -> EvalResult<'a, Value<'a>> {
        if let (Some((a, lhs, left_float)), Some((b, rhs, right_float))) =
            (a.numeric(), b.numeric())
        {
            let unit = if lhs.is_empty() { rhs } else { lhs };
            let float = unit.is_empty() && (left_float || right_float);
            let number = |n| {
                if float {
                    Value::Float(n)
                } else {
                    Value::Number(n, unit)
                }
            };
            return Ok(match op {
                "+" => number(a + b),
                "-" => number(a - b),
                "*" => number(a * b),
                "/" if !float && unit.is_empty() && b == 0. => {
                    return Err(self
                        .fail(pos, &format!("Can't / VInteger {a} and VInteger {b}"))
                        .into());
                }
                "/" if unit.is_empty() && (a / b).fract() != 0. => Value::Float(a / b),
                "/" => number(a / b),
                "<" => Value::Bool(a < b),
                ">" => Value::Bool(a > b),
                "<=" => Value::Bool(a <= b),
                ">=" => Value::Bool(a >= b),
                "==" => Value::Bool(a == b),
                "!=" => Value::Bool(a != b),
                _ => Value::None,
            });
        }
        Ok(match (op, a, b) {
            ("and" | "or", _, b) => b,
            ("+", Value::Array(a), Value::Array(b)) => {
                self.spend((a.len() + b.len()).saturating_mul(32), pos)?;
                Value::array(a.iter().chain(b.iter()).cloned().collect())
            }
            ("+", Value::Text(a), Value::Text(b)) => {
                self.spend(a.len().saturating_add(b.len()), pos)?;
                Value::Text(format!("{a}{b}").into())
            }
            ("+", Value::Builtin(a), Value::Builtin(b)) => Value::Builtin(format!("{a} + {b}")),
            ("+", a, b) => {
                let mut items = self.content(a, depth, pos)?;
                items.append(self.content(b, depth, pos)?);
                self.new_content(items, pos)?
            }
            ("*", Value::Text(text), Value::Number(n, ""))
            | ("*", Value::Number(n, ""), Value::Text(text)) => {
                let n = n.max(0.) as usize;
                self.spend(text.len().saturating_mul(n).max(n), pos)?;
                Value::Text(text.repeat(n).into())
            }
            ("*", Value::Array(items), Value::Number(n, "")) => {
                let n = n.max(0.) as usize;
                self.spend(items.len().max(1).saturating_mul(n).saturating_mul(32), pos)?;
                Value::array((0..n).flat_map(|_| items.iter().cloned()).collect())
            }
            ("==" | "!=", a, b) => {
                Value::Bool((self.text(&a, pos)? == self.text(&b, pos)?) == (op == "=="))
            }
            (op, a, Value::Dict(dict)) if op == "in" || op.starts_with("not") => {
                Value::Bool(dict.contains_key(self.text(&a, pos)?.as_str()) == (op == "in"))
            }
            _ => Value::None,
        })
    }

    fn method(
        &mut self,
        value: Value<'a>,
        key: &str,
        args: Vec<Value<'a>>,
        mut named: Scope<'a>,
        depth: usize,
        pos: usize,
    ) -> EvalResult<'a, Value<'a>> {
        let first = args.first().cloned().unwrap_or(Value::None);
        Ok(match (value, key) {
            (Value::Array(items), "len") => Value::Number(items.len() as f64, ""),
            (Value::Dict(items), "len") => Value::Number(items.len() as f64, ""),
            (Value::Text(text), "len") => Value::Number(text.chars().count() as f64, ""),
            (Value::Dict(items), "at") => items
                .get(self.text(&first, pos)?.as_str())
                .cloned()
                .or_else(|| named.remove("default"))
                .unwrap_or(Value::None),
            (Value::Array(items), "at" | "first" | "last") => {
                let index = match key {
                    "first" => 0,
                    "last" => -1,
                    _ => first.number().unwrap_or(0.) as isize,
                };
                let index = if index < 0 {
                    items.len().checked_sub(index.unsigned_abs())
                } else {
                    Some(index as usize)
                };
                index
                    .and_then(|i| items.get(i))
                    .cloned()
                    .or_else(|| named.remove("default"))
                    .unwrap_or(Value::None)
            }
            (Value::Array(items), "map" | "filter") => {
                self.spend(items.len().saturating_mul(32), pos)?;
                let mut result = Vec::new();
                for item in items.iter() {
                    let mapped =
                        self.call(first.clone(), vec![item.clone()], Scope::new(), depth, pos)?;
                    if key == "map" {
                        result.push(mapped);
                    } else if mapped.truth() {
                        result.push(item.clone());
                    }
                }
                Value::array(result)
            }
            (Value::Array(items), "join") => {
                let mut result = Value::None;
                for (index, item) in items.iter().enumerate() {
                    self.step(pos, depth)?;
                    if index == 0 {
                        result = item.clone();
                    } else {
                        if !matches!(first, Value::None) {
                            result = self.binary("+", result, first.clone(), depth, pos)?;
                        }
                        result = self.binary("+", result, item.clone(), depth, pos)?;
                    }
                }
                result
            }
            (Value::Text(text), "trim") => Value::Text(text.trim().into()),
            (Value::Text(text), "split") => {
                let separator = self.text(&first, pos)?;
                let items: Vec<_> = if separator.is_empty() {
                    text.split_whitespace()
                        .map(|s| Value::Text(s.into()))
                        .collect()
                } else {
                    text.split(&separator)
                        .map(|s| Value::Text(s.into()))
                        .collect()
                };
                self.spend(items.len().saturating_mul(32), pos)?;
                Value::array(items)
            }
            _ => return Err(self.fail(pos, &format!("Method {key:?} not found")).into()),
        })
    }

    fn call_builtin(
        &mut self,
        name: &str,
        values: Vec<Value<'a>>,
        named: Scope<'a>,
        depth: usize,
        pos: usize,
    ) -> EvalResult<'a, Value<'a>> {
        let name = name.strip_prefix("std.").unwrap_or(name);
        let first = values.first().cloned().unwrap_or(Value::None);
        Ok(match name {
            "regex" | "json" | "csv" | "yaml" | "toml" | "xml" => {
                return Err(self
                    .fail(pos, &format!("The Typst function {name} is not supported."))
                    .into());
            }
            "panic" => {
                let message = self.text(&first, pos)?;
                return Err(self.fail(pos, &message).into());
            }
            "assert" if !first.truth() => {
                return Err(self.fail(pos, "Typst assertion failed.").into());
            }
            "assert" => Value::None,
            "str" | "repr" => Value::Text(self.text(&first, pos)?.into()),
            "int" | "float" => {
                let number = match first.number() {
                    Some(number) => number,
                    None => self.text(&first, pos)?.parse().unwrap_or(0.),
                };
                if name == "float" {
                    Value::Float(number)
                } else {
                    Value::Number(number.trunc(), "")
                }
            }
            "range" => {
                let (start, end) = match values.as_slice() {
                    [a, b, ..] => (
                        a.number().unwrap_or(0.) as i64,
                        b.number().unwrap_or(0.) as i64,
                    ),
                    _ => (0, first.number().unwrap_or(0.) as i64),
                };
                let step = named.get("step").and_then(Value::number).unwrap_or(1.) as i64;
                if step == 0 {
                    return Err(self.fail(pos, "Range step cannot be zero.").into());
                }
                let step_size = usize::try_from(step.unsigned_abs()).unwrap_or(usize::MAX);
                let count = usize::try_from(start.abs_diff(end))
                    .unwrap_or(usize::MAX)
                    .div_ceil(step_size);
                self.spend(count.saturating_mul(32), pos)?;
                let numbers: Box<dyn Iterator<Item = i64>> = if step > 0 {
                    Box::new((start..end).step_by(step_size))
                } else {
                    Box::new((end.saturating_add(1)..=start).rev().step_by(step_size))
                };
                Value::array(numbers.map(|i| Value::Number(i as f64, "")).collect())
            }
            "calc.min" | "calc.max" => {
                let reduce = if name.ends_with("min") {
                    f64::min
                } else {
                    f64::max
                };
                Value::Number(
                    values
                        .iter()
                        .filter_map(Value::number)
                        .reduce(reduce)
                        .unwrap_or(0.),
                    "",
                )
            }
            "calc.even" | "calc.odd" => Value::Bool(
                (first.number().unwrap_or(0.) as i64 % 2 == 0) == name.ends_with("even"),
            ),
            "datetime" => {
                let number = |key| named.get(key).and_then(Value::number).unwrap_or(0.) as i64;
                Value::Text(
                    format!(
                        "{:04}-{:02}-{:02}",
                        number("year"),
                        number("month"),
                        number("day")
                    )
                    .into(),
                )
            }
            "label" => {
                let text = self.text(&first, pos)?;
                Value::element("label", vec![Value::Text(text.into())], named)
            }
            "heading.where" => Value::Builtin(if let Some(level) = named.get("level") {
                format!("heading:{}", self.text(level, pos)?)
            } else {
                "heading".into()
            }),
            "raw.where" => Value::Builtin("raw".into()),
            "rgb" | "luma" | "cmyk" => Value::Builtin(name.into()),
            "style" => self.transform(first, Value::None, depth, pos)?,
            "measure" => Value::dict(
                [
                    ("width".into(), Value::Number(0., "pt")),
                    ("height".into(), Value::Number(0., "pt")),
                ]
                .into(),
                vec!["width".into(), "height".into()],
            ),
            "h" => {
                let (n, unit) = match first {
                    Value::Number(n, unit) => (n, unit),
                    Value::Builtin(ref word) if word == "auto" => (0., ""),
                    _ => (0., ""),
                };
                let em = match unit {
                    "em" | "fr" => n,
                    "pt" => n / 10.,
                    _ => 0.,
                };
                if em == 0. {
                    Value::None
                } else {
                    let text = match em {
                        n if n < 0.25 => "\u{200a}".to_owned(),
                        n if n < 0.6 => "\u{2000}".to_owned(),
                        n if n < 1. => "\u{2000}\u{a0}".to_owned(),
                        n => {
                            let count = n as usize;
                            self.spend(count.saturating_mul(3), pos)?;
                            format!("\u{200a}{}", "\u{2001}".repeat(count))
                        }
                    };
                    Value::Text(text.into())
                }
            }
            "v" | "hide" | "outline" | "metadata" | "counter" | "state" | "set" => Value::None,
            "read" => {
                let path = self.text(&first, pos)?;
                if path.starts_with(['/', '@']) || path.split('/').any(|part| part == "..") {
                    return Err(self
                        .fail(pos, "The Typst read path is outside the permitted scope.")
                        .into());
                }
                let path = self
                    .path
                    .rsplit_once('/')
                    .map_or(path.clone(), |(dir, _)| format!("{dir}/{path}"));
                let limit = usize::try_from(self.context.limits.max_input_size)
                    .unwrap_or(usize::MAX)
                    .min(self.budget.expansion);
                let bytes = self
                    .context
                    .read_file_limited(&path, limit)
                    .map_err(|e| self.fail(pos, &e.to_string()))?;
                self.spend(bytes.len(), pos)?;
                Value::Text(
                    String::from_utf8(bytes)
                        .map_err(|err| self.fail(pos, &err.to_string()))?
                        .into(),
                )
            }
            "str.trim" => Value::Text(self.text(&first, pos)?.trim().into()),
            "lorem" => {
                let n = first.number().unwrap_or(0.).max(0.) as usize;
                self.spend(n.saturating_mul(16), pos)?;
                const WORDS: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.";
                Value::Text(
                    WORDS
                        .split_whitespace()
                        .cycle()
                        .take(n)
                        .collect::<Vec<_>>()
                        .join(" ")
                        .into(),
                )
            }
            _ if builtin(name) => {
                let element = Value::element(name, values, named);
                if let Some(rule) = self.rule(pos, 1, |rule| {
                    rule.selector.as_ref() == name && matches!(rule.transform, Value::Builtin(_))
                })? {
                    self.transform(rule.transform, element, depth, pos)?
                } else {
                    element
                }
            }
            _ => {
                return Err(self
                    .fail(pos, &format!("Function {name:?} not found"))
                    .into());
            }
        })
    }
}

fn continuation(marks: &[Mark<'_>], start: usize, indent: usize) -> usize {
    if !matches!(marks.get(start), Some(Mark::Break(next) | Mark::Paragraph(next)) if *next > indent)
    {
        return start;
    }
    marks[start + 1..]
        .iter()
        .position(
            |mark| matches!(mark, Mark::Break(next) | Mark::Paragraph(next) if *next <= indent),
        )
        .map_or(marks.len(), |offset| start + 1 + offset)
}

fn is_bullet(marker: &str) -> bool {
    marker == "-"
}

type ListMark<'a> = (usize, &'a str, usize, &'a [Mark<'a>]);

fn next_list<'a>(marks: &'a [Mark<'a>], index: usize) -> Option<ListMark<'a>> {
    let (offset, mark) = marks[index..]
        .iter()
        .enumerate()
        .find(|(_, mark)| !matches!(mark, Mark::Break(_) | Mark::Paragraph(_)))?;
    if let Mark::List(marker, margin, body) = mark {
        Some((index + offset, marker, *margin, body))
    } else {
        None
    }
}

fn smart_quotes(text: &str, follows_word: bool) -> String {
    let mut previous = follows_word;
    text.chars()
        .map(|c| {
            let quote = match c {
                '\'' if previous => '’',
                '\'' => '‘',
                '"' if previous => '”',
                '"' => '“',
                _ => c,
            };
            previous = c.is_alphanumeric() || matches!(c, '.' | ',' | '!' | '?' | ')');
            quote
        })
        .collect()
}

pub(super) fn decode_string(text: &str) -> String {
    let mut chars = text
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(text)
        .chars();
    let mut output = String::new();
    while let Some(c) = chars.next() {
        if c != '\\' {
            output.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => output.push('\n'),
            Some('r') => output.push('\r'),
            Some('t') => output.push('\t'),
            Some('u') if chars.as_str().starts_with('{') => {
                chars.next();
                let digits: String = chars.by_ref().take_while(|c| *c != '}').collect();
                if let Some(c) = u32::from_str_radix(&digits, 16)
                    .ok()
                    .and_then(char::from_u32)
                {
                    output.push(c);
                }
            }
            Some(c) => output.push(c),
            None => output.push('\\'),
        }
    }
    output
}

fn builtin(name: &str) -> bool {
    matches!(
        name,
        "align"
            | "auto"
            | "bibliography"
            | "black"
            | "block"
            | "blue"
            | "box"
            | "bytes"
            | "calc"
            | "circle"
            | "cite"
            | "cmyk"
            | "columns"
            | "context"
            | "counter"
            | "csv"
            | "datetime"
            | "divider"
            | "document"
            | "emph"
            | "enum"
            | "figure"
            | "float"
            | "footnote"
            | "grid"
            | "h"
            | "heading"
            | "hide"
            | "highlight"
            | "image"
            | "include"
            | "int"
            | "json"
            | "label"
            | "line"
            | "linebreak"
            | "link"
            | "list"
            | "lorem"
            | "lower"
            | "luma"
            | "math"
            | "metadata"
            | "outline"
            | "pad"
            | "page"
            | "pagebreak"
            | "panic"
            | "par"
            | "parbreak"
            | "place"
            | "quote"
            | "range"
            | "raw"
            | "read"
            | "rect"
            | "ref"
            | "regex"
            | "repr"
            | "rgb"
            | "set"
            | "smallcaps"
            | "smartquote"
            | "square"
            | "state"
            | "std"
            | "str"
            | "strike"
            | "strong"
            | "style"
            | "sub"
            | "super"
            | "sym"
            | "emoji"
            | "sys"
            | "table"
            | "terms"
            | "text"
            | "title"
            | "toml"
            | "underline"
            | "upper"
            | "v"
            | "xml"
            | "yaml"
            | "left"
            | "center"
            | "right"
            | "top"
            | "bottom"
            | "horizon"
            | "red"
            | "orange"
            | "green"
            | "purple"
            | "white"
            | "measure"
            | "assert"
            | "table.header"
            | "table.footer"
            | "table.cell"
            | "table.hline"
            | "table.vline"
            | "grid.header"
            | "grid.footer"
            | "grid.cell"
            | "grid.hline"
            | "grid.vline"
    )
}
