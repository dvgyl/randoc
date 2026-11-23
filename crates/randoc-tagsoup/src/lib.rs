//! HTML tokenization and rendering.

#![forbid(unsafe_code)]

mod tokens;

pub use tokens::{
    RenderOptions, Token, TokenKind, escape_into, render, render_close, render_comment,
    render_open, show_token, tokenize,
};

#[cfg(test)]
mod tests {
    use super::*;

    const OPTIONS: RenderOptions = RenderOptions {
        minimize: |name| {
            matches!(
                name,
                "br" | "col" | "hr" | "img" | "link" | "meta" | "path" | "rect" | "use"
            )
        },
        raw: |name| matches!(name, "script" | "style"),
    };

    fn render_html(html: &str) -> String {
        let tokens: Vec<_> = tokenize(html).into_iter().map(|token| token.kind).collect();
        let mut output = String::new();
        render(&tokens, &OPTIONS, &mut output);
        output
    }

    #[test]
    fn raw_text_ends_at_an_exact_case_close_tag() {
        let html = "<style>f</STYLE>&amp;<STYLE>&amp;</STYLE>&lt;</style>&amp;";
        assert_eq!(
            render_html(html),
            "<style>f</STYLE>&<STYLE>&</STYLE><</style>&amp;"
        );
        assert_eq!(
            render_html("<SCRIPT>a < b && c</SCRIPT>"),
            "<SCRIPT>a < b && c</SCRIPT>"
        );
    }

    #[test]
    fn minimizes_only_matching_empty_tags() {
        for tag in [
            "br", "col", "hr", "img", "link", "meta", "path", "rect", "use", "IMG",
        ] {
            assert_eq!(
                render_html(&format!("<{tag}></{tag}>")),
                format!("<{tag} />")
            );
        }
        assert_eq!(
            render_html("<img></IMG><br>x</br><div/>"),
            "<img></IMG><br>x</br><div></div>"
        );
    }

    #[test]
    fn renders_comments_processing_instructions_and_entities() {
        assert_eq!(
            render_html(
                r#"<?xml version="1.0"?><img alt="" title="&copy=2"/>&nbsp;&lt;&gt;&quot;&apos;&amp;"#
            ),
            "<?xml version=\"1.0\" ?><img alt title=\"©=2\" />\u{a0}&lt;&gt;&quot;&#39;&amp;",
        );
        let mut output = String::new();
        render(&[TokenKind::Comment("x-->y".into())], &OPTIONS, &mut output);
        assert_eq!(output, "<!--x-- >y-->");
    }
}
