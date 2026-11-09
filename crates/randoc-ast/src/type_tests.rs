//! Tests for the behaviour of the document model types.

use crate::walk::{Visitor, query};
use crate::{Attr, Block, Format, Inline, Pandoc};

#[test]
fn format_names_are_equal_without_regard_to_letter_case() {
    // Pandoc 3.13 writes a raw block with the format `HTML` or `Html` in HTML
    // output, and it does not write a raw block with the format `latex`.
    assert_eq!(Format::new("HTML"), Format::new("html"));
    assert_eq!(Format::new("Html"), Format::new("hTML"));
    assert_eq!(Format::new("html"), Format::new("html"));
    assert_ne!(Format::new("html"), Format::new("latex"));
    assert_ne!(Format::new("html"), Format::new("html5"));
    assert_ne!(Format::new(""), Format::new("html"));
}

#[test]
fn a_format_compares_with_text_without_regard_to_letter_case() {
    let format = Format::new("LaTeX");
    assert!(format == *"latex");
    assert!(format == *"LaTeX");
    assert!(format != *"tex");
    assert!(format == "latex");
    assert!(format == "LATEX");
    assert!(format != "html");
    assert_eq!(format.as_str(), "LaTeX");
}

#[test]
fn query_shows_each_element_to_the_visitor() {
    #[derive(Default)]
    struct Count {
        blocks: usize,
        inlines: usize,
    }
    impl Visitor for Count {
        fn block(&mut self, _: &Block) {
            self.blocks += 1;
        }
        fn inline(&mut self, _: &Inline) {
            self.inlines += 1;
        }
    }
    let doc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Div(
            Attr::default(),
            vec![Block::Para(vec![
                Inline::Emph(vec![Inline::Str("a".into())]),
                Inline::Space,
            ])],
        )],
    };
    let mut count = Count::default();
    query(&doc, &mut count);
    assert_eq!((count.blocks, count.inlines), (2, 3));
}

#[test]
fn attributes_report_classes_and_the_first_value_of_a_key() {
    let attr = Attr::new(
        "id",
        vec!["a".into(), "b".into()],
        vec![("k".into(), "1".into()), ("k".into(), "2".into())],
    );
    assert!(attr.has_class("b"));
    assert!(!attr.has_class("c"));
    assert_eq!(attr.get("k"), Some("1"));
    assert_eq!(attr.get("none"), None);
    assert!(!attr.is_empty());
    assert!(Attr::default().is_empty());
    assert!(!Attr::new("x", vec![], vec![]).is_empty());
    assert!(!Attr::new("", vec!["c".into()], vec![]).is_empty());
    assert!(!Attr::new("", vec![], vec![("k".into(), "v".into())]).is_empty());
}
