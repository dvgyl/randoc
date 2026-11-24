//! Clean-room Markdown, from the CommonMark specification and black-box
//! runs of the oracle. `Org Mode` includes the context `Normal Text`. The
//! first lines of a document can be metadata; a block quote lasts to the
//! next empty line.

use super::{Ctx, Lang, M, Switch::*, opts};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    let lang = Lang::new("Markdown", "markdown", &["*.md", "*.markdown", "*.mmd"])
        // Metadata lines at the start of the document, or front matter.
        .context("Start", Normal, |c| {
            c.rule(M::Re("^---$"), Comment, Push("Front Matter"))
                .rule(M::Re(METADATA_KEY), Annotation, Push("Metadata Value"))
                .fallthrough(Push("Normal Text"))
                .line_empty(Push("Normal Text"))
        })
        .context("Metadata Value", Comment, |c| c.line_end(Pop(1)))
        .context("Front Matter", Comment, |c| {
            c.rule(
                M::Re(r"^(?:---|\.\.\.)$"),
                Comment,
                PopPush(1, "Normal Text"),
            )
            .rule(M::Re(METADATA_KEY), Annotation, Push("Metadata Value"))
        })
        .context("Normal Text", Normal, |c| {
            blocks(c.rule(M::Re(r"^\s*>"), Attribute, Push("Block Quote"))).include("Inline")
        })
        // A list item ends a block quote.
        .context("Block Quote", Attribute, |c| {
            let c = c.rule_with(M::Re(LIST_MARKER), Normal, Pop(1), opts().lookahead());
            blocks(c).include("Inline").line_empty(Pop(1))
        })
        .context("Inline", Normal, |c| {
            c.rule(M::Re("``.*?``|`[^`]*`"), Information, Stay)
                .rule(
                    M::Re(r"<(?:[A-Za-z][\w+.-]*:[^>\s]*|[^>\s@]+@[^>\s]+)>"),
                    Other,
                    Stay,
                )
                .rule(M::Re(IMAGE), Alert, Stay)
                .rule(M::Re(r"\[(?=[^\]]*\])"), Comment, Push("Link Text"))
                .rule(M::Str("<!--"), Comment, Push("HTML Comment"))
                .rule(M::Re("</?(?=[A-Za-z])"), DataType, Push("Tag Name"))
                .rule(
                    M::Re(r"&(?:#[0-9]+|#[xX][0-9A-Fa-f]+|[A-Za-z]\w*);"),
                    DecVal,
                    Stay,
                )
                .rule(M::Re(r"\\[\\`*_{}\[\]()#+\-.!|<>~]"), SpecialChar, Stay)
                .rule(M::Char('|'), Preprocessor, Stay)
        })
        .context("Link Text", Other, |c| {
            c.rule(M::Re(IMAGE), Alert, Stay).rule(
                M::Re(r"\](?:\([^)]*\)|\[[^\]]*\])?"),
                Comment,
                Pop(1),
            )
        })
        .context("HTML Comment", Comment, |c| {
            c.rule(M::Str("-->"), Comment, Pop(1))
        })
        .context("Tag Name", Keyword, |c| {
            c.rule(M::Re(r"[A-Za-z][\w-]*"), Keyword, PopPush(1, "Tag"))
                .fallthrough(Pop(1))
        })
        .context("Tag", Other, |c| {
            c.rule(M::Char('='), Operator, Stay)
                .rule(M::Re(r#""[^"]*"|'[^']*'"#), String, Stay)
                .rule(M::Re("/?>"), DataType, Pop(1))
        })
        .context("List Item", Normal, |c| {
            c.rule(M::Re(r"\[[ xX]\]"), Variable, Pop(1))
                .fallthrough(Pop(1))
        })
        .context("Fence", Information, |c| {
            c.rule(M::Re(FENCE_END), Information, Pop(1))
        });
    FENCES
        .iter()
        .fold(lang, |lang, (_, context, syntax)| {
            lang.context(context, Normal, |c| {
                let c = c
                    .rule_with(
                        M::Re(FENCE_END),
                        Information,
                        Pop(1),
                        opts().first_non_space(),
                    )
                    .include(syntax);
                match fallthrough(syntax) {
                    Some(target) => c.fallthrough(Push(target)),
                    None => c,
                }
            })
        })
        .build()
}

/// The block rules at the start of a line.
fn blocks(c: Ctx) -> Ctx {
    let c = FENCES.iter().fold(c, |c, (language, context, _)| {
        c.rule_pattern(
            format!(r"^\s*(?:```|~~~)\s*(?i:{language})\s*$"),
            Information,
            Push(context),
        )
    });
    c.rule(M::Re(r"^\s*(?:```|~~~).*$"), Information, Push("Fence"))
        .rule(M::Re(r"^#{1,6}(?:\s.*)?$"), Function, Stay)
        .rule(M::Re(r"^=+\s*$"), Function, Stay)
        .rule(M::Re(r"^-+\s*$"), Normal, Stay)
        .rule(M::Re(r"^\s*\[\^[^\]]*\]:\s*"), Other, Stay)
        .rule(M::Re(r"^\s*\[[^\]]+\]:\s*(?:[^\s<].*)?"), Other, Stay)
        .rule(
            M::Re(r"^(?=[ \t:|]*-)[ \t:|-]*\|[ \t:|-]*$"),
            Preprocessor,
            Stay,
        )
        .rule(M::Re(LIST_MARKER), SpecialString, Push("List Item"))
}

const LIST_MARKER: &str = r"^\s*(?:[-*+]|[0-9]+\.)[ \t]";

/// The context that an includer of a start context falls through to, as
/// `Org Mode` does: the start rules of these syntaxes do not match all text.
pub(super) fn fallthrough(include: &str) -> Option<&'static str> {
    match include {
        "##Bash" => Some("Command##Bash"),
        _ => None,
    }
}

const METADATA_KEY: &str = r"^[A-Za-z0-9][\w -]*:(?=\s|$)";

const IMAGE: &str = r"!\[[^\]]*\](?:\([^)]*\)|\[[^\]]*\])";

const FENCE_END: &str = r"(?:```|~~~)\s*$";

/// The languages of fenced code blocks: the name, the context of the
/// content, and the embedded syntax.
const FENCES: [(&str, &str, &str); 17] = [
    ("python|py", "Fence Python", "##Python"),
    ("javascript|js", "Fence JavaScript", "Normal##JavaScript"),
    ("java", "Fence Java", "##Java"),
    ("c", "Fence C", "##C"),
    (r"c\+\+|cpp", "Fence C++", "##C++"),
    ("bash|sh|shell", "Fence Bash", "##Bash"),
    ("ruby|rb", "Fence Ruby", "##Ruby"),
    ("rust|rs", "Fence Rust", "##Rust"),
    ("json", "Fence JSON", "##JSON"),
    ("yaml|yml", "Fence YAML", "##YAML"),
    ("xml", "Fence XML", "##XML"),
    ("html", "Fence HTML", "##HTML"),
    ("css", "Fence CSS", "##CSS"),
    ("haskell|hs", "Fence Haskell", "##Haskell"),
    ("latex|tex", "Fence LaTeX", "##LaTeX"),
    ("toml", "Fence TOML", "##TOML"),
    ("elixir", "Fence Elixir", "##Elixir"),
];

#[cfg(test)]
mod tests {
    use crate::registry::Registry;
    use crate::token::TokenType::{self, *};
    use crate::tokenizer::tokenize;

    /// The token types of the second line of `text` in Markdown.
    fn second_line(text: &str) -> Vec<TokenType> {
        let registry = Registry::embedded();
        let lines = registry
            .get("Markdown")
            .and_then(|syntax| tokenize(registry, syntax, text, 128).ok())
            .unwrap_or_default();
        lines
            .get(1)
            .map(|line| line.iter().map(|t| t.0).collect())
            .unwrap_or_default()
    }

    /// A named fence opens its language only at the start of a line.
    #[test]
    fn named_fences_start_at_column_zero() {
        assert_eq!(
            second_line("```python\nx = 1\n```\n"),
            [Normal, Operator, Normal, DecVal]
        );
        assert_eq!(second_line("a ```python\nx = 1\n"), [Normal]);
    }
}
