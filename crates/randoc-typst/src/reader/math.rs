use super::eval::decode_string;
use super::symbols;

type Variable<'f> = dyn FnMut(&str) -> randoc_core::Result<Option<String>> + 'f;

pub(super) enum Error {
    Depth,
    Incomplete,
    Evaluation(randoc_core::Error),
}

type Result<T> = std::result::Result<T, Error>;

struct Math<'a, 'f> {
    rest: &'a str,
    variable: &'f mut Variable<'f>,
}

struct Atom {
    tex: String,
    group: Option<String>,
    binary: bool,
}

impl Atom {
    fn ordinary(tex: impl Into<String>) -> Self {
        Self {
            tex: tex.into(),
            group: None,
            binary: false,
        }
    }
    fn inner(&self) -> &str {
        self.group.as_deref().unwrap_or(&self.tex)
    }
}

pub(super) fn convert(source: &str, depth: usize, variable: &mut Variable<'_>) -> Result<String> {
    let mut math = Math {
        rest: source,
        variable,
    };
    let rows = math.rows(depth, '\0')?;
    if rows.len() == 1 {
        return Ok(rows.first().ok_or(Error::Incomplete)?.join(" & "));
    }
    let aligned = rows.iter().any(|row| row.len() > 1);
    let mut rows = rows;
    if !aligned {
        for line in rows.iter_mut().skip(1).flatten() {
            if line.starts_with("+ ") || line.starts_with("- ") {
                line.insert(0, ' ');
            }
        }
    }
    Ok(if aligned {
        environment("aligned", rows)
    } else {
        format!("\\begin{{array}}{{r}}\n{}\n\\end{{array}}", join_rows(rows))
    })
}

impl<'a> Math<'a, '_> {
    fn variable(&mut self, name: &str) -> Result<Option<String>> {
        Ok((self.variable)(name)
            .map_err(Error::Evaluation)?
            .map(|value| match value.as_str() {
                "\u{200a}\u{2001}" => "\\quad".into(),
                "\u{200a}\u{2001}\u{2001}" => "\\qquad".into(),
                _ => value,
            }))
    }

    fn trim(&mut self) {
        self.rest = self.rest.trim_start();
    }
    fn peek(&self) -> Option<char> {
        self.rest.chars().next()
    }
    fn pop(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.rest = &self.rest[c.len_utf8()..];
        Some(c)
    }
    fn take(&mut self, s: &str) -> bool {
        if let Some(rest) = self.rest.strip_prefix(s) {
            self.rest = rest;
            true
        } else {
            false
        }
    }
    fn scan(&mut self, predicate: impl Fn(char) -> bool) -> &'a str {
        let end = self.rest.find(|c| !predicate(c)).unwrap_or(self.rest.len());
        let word = &self.rest[..end];
        self.rest = &self.rest[end..];
        word
    }
    fn rows(&mut self, depth: usize, end: char) -> Result<Vec<Vec<String>>> {
        if depth == 0 {
            return Err(Error::Depth);
        }
        let mut rows = Vec::new();
        let mut columns = Vec::new();
        loop {
            columns.push(self.sequence(depth, &[end, '&'])?);
            if self.take("&") {
                continue;
            }
            rows.push(std::mem::take(&mut columns));
            if self.take("\\") {
                self.trim();
                continue;
            }
            break;
        }
        Ok(rows)
    }

    fn sequence(&mut self, depth: usize, ends: &[char]) -> Result<String> {
        if depth == 0 {
            return Err(Error::Depth);
        }
        let mut out = String::new();
        let mut control_word = false;
        loop {
            let spaced = self.peek().is_some_and(char::is_whitespace) && !out.is_empty();
            self.trim();
            let Some(c) = self.peek() else { break };

            if ends.contains(&c)
                || c == '\\' && self.rest.chars().nth(1).is_none_or(char::is_whitespace)
            {
                break;
            }
            let mut left = self.attachment(depth, false)?;
            if c == '"'
                && let Some(text) = left.tex.strip_prefix("\\text{")
                && let Some(end) = text.find('}')
            {
                let text = &text[..end];
                let tail = &left.tex["\\text{".len() + end + 1..];
                let space_before = if spaced { " " } else { "" };
                let space_after = if self.peek().is_some_and(char::is_whitespace) {
                    " "
                } else {
                    ""
                };
                if text.trim().parse::<f64>().is_ok() {
                    let before = if spaced || text.starts_with(char::is_whitespace) {
                        "\\ "
                    } else {
                        ""
                    };
                    let after = if !space_after.is_empty() || text.ends_with(char::is_whitespace) {
                        "\\ "
                    } else {
                        ""
                    };
                    left.tex = format!("{before}{}{after}{tail}", text.trim());
                } else {
                    left.tex = format!("\\text{{{space_before}{text}{space_after}}}{tail}");
                }
            }
            if c == '|' && spaced && self.peek().is_some_and(char::is_whitespace) {
                left.tex = "~|~".into();
            }
            let save = self.rest;
            self.trim();
            if self.take("/") {
                self.trim();
                let right = self.attachment(depth, false)?;
                left = Atom::ordinary(format!("\\frac{{{}}}{{{}}}", left.inner(), right.inner()));
            } else {
                self.rest = save;
            }
            if left.tex.starts_with("\\mathbb{")
                && left.tex.ends_with('}')
                && (!out.is_empty() || self.rest.trim_start().starts_with(|c| !ends.contains(&c)))
            {
                left.tex = format!("{{{}}}", left.tex);
            }
            if left.binary {
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push_str(&left.tex);
                out.push(' ');
                control_word = false;
            } else if !left.tex.is_empty() {
                if control_word && left.tex.starts_with(|c: char| c.is_ascii_alphabetic()) {
                    out.push(' ');
                }
                control_word = left.tex.rsplit_once('\\').is_some_and(|(_, last)| {
                    !last.is_empty() && last.bytes().all(|b| b.is_ascii_alphabetic())
                });
                out.push_str(&left.tex);
            }
        }
        Ok(out.trim_end().to_owned())
    }

    fn attachment(&mut self, depth: usize, script: bool) -> Result<Atom> {
        let mut base = self.atom(depth, script)?;
        if script {
            return Ok(base);
        }
        let mut sub = None;
        let mut sup = None;
        while let Some(c @ ('_' | '^' | '\'')) = self.peek() {
            self.pop();
            if c == '\'' {
                base.tex.push('\'');
                continue;
            }
            self.trim();
            let arg = self.attachment(depth.saturating_sub(1), true)?;
            if c == '_' {
                sub = Some(arg.inner().to_owned());
            } else {
                sup = Some(arg.inner().to_owned());
            }
        }
        if sub.is_some() || sup.is_some() {
            if base.tex.starts_with("\\mathbb{")
                || base.tex.starts_with('\\') && base.tex.contains('(') && base.tex.ends_with(')')
            {
                base.tex = format!("{{{}}}", base.tex);
            }
            if base.tex.is_empty() {
                base.tex.push_str("{}");
            }
            if is_operator_with_limits(&base.tex) {
                base.tex.push_str("\\limits");
            }
            if let Some(arg) = sub {
                base.tex.push_str(&format!("_{{{arg}}}"));
            }
            if let Some(arg) = sup {
                base.tex.push_str(&format!("^{{{arg}}}"));
            }
            base.group = None;
        }
        while !self.rest.starts_with("!=") && self.take("!") {
            base.tex.push('!');
            base.group = None;
        }
        Ok(base)
    }

    fn atom(&mut self, depth: usize, script: bool) -> Result<Atom> {
        if depth == 0 {
            return Err(Error::Depth);
        }
        let c = self.pop().ok_or(Error::Incomplete)?;
        let mut out = Atom::ordinary(match c {
            '(' | '[' | '{' => {
                let closing = match c {
                    '(' => ')',
                    '[' => ']',
                    _ => '}',
                };
                let inner = self.sequence(depth - 1, &[closing])?;
                self.take(&closing.to_string());
                let tex = match c {
                    '{' if inner.is_empty() => "\\left\\{ \\right\\}".into(),
                    '{' => format!("\\left\\{{ {inner} \\right\\}}"),
                    '[' if inner.is_empty() => "\\lbrack\\rbrack".into(),
                    '[' => format!("\\left\\lbrack {inner} \\right\\rbrack"),
                    _ if inner.contains("\\frac") || inner.contains(['^', '_', '(']) => {
                        format!("\\left( {inner} \\right)")
                    }
                    _ => format!("({inner})"),
                };
                return Ok(Atom {
                    tex,
                    group: Some(inner),
                    binary: false,
                });
            }
            '"' => {
                let mut text = String::new();
                while let Some(c) = self.pop() {
                    if c == '"' {
                        break;
                    }
                    text.push(c);
                    if c == '\\' {
                        text.push(self.pop().ok_or(Error::Incomplete)?);
                    }
                }
                let text = decode_string(&text);
                if text.is_empty() {
                    String::new()
                } else {
                    format!("\\text{{{text}}}")
                }
            }
            '#' => {
                let word = self.scan(|c| c.is_alphanumeric() || c == '.' || c == '-');
                self.variable(word)?.unwrap_or_default()
            }
            '\\' => {
                if self.take("u{") {
                    let chars = self.scan(|c| c.is_ascii_hexdigit());
                    let codepoint = u32::from_str_radix(chars, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .unwrap_or('\u{fffd}');
                    self.take("}");
                    unicode(codepoint).map_or_else(|| codepoint.to_string(), str::to_owned)
                } else {
                    match self.pop() {
                        Some('(') => "(".into(),
                        Some(')') => ")".into(),
                        Some(c) => c.to_string(),
                        None => String::new(),
                    }
                }
            }
            '.' if self.take("..") => "\\ldots".into(),
            '<' if self.take("-->") => "\\longleftrightarrow".into(),
            '<' if self.take("-") => "\\leftarrow".into(),
            '-' if self.take(">") => "\\rightarrow".into(),
            '|' if self.take("->") => "\\mapsto".into(),
            '=' if self.take(">") => "\\Rightarrow".into(),
            '<' if self.take("=") => "\\leq".into(),
            '>' if self.take("=") => "\\geq".into(),
            '!' if self.take("=") => "\\neq".into(),
            '<' if self.take("<") => "\\ll".into(),
            '>' if self.take(">") => "\\gg".into(),
            '*' => "\\ast".into(),
            '0'..='9' if !script => {
                let digits = self.scan(|c| c.is_ascii_digit() || c == '.');
                format!("{c}{digits}")
            }
            c if c.is_alphabetic() => {
                let suffix = self.scan(|c| c.is_alphanumeric() || c == '.');
                let name = format!("{c}{suffix}");
                if self.peek() == Some('(') && name.len() > 1 {
                    if is_operator(&name)
                        || matches!(
                            name.as_str(),
                            "sum" | "integral" | "union.big" | "inter.big"
                        )
                    {
                        let group = self.atom(depth - 1, script)?;
                        let op = symbol(&name).map_or_else(|| format!("\\{name}"), str::to_owned);
                        return Ok(Atom::ordinary(format!("{op}({})", group.inner())));
                    }
                    self.pop();
                    return self.function(&name, depth - 1);
                }
                if name.len() > 1 {
                    if let Some(variable) = self.variable(&name)? {
                        variable
                    } else if let Some(tex) = symbol(&name) {
                        tex.into()
                    } else if is_operator(&name) {
                        format!("\\{name}")
                    } else if name.len() == 2
                        && name.as_bytes()[0].is_ascii_uppercase()
                        && name.as_bytes()[0] == name.as_bytes()[1]
                    {
                        format!("\\mathbb{{{c}}}")
                    } else if let Some(symbol) = symbols::lookup(&format!("sym.{name}")) {
                        symbol.into()
                    } else {
                        name
                    }
                } else {
                    unicode(c).map_or_else(|| c.to_string(), str::to_owned)
                }
            }
            c => unicode(c).map_or_else(|| c.to_string(), str::to_owned),
        });
        out.binary = matches!(
            out.tex.as_str(),
            "+" | "-"
                | "="
                | "<"
                | ">"
                | "\\leq"
                | "\\geq"
                | "\\ll"
                | "\\gg"
                | "\\in"
                | "\\notin"
                | "\\times"
                | "\\div"
                | "\\neq"
                | "\\cdot"
                | "\\mid"
                | "\\nmid"
                | "\\cup"
                | "\\cap"
                | "\\rightarrow"
                | "\\leftarrow"
                | "\\pm"
                | "\\mp"
                | "\\subset"
                | "\\subseteq"
                | "\\approx"
        );
        Ok(out)
    }

    fn function(&mut self, name: &str, depth: usize) -> Result<Atom> {
        if depth == 0 {
            return Err(Error::Depth);
        }
        let mut rows = Vec::new();
        let mut args = Vec::new();
        let mut named = std::collections::BTreeMap::new();
        self.trim();
        while !self.take(")") {
            if self.rest.is_empty() {
                break;
            }
            let save = self.rest;
            let key = self.scan(|c| c.is_alphabetic() || c == '-').to_owned();
            self.trim();
            if !key.is_empty() && self.take(":") {
                named.insert(key, self.sequence(depth, &[',', ';', ')'])?);
            } else {
                self.rest = save;
                args.push(self.sequence(depth, &[',', ';', ')'])?);
            }
            if self.take(";") {
                rows.push(std::mem::take(&mut args));
            } else if !self.take(",") {
                self.take(")");
                break;
            }
            self.trim();
        }
        let a = args.first().map_or("", String::as_str);
        let b = args.get(1).map_or("", String::as_str);
        let style = match name {
            "bold" => "mathbf",
            "bb" => "mathbb",
            "cal" => "mathcal",
            "upright" | "serif" => "mathrm",
            "italic" => "mathit",
            "sans" => "mathsf",
            "frak" => "mathfrak",
            "mono" => "mathtt",
            "hat" => "hat",
            "tilde" => "tilde",
            "macron" | "overline" => "overline",
            "underline" => "underline",
            "dot" => "dot",
            "dot.double" | "diaer" => "ddot",
            "acute" => "acute",
            "grave" => "grave",
            "breve" => "breve",
            "caron" => "check",
            _ => "",
        };
        let tex = if !style.is_empty() {
            format!("\\{style}{{{a}}}")
        } else {
            match name {
                "text" => args.last().cloned().unwrap_or_default(),
                "sqrt" => format!("\\sqrt{{{a}}}"),
                "root" => format!("\\sqrt[{a}]{{{b}}}"),
                "frac" => format!("\\frac{{{a}}}{{{b}}}"),
                "binom" => format!("\\binom{{{a}}}{{{b}}}"),
                "round" => format!("\\left\\lfloor {{{a}}} \\right\\rceil"),
                "floor" | "ceil" | "norm" | "abs" => {
                    let (l, r) = match name {
                        "floor" => ("\\lfloor", "\\rfloor"),
                        "ceil" => ("\\lceil", "\\rceil"),
                        "norm" => ("\\|", "\\|"),
                        _ => ("|", "|"),
                    };
                    if name == "abs" && !a.is_empty() {
                        format!("|{a}|")
                    } else {
                        format!(
                            "\\left{l} {} \\right{r}",
                            if a.is_empty() { "{}" } else { a }
                        )
                    }
                }
                "limits" => format!("{a}\\limits"),
                "scripts" => a.into(),
                "op" => format!(
                    "\\operatorname{{{}}}{}",
                    a.trim_start_matches("\\text{").trim_end_matches('}'),
                    if named.contains_key("limits") {
                        "\\limits"
                    } else {
                        ""
                    }
                ),
                "arrow" => format!("\\overset{{\\rightarrow}}{{{a}}}"),
                "overbrace" => format!("\\overset{{{b}}}{{\\overbrace{{{a}}}}}"),
                "underbrace" => format!("\\underset{{{b}}}{{\\underbrace{{{a}}}}}"),
                "accent" => {
                    let accent = match b {
                        "´" => "acute",
                        "`" => "grave",
                        "~" | "\\sim" => "tilde",
                        _ => "hat",
                    };
                    format!("\\{accent}{{{a}}}")
                }
                "lr" => {
                    let body = args.join(",");
                    if let Some(rest) = body
                        .strip_prefix("\\langle")
                        .and_then(|s| s.strip_suffix("\\rangle"))
                    {
                        format!("\\left\\langle {} \\right\\rangle", rest.trim())
                    } else if let Some(rest) = body.strip_suffix('|') {
                        format!("\\left. {} \\right|", rest.trim())
                    } else {
                        format!("\\left. {body} \\right.")
                    }
                }
                "attach" => {
                    let mut left = String::new();
                    let mut right = String::new();
                    for (key, mark, dest) in [("bl", '_', &mut left), ("br", '_', &mut right)] {
                        if let Some(v) = named.get(key) {
                            dest.push_str(&format!("{mark}{{{v}}}"));
                        }
                    }
                    if let Some(v) = named.get("tl") {
                        left.push_str(&format!("^{{{v}}}"));
                    }
                    if let Some(v) = named.get("tr") {
                        right.push_str(&format!("^{{{v}}}"));
                    }
                    format!(
                        "{}{left}{a}{right}",
                        if left.is_empty() { "" } else { "{}" }
                    )
                }
                "mat" => {
                    rows.push(args);
                    let delim = named
                        .get("delim")
                        .map(String::as_str)
                        .unwrap_or("\\text{(}");
                    match delim {
                        "\\text{[}" => environment("bmatrix", rows),
                        "\\text{{}" => environment("Bmatrix", rows),
                        "\\text{|}" => format!("\\left| {} \\right|", environment("matrix", rows)),
                        _ => environment("pmatrix", rows),
                    }
                }
                "vec" | "cases" => {
                    let rows = args
                        .into_iter()
                        .map(|value| vec![value.replace('&', " & ")])
                        .collect();
                    environment(if name == "cases" { "cases" } else { "pmatrix" }, rows)
                }
                _ => format!("{name}({})", args.join(",")),
            }
        };
        Ok(Atom::ordinary(tex))
    }
}

fn join_rows(rows: Vec<Vec<String>>) -> String {
    rows.into_iter()
        .map(|row| row.join(" & "))
        .collect::<Vec<_>>()
        .join(" \\\\\n")
}

fn environment(name: &str, rows: Vec<Vec<String>>) -> String {
    format!("\\begin{{{name}}}\n{}\n\\end{{{name}}}", join_rows(rows))
}

fn is_operator(name: &str) -> bool {
    matches!(
        name,
        "sin"
            | "cos"
            | "tan"
            | "sinh"
            | "cosh"
            | "tanh"
            | "arcsin"
            | "arccos"
            | "arctan"
            | "sec"
            | "csc"
            | "cot"
            | "coth"
            | "det"
            | "dim"
            | "exp"
            | "ker"
            | "log"
            | "ln"
            | "lg"
            | "lim"
            | "liminf"
            | "limsup"
            | "min"
            | "max"
            | "inf"
            | "sup"
            | "arg"
            | "deg"
            | "gcd"
    )
}

fn is_operator_with_limits(tex: &str) -> bool {
    matches!(tex, "\\lim" | "\\liminf" | "\\limsup")
}

fn unicode(c: char) -> Option<&'static str> {
    Some(match c {
        'ς' => "\\varsigma",
        '∅' | '⌀' => "\\varnothing",
        '§' => "§",
        '⊘' => "\\oslash",
        '≍' => "\\asymp",
        '≐' => "\\doteq",
        '𝚤' => "\\imath",
        '𝚥' => "j",
        'α' => "\\alpha",
        'β' => "\\beta",
        'π' => "\\pi",
        '∞' => "\\infty",
        _ => return None,
    })
}

fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "\\alpha",
        "beta" => "\\beta",
        "gamma" => "\\gamma",
        "Gamma" => "\\Gamma",
        "delta" => "\\delta",
        "Delta" => "\\Delta",
        "epsilon" => "\\varepsilon",
        "epsilon.alt" => "\\epsilon",
        "zeta" => "\\zeta",
        "eta" => "\\eta",
        "theta" => "\\theta",
        "Theta" => "\\Theta",
        "theta.alt" => "\\vartheta",
        "iota" => "\\iota",
        "kappa" => "\\kappa",
        "lambda" => "\\lambda",
        "Lambda" => "\\Lambda",
        "mu" => "\\mu",
        "nu" => "\\nu",
        "xi" => "\\xi",
        "Xi" => "\\Xi",
        "pi" => "\\pi",
        "Pi" => "\\Pi",
        "pi.alt" => "\\varpi",
        "rho" => "\\rho",
        "rho.alt" => "\\varrho",
        "sigma" => "\\sigma",
        "Sigma" => "\\Sigma",
        "tau" => "\\tau",
        "upsilon" => "\\upsilon",
        "Upsilon" => "\\Upsilon",
        "phi" => "\\varphi",
        "Phi" => "\\Phi",
        "phi.alt" => "\\phi",
        "chi" => "\\chi",
        "psi" => "\\psi",
        "Psi" => "\\Psi",
        "omega" => "\\omega",
        "Omega" => "\\Omega",
        "nabla" => "\\nabla",
        "partial" => "\\partial",
        "ell" => "\\ell",
        "oo" | "infinity" => "\\infty",
        "emptyset" | "diameter" | "nothing" => "\\varnothing",
        "union" => "\\cup",
        "inter" => "\\cap",
        "forall" => "\\forall",
        "exists" => "\\exists",
        "subset" => "\\subset",
        "subset.not" => "⊄",
        "planck" => "ħ",
        "subset.eq" => "\\subseteq",
        "supset" => "\\supset",
        "supset.eq" => "\\supseteq",
        "in" => "\\in",
        "in.not" => "\\notin",
        "not" => "\\neg",
        "and" => "\\land",
        "or" => "\\vee",
        "models" => "\\models",
        "tack.r" => "\\vdash",
        "without" => "\\smallsetminus",
        "complement" => "\\complement",
        "dot" | "dot.op" => "\\cdot",
        "times" => "\\times",
        "div" => "\\div",
        "ast" => "\\ast",
        "plus.minus" => "\\pm",
        "minus.plus" => "\\mp",
        "lt.eq" => "\\leq",
        "gt.eq" => "\\geq",
        "lt.double" => "\\ll",
        "gt.double" => "\\gg",
        "eq.not" => "\\neq",
        "approx" => "\\approx",
        "equiv" => "\\equiv",
        "mod" => "\\operatorname{mod}",
        "prec" => "\\prec",
        "prec.eq" => "\\preceq",
        "succ" => "\\succ",
        "succ.eq" => "\\succeq",
        "angle" => "\\angle",
        "angle.arc" => "\\measuredangle",
        "parallel" => "\\parallel",
        "tilde" => "\\sim",
        "tilde.eq" => "\\simeq",
        "tilde.not" => "\\nsim",
        "tilde.equiv" => "\\cong",
        "tilde.equiv.not" => "\\ncong",
        "divides" => "\\mid",
        "divides.not" => "\\nmid",
        "plus.o" => "\\oplus",
        "minus.o" => "\\ominus",
        "times.o" => "\\otimes",
        "dot.o" => "\\odot",
        "circle.stroked.tiny" => "\\circ",
        "degree" => "{^\\circ}",
        "star" => "\\star",
        "prop" => "\\propto",
        "harpoon.tr" => "\\upharpoonright",
        "arrow.r" => "\\rightarrow",
        "arrow.l" => "\\leftarrow",
        "arrow.t" => "\\uparrow",
        "arrow.b" => "\\downarrow",
        "arrow.r.not" => "\\nrightarrow",
        "arrow.r.long" => "\\longrightarrow",
        "arrow.r.bar" => "\\mapsto",
        "arrow.r.long.bar" => "\\longmapsto",
        "arrow.l.r.long" => "\\longleftrightarrow",
        "arrow.r.double" => "\\Rightarrow",
        "arrow.r.double.not" => "\\nRightarrow",
        "arrow.r.double.long" => "\\Longrightarrow",
        "arrow.squiggly" => "\\rightsquigarrow",
        "arrow.t.b" => "\\updownarrow",
        "sum" => "\\sum",
        "integral" => "\\int",
        "integral.double" => "\\iint",
        "integral.triple" => "\\iiint",
        "integral.cont" => "\\oint",
        "union.big" => "\\bigcup",
        "inter.big" => "\\bigcap",
        "chevron.l" => "\\langle",
        "chevron.r" => "\\rangle",
        "paren.l" => "(",
        "paren.r" => ")",
        "dots.h" => "\\ldots",
        "dots.h.c" => "\\cdots",
        "dots.v" => "\\vdots",
        "dots.down" => "\\ddots",
        "thin" => "\\, ",
        "med" | "medium" => "\\: ",
        "thick" => "\\; ",
        "quad" => "\\quad",
        "qquad" => "\\qquad",
        "prime" => "'",
        "prime.double" => "''",
        "dif" => "d",
        _ => return None,
    })
}
