//! Office Math Markup Language output for the DOCX writer.

use crate::{
    render::{Unsupported, accent_char, column_alignment, has_limits},
    tree::{Aligns, Alphabet, Class, Exp, Row},
};

pub(crate) fn render(items: &[Exp], display: bool) -> Result<String, Unsupported> {
    let mut writer = Omml {
        output: String::new(),
        display,
        style: None,
    };
    writer.container("oMath", items);
    Ok(writer.output)
}

struct Omml {
    output: String,
    display: bool,
    style: Option<&'static Alphabet>,
}

impl Omml {
    fn open(&mut self, tag: &str) {
        self.output.push_str("<m:");
        self.output.push_str(tag);
        self.output.push('>');
    }

    fn close(&mut self, tag: &str) {
        self.output.push_str("</m:");
        self.output.push_str(tag);
        self.output.push('>');
    }

    fn empty(&mut self, tag: &str) {
        self.output.push_str("<m:");
        self.output.push_str(tag);
        self.output.push_str(" />");
    }

    fn property(&mut self, name: &str, value: &str) {
        self.output.push_str("<m:");
        self.output.push_str(name);
        self.output.push_str(" m:val=\"");
        self.escaped(value);
        self.output.push_str("\" />");
    }

    fn escaped(&mut self, text: &str) {
        for c in text.chars() {
            self.output.push_str(match c {
                '&' => "&amp;",
                '<' => "&lt;",
                '>' => "&gt;",
                '"' => "&quot;",
                _ => {
                    self.output.push(c);
                    continue;
                }
            });
        }
    }

    fn container(&mut self, name: &str, items: &[Exp]) {
        if items.is_empty() {
            self.empty(name);
        } else {
            self.open(name);
            self.list(items);
            self.close(name);
        }
    }

    fn list(&mut self, items: &[Exp]) {
        let fences = sized_pairs(items);
        let mut index = 0;
        let mut naries = 0;
        let mut outer_naries = Vec::new();
        while let Some(exp) = items.get(index) {
            if let Some((operator, sub, sup, limits)) = nary(exp) {
                self.open_nary(operator, sub, sup, limits);
                naries += 1;
                index += 1;
                if index == items.len() || matches!(fences.get(index), Some(Sized::Close { .. })) {
                    self.unstyled_run("\u{200b}");
                } else {
                    continue;
                }
            } else {
                match fences.get(index).unwrap_or(&Sized::Unpaired) {
                    Sized::Open { close, end } => {
                        self.open_delimiter(sized_text(exp).unwrap_or_default(), "", close);
                        if *end == index + 1 {
                            self.empty("e");
                        } else {
                            self.open("e");
                        }
                        outer_naries.push(std::mem::take(&mut naries));
                        index += 1;
                        continue;
                    }
                    Sized::Close { empty } => {
                        if !empty {
                            self.close("e");
                        }
                        self.close("d");
                        naries = outer_naries.pop().unwrap_or_default();
                    }
                    Sized::Unpaired => self.expression(exp),
                }
                index += 1;
            }
            for _ in 0..naries {
                self.close("e");
                self.close("nary");
            }
            naries = 0;
        }
    }

    fn expression(&mut self, exp: &Exp) {
        match exp {
            Exp::Num(text) => self.run(text, self.style.map(|style| style.other), false, false),
            Exp::Sym(class, text) => self.symbol(*class, text),
            Exp::Group(items) if items.is_empty() => self.unstyled_run("\u{200b}"),
            Exp::Group(items) | Exp::PaddedText(items) => self.list(items),
            Exp::Scripts {
                base,
                sub,
                sup,
                limits,
            } => {
                if let Some((operator, sub, sup, stacked)) = nary(exp) {
                    self.open_nary(operator, sub, sup, stacked);
                    self.unstyled_run("\u{200b}");
                    self.close("e");
                    self.close("nary");
                } else {
                    let stacked = limits.unwrap_or_else(|| {
                        matches!(base.as_ref(), Exp::Accent { mark, .. } if matches!(mark.mathml, "⏞" | "⏟"))
                            || (self.display && has_limits(base))
                    });
                    self.scripts(base, sub.as_deref(), sup.as_deref(), stacked);
                }
            }
            Exp::Stack { base, under, over } => {
                if let Some((operator, sub, sup, stacked)) = nary(exp) {
                    self.open_nary(operator, sub, sup, stacked);
                    self.unstyled_run("\u{200b}");
                    self.close("e");
                    self.close("nary");
                } else {
                    self.scripts(base, under.as_deref(), over.as_deref(), true);
                }
            }
            Exp::Frac { num, den, line } => {
                self.fraction(num, den, if *line { "bar" } else { "noBar" })
            }
            Exp::Display(false, body)
                if let Exp::Frac {
                    num,
                    den,
                    line: true,
                } = body.as_ref() =>
            {
                self.fraction(num, den, "lin");
            }
            Exp::Sqrt(body) => {
                self.open("rad");
                self.open("radPr");
                self.property("degHide", "on");
                self.close("radPr");
                self.empty("deg");
                self.argument("e", Some(body));
                self.close("rad");
            }
            Exp::Root(body, index) => {
                self.open("rad");
                self.argument("deg", Some(index));
                self.argument("e", Some(body));
                self.close("rad");
            }
            Exp::Fenced { open, body, close } => {
                self.delimiter(open.as_deref(), body, close.as_deref())
            }
            Exp::Middle(text) => self.symbol(Class::Ord, text),
            Exp::Accent { body, mark } => self.accent(body, mark.mathml),
            Exp::Space(space) => {
                let text = match space.width {
                    "0.167em" => "\u{2009}",
                    "0.222em" => "\u{2005}",
                    "0.278em" | "0.444em" | "0.5em" => "\u{2004}",
                    "-0.167em" => "\u{200b}",
                    "0.333em" => return,
                    "1.0em" => "\u{2001}",
                    "2.0em" => "\u{2001}\u{2001}",
                    _ => space.text,
                };
                self.run(text, self.style.map(|style| style.other), false, false);
            }
            Exp::Text(style, text) => {
                self.run(text, style.map(|style| style.other), style.is_none(), false)
            }
            Exp::Styled(style, items) => {
                let previous = self.style.replace(style);
                if items.is_empty() {
                    self.unstyled_run("\u{200b}");
                } else {
                    self.list(items);
                }
                self.style = previous;
            }
            Exp::Limits(body) | Exp::Display(_, body) | Exp::Sized(_, body) => {
                self.expression(body)
            }
            Exp::Enclose(notation, body) => self.border_box(notation, body),
            Exp::Phantom(body) => {
                self.open("phant");
                self.open("phantPr");
                self.property("show", "off");
                self.close("phantPr");
                self.argument("e", Some(body));
                self.close("phant");
            }
            Exp::Table { aligns, rows, .. } => self.table(aligns, rows),
        }
    }

    fn run(&mut self, text: &str, style: Option<&str>, normal_text: bool, function: bool) {
        self.open("r");
        if normal_text || function || style.is_some() {
            self.open("rPr");
            if normal_text {
                self.empty("nor");
            }
            if function {
                self.property("sty", "p");
            }
            if let Some(style) = style {
                let (script, weight) = match style {
                    "italic" => (None, "i"),
                    "bold" => (None, "b"),
                    "bold-italic" => (None, "bi"),
                    "bold-script" => (Some("script"), "b"),
                    "bold-fraktur" => (Some("fraktur"), "b"),
                    "sans-serif-italic" => (Some("sans-serif"), "i"),
                    "normal" => (None, "p"),
                    alphabet => (Some(alphabet), "p"),
                };
                if let Some(script) = script {
                    self.property("scr", script);
                }
                self.property("sty", weight);
            }
            self.close("rPr");
        }
        self.open("t");
        self.escaped(text);
        self.close("t");
        self.close("r");
    }

    fn unstyled_run(&mut self, text: &str) {
        self.run(text, None, false, false);
    }

    fn symbol(&mut self, class: Class, text: &str) {
        if text.is_empty() {
            self.unstyled_run("\u{200b}");
            return;
        }
        let function = matches!(class, Class::Func | Class::FuncLimits);
        let style = match (self.style, class) {
            (Some(style), Class::Ident) => Some(style.ident),
            (Some(style), _) => Some(style.other),
            (None, Class::Ident | Class::Text | Class::Func | Class::FuncLimits) => None,
            (None, _) => Some("normal"),
        };
        let operator_name = class == Class::Op
            && text.chars().any(char::is_alphanumeric)
            && !crate::symbols::SYMBOLS
                .iter()
                .any(|(_, class, symbol, _)| *class == Class::Op && *symbol == text);
        if operator_name {
            self.output
                .push_str("<m:box><m:boxPr><m:opEmu m:val=\"on\" /></m:boxPr><m:e>");
        }
        self.run(text, style, class == Class::Text, function);
        if operator_name {
            self.output.push_str("</m:e></m:box>");
        }
    }

    fn argument(&mut self, name: &str, exp: Option<&Exp>) {
        self.open(name);
        if let Some(exp) = exp {
            self.expression(exp);
        } else {
            self.unstyled_run("\u{200b}");
        }
        self.close(name);
    }

    fn fraction(&mut self, num: &Exp, den: &Exp, kind: &str) {
        self.open("f");
        self.open("fPr");
        self.property("type", kind);
        self.close("fPr");
        self.argument("num", Some(num));
        self.argument("den", Some(den));
        self.close("f");
    }

    fn scripts(&mut self, base: &Exp, sub: Option<&Exp>, sup: Option<&Exp>, stacked: bool) {
        if stacked {
            for (tag, script) in [("limLow", sub), ("limUpp", sup)] {
                if script.is_some() {
                    self.open(tag);
                    self.open("e");
                }
            }
            self.expression(base);
            for (tag, script) in [("limUpp", sup), ("limLow", sub)] {
                if script.is_some() {
                    self.close("e");
                    self.argument("lim", script);
                    self.close(tag);
                }
            }
        } else {
            let tag = match (sub.is_some(), sup.is_some()) {
                (true, true) => "sSubSup",
                (true, false) => "sSub",
                _ => "sSup",
            };
            self.open(tag);
            self.argument("e", Some(base));
            if sub.is_some() {
                self.argument("sub", sub);
            }
            if sup.is_some() {
                self.argument("sup", sup);
            }
            self.close(tag);
        }
    }

    fn open_nary(&mut self, operator: &str, sub: Option<&Exp>, sup: Option<&Exp>, stacked: bool) {
        self.open("nary");
        self.open("naryPr");
        self.property("chr", operator);
        self.property("limLoc", if stacked { "undOvr" } else { "subSup" });
        self.property("subHide", if sub.is_some() { "off" } else { "on" });
        self.property("supHide", if sup.is_some() { "off" } else { "on" });
        self.close("naryPr");
        self.argument("sub", sub);
        self.argument("sup", sup);
        self.open("e");
    }

    fn accent(&mut self, body: &Exp, mark: &str) {
        let tag = match mark {
            "¯" | "_" => {
                self.open("bar");
                self.open("barPr");
                self.property("pos", if mark == "_" { "bot" } else { "top" });
                self.close("barPr");
                "bar"
            }
            "⏞" | "⎴" | "⏜" => {
                self.open("groupChr");
                self.open("groupChrPr");
                self.property("chr", mark);
                self.property("pos", "top");
                self.property("vertJc", "bot");
                self.close("groupChrPr");
                "groupChr"
            }
            "⏟" | "⎵" | "⏝" => {
                self.open("limLow");
                self.argument("e", Some(body));
                self.open("lim");
                self.symbol(Class::Ord, mark);
                self.close("lim");
                self.close("limLow");
                return;
            }
            _ => {
                self.open("acc");
                self.open("accPr");
                self.property("chr", accent_char(mark));
                self.close("accPr");
                "acc"
            }
        };
        self.argument("e", Some(body));
        self.close(tag);
    }

    fn open_delimiter(&mut self, open: &str, middle: &str, close: &str) {
        self.open("d");
        self.open("dPr");
        self.property("begChr", open);
        self.property("sepChr", middle);
        self.property("endChr", close);
        self.empty("grow");
        self.close("dPr");
    }

    fn delimiter(&mut self, open: Option<&str>, body: &[Exp], close: Option<&str>) {
        let middle = body.iter().find_map(|item| match item {
            Exp::Middle(text) => Some(text.as_str()),
            _ => None,
        });
        self.open_delimiter(
            open.unwrap_or_default(),
            middle.unwrap_or_default(),
            close.unwrap_or_default(),
        );
        for part in body.split(|item| matches!(item, Exp::Middle(_))) {
            self.container("e", part);
        }
        self.close("d");
    }

    fn border_box(&mut self, notation: &str, body: &Exp) {
        self.open("borderBox");
        if notation != "box" {
            self.open("borderBoxPr");
            for side in ["hideTop", "hideBot", "hideLeft", "hideRight"] {
                self.property(side, "1");
            }
            if notation.contains("updiagonalstrike") {
                self.property("strikeBLTR", "1");
            }
            if notation.contains("downdiagonalstrike") {
                self.property("strikeTLBR", "1");
            }
            self.close("borderBoxPr");
        }
        self.argument("e", Some(body));
        self.close("borderBox");
    }

    fn table(&mut self, aligns: &Aligns, rows: &[Row]) {
        let columns = rows.iter().map(Vec::len).max().unwrap_or_default();
        if matches!(aligns, Aligns::Pairs) && columns > 1 && columns % 2 == 0 {
            self.open("eqArr");
            for row in rows {
                self.open("e");
                for (column, cell) in row.iter().enumerate() {
                    if column > 0 {
                        self.unstyled_run("&");
                    }
                    self.list(cell);
                }
                self.close("e");
            }
            self.close("eqArr");
        } else {
            self.open("m");
            self.open("mPr");
            self.property("baseJc", "center");
            self.property("plcHide", "on");
            self.open("mcs");
            for column in 0..columns {
                self.open("mc");
                self.open("mcPr");
                self.property("count", "1");
                self.property("mcJc", column_alignment(aligns, column, rows));
                self.close("mcPr");
                self.close("mc");
            }
            self.close("mcs");
            self.close("mPr");
            for row in rows {
                self.open("mr");
                for cell in row {
                    self.container("e", cell);
                }
                self.close("mr");
            }
            self.close("m");
        }
    }
}

fn nary(exp: &Exp) -> Option<(&str, Option<&Exp>, Option<&Exp>, bool)> {
    let (base, sub, sup, limits) = match exp {
        Exp::Scripts {
            base,
            sub,
            sup,
            limits,
        } => (base, sub, sup, *limits),
        Exp::Stack { base, under, over } => (base, under, over, Some(true)),
        _ => return None,
    };
    let Exp::Sym(Class::Op, operator) = base.as_ref() else {
        return None;
    };
    let stacked = match operator.as_str() {
        "∑" | "∏" | "∐" => true,
        "∫" | "∬" | "∭" | "∮" | "∯" | "∰" => false,
        _ => return None,
    };
    Some((
        operator,
        sub.as_deref(),
        sup.as_deref(),
        limits.unwrap_or(stacked),
    ))
}

#[derive(Clone, Copy)]
enum Sized<'a> {
    Unpaired,
    Open { close: &'a str, end: usize },
    Close { empty: bool },
}

fn sized_text(exp: &Exp) -> Option<&str> {
    match exp {
        Exp::Sized(_, body) if let Exp::Sym(_, text) = body.as_ref() => Some(text),
        _ => None,
    }
}

fn sized_pairs(items: &[Exp]) -> Vec<Sized<'_>> {
    if !items.iter().any(|exp| matches!(exp, Exp::Sized(..))) {
        return Vec::new();
    }
    let mut opens = Vec::new();
    let mut pairs = vec![Sized::Unpaired; items.len()];
    for (index, item) in items.iter().enumerate() {
        if let Exp::Sized(_, body) = item
            && let Exp::Sym(class, text) = body.as_ref()
        {
            match class {
                Class::Open if text != "|" => opens.push(index),
                Class::Close
                    if matches!(text.as_str(), ")" | "]" | "}" | "⟩" | "⌋" | "⌉" | "⟧") =>
                {
                    if let Some(start) = opens.pop() {
                        pairs[start] = Sized::Open {
                            close: text,
                            end: index,
                        };
                        pairs[index] = Sized::Close {
                            empty: index == start + 1,
                        };
                    }
                }
                _ => {}
            }
        }
    }
    pairs
}
