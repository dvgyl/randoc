//! Options and block preparation for HTML slide decks.

use randoc_ast::{Attr, Block, Inline};
use randoc_core::shared::sections::prepare_slides;
use randoc_core::{Context, Result};

use crate::Version;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlideFormat {
    RevealJs,
    Slidy,
    DzSlides,
    Slideous,
    S5,
}

impl SlideFormat {
    /// Returns the slide format for a writer name.
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "revealjs" => Self::RevealJs,
            "slidy" => Self::Slidy,
            "dzslides" => Self::DzSlides,
            "slideous" => Self::Slideous,
            "s5" => Self::S5,
            _ => return None,
        })
    }

    pub(crate) fn version(self) -> Version {
        match self {
            Self::RevealJs | Self::DzSlides => Version::Html5,
            Self::Slidy | Self::Slideous | Self::S5 => Version::Html4,
        }
    }

    pub(crate) fn fragment_class(self) -> &'static str {
        match self {
            Self::RevealJs => "fragment",
            _ => "incremental",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Slides {
    pub(crate) format: SlideFormat,
    pub(crate) level: i64,
}

impl Slides {
    pub(crate) fn prepare<'a>(
        self,
        blocks: &'a [Block],
        untitled: &'a Block,
        max_depth: usize,
    ) -> Result<Vec<&'a Block>> {
        // Pass the shapes that prepare_slides needs. Keep the content in the
        // input tree so preparation uses no recursive clone or drop.
        let retained: Vec<_> = blocks
            .iter()
            .enumerate()
            .filter(|(index, block)| {
                !matches!(block, Block::HorizontalRule)
                    || !matches!(blocks.get(index + 1), Some(Block::Header(level, ..)) if *level == self.level)
            })
            .map(|(_, block)| block)
            .collect();
        let heading = |level| Block::Header(level, Attr::default(), Vec::new());
        let shape = |block: &Block| match block {
            Block::Header(level, ..) => heading(*level),
            Block::HorizontalRule => Block::HorizontalRule,
            Block::Div(_, blocks) => {
                let children = match blocks.first() {
                    Some(Block::Header(level, ..)) => vec![heading(*level)],
                    _ => Vec::new(),
                };
                Block::Div(Attr::default(), children)
            }
            _ => Block::Plain(Vec::new()),
        };
        let prepared = prepare_slides(
            retained.iter().map(|block| shape(block)).collect(),
            self.level,
            max_depth,
        )?;
        let added = prepared.len().saturating_sub(retained.len());
        let originals = retained.into_iter().zip(prepared.iter().skip(added));
        Ok(std::iter::repeat_n(untitled, added)
            .chain(originals.map(|(original, shape)| match shape {
                Block::Header(_, _, text) if self::untitled(text) => untitled,
                _ => original,
            }))
            .collect())
    }
}

pub(crate) fn untitled(inlines: &[Inline]) -> bool {
    matches!(inlines, [Inline::Str(marker)] if marker == "\0")
}

pub(crate) fn is_pause(block: &Block) -> bool {
    let Block::Para(inlines) = block else {
        return false;
    };
    matches!(
        inlines.as_slice(),
        [Inline::Str(a), Inline::Space, Inline::Str(b), Inline::Space, Inline::Str(c)]
            if a == "." && b == "." && c == "."
    )
}

pub(crate) fn dzslides_core(context: &mut Context) -> Result<String> {
    const PATH: &str = "dzslides/template.html";
    const MARKER: &str = "<!-- {{{{ dzslides core";
    let override_path = context
        .state
        .user_data_dir
        .as_ref()
        .map(|dir| format!("{dir}/{PATH}"));
    let suffix = |asset: &str| {
        asset
            .find(MARKER)
            .map_or_else(String::new, |start| asset[start..].to_owned())
    };
    if let Some(path) = override_path.filter(|path| context.host().file_exists(path)) {
        let max_bytes = usize::try_from(context.limits.max_resource_size).unwrap_or(usize::MAX);
        let bytes = context.read_file_limited(&path, max_bytes)?;
        let asset = std::str::from_utf8(&bytes)
            .map_err(|_| randoc_core::Error::App(format!("Invalid UTF-8 in {path}.")))?;
        return Ok(suffix(asset));
    }
    let bytes = randoc_data::data_file(PATH)
        .ok_or_else(|| randoc_core::Error::App(format!("Missing embedded data file {PATH}.")))?;
    let asset = std::str::from_utf8(bytes)
        .map_err(|_| randoc_core::Error::App(format!("Invalid UTF-8 in {PATH}.")))?;
    Ok(suffix(asset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use randoc_ast::{Pandoc, Target};
    use randoc_core::{Output, WriterOptions};

    const FORMATS: [&str; 5] = ["revealjs", "slidy", "dzslides", "slideous", "s5"];

    fn write(
        format: &str,
        blocks: Vec<Block>,
        options: &WriterOptions,
        depth: usize,
    ) -> Result<String> {
        let doc = Pandoc {
            meta: Default::default(),
            blocks,
        };
        let mut context = Context::pure();
        context.limits.max_nesting_depth = depth;
        let writer = crate::writer(format).unwrap();
        let Output::Text(text) = writer.write(&doc, options, &mut context)? else {
            panic!("The slide writer must return text.");
        };
        Ok(text)
    }

    fn paragraph(text: &str) -> Block {
        Block::Para(vec![Inline::Str(text.into())])
    }

    fn pause() -> Block {
        Block::Para(vec![
            Inline::Str(".".into()),
            Inline::Space,
            Inline::Str(".".into()),
            Inline::Space,
            Inline::Str(".".into()),
        ])
    }

    fn list(text: &str) -> Block {
        Block::BulletList(vec![vec![Block::Plain(vec![Inline::Str(text.into())])]])
    }

    #[test]
    fn all_five_writers_render_an_empty_slide() {
        for (format, expected) in FORMATS.into_iter().zip([
            "<section class=\"slide level6\">\n\n</section>",
            "<div class=\"slide section level6\">\n\n</div>",
            "<section class=\"slide level6\">\n\n</section>",
            "<div class=\"slide section level6\">\n\n</div>",
            "<div class=\"slide section level6\">\n\n</div>",
        ]) {
            assert_eq!(
                write(format, Vec::new(), &WriterOptions::default(), 128).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn reveal_groups_vertical_slides_and_keeps_background_attributes() {
        let mut options = WriterOptions::default();
        options.slide_level = Some(2);
        let blocks = vec![
            Block::Header(
                1,
                Attr::new("group", vec![], vec![]),
                vec![Inline::Str("Group".into())],
            ),
            Block::Header(
                2,
                Attr::new(
                    "slide",
                    vec!["custom".into()],
                    vec![("background-image".into(), "pic.png".into())],
                ),
                vec![Inline::Str("Slide".into())],
            ),
            paragraph("Body"),
        ];
        let expected = concat!(
            "<section>\n<section id=\"group\" class=\"title-slide slide level1\">\n",
            "<h1>Group</h1>\n\n</section>\n",
            "<section id=\"slide\" class=\"slide level2 custom\"\ndata-background-image=\"pic.png\">\n",
            "<h2 class=\"custom\" data-background-image=\"pic.png\">Slide</h2>\n",
            "<p>Body</p>\n</section></section>",
        );
        assert_eq!(write("revealjs", blocks, &options, 128).unwrap(), expected);
    }

    #[test]
    fn slidy_uses_h1_for_slide_headings() {
        let mut options = WriterOptions::default();
        options.slide_level = Some(2);
        let blocks = vec![
            Block::Header(2, Attr::default(), Vec::new()),
            Block::Header(3, Attr::default(), vec![Inline::Str("Inner".into())]),
        ];
        let expected = "<div class=\"slide section level2\">\n<h1></h1>\n<h3>Inner</h3>\n</div>";
        assert_eq!(write("slidy", blocks, &options, 128).unwrap(), expected);
    }

    #[test]
    fn incremental_flags_restore_after_divisions_quotes_and_notes() {
        let mut options = WriterOptions::default();
        options.incremental = true;
        let blocks = vec![
            Block::Div(
                Attr::new("", vec!["nonincremental".into()], vec![]),
                vec![list("Visible"), Block::BlockQuote(vec![list("Hidden")])],
            ),
            Block::Div(
                Attr::new("", vec!["notes".into()], vec![]),
                vec![list("Note")],
            ),
            pause(),
            list("Later"),
            pause(),
        ];
        let expected = concat!(
            "<section class=\"slide level6\">\n\n",
            "<ul>\n<li>Visible</li>\n</ul>\n",
            "<ul>\n<li class=\"fragment\">Hidden</li>\n</ul>\n",
            "<aside class=\"notes\">\n<ul>\n<li>Note</li>\n</ul>\n</aside>\n",
            "<div class=\"fragment\">\n<ul>\n<li class=\"fragment\">Later</li>\n</ul>\n</div>\n",
            "</section>",
        );
        assert_eq!(write("revealjs", blocks, &options, 128).unwrap(), expected);
    }

    #[test]
    fn reveal_links_keep_cross_file_urls() {
        let mut options = WriterOptions::default();
        options.identifier_prefix = "deck-".into();
        let link = |url| {
            Inline::Link(
                Attr::default(),
                vec![Inline::Str("Link".into())],
                Target::new(url, ""),
            )
        };
        let blocks = vec![Block::Para(vec![
            link("#/one"),
            Inline::Space,
            link("other.html#one"),
        ])];
        let result = write("revealjs", blocks, &options, 128).unwrap();
        assert!(result.contains("href=\"#//one\""));
        assert!(result.contains("href=\"other.html#one\""));
    }

    #[test]
    fn dzslides_reads_the_core_from_the_context_data_directory() {
        use randoc_core::PureHost;
        let mut host = PureHost::default();
        let core = "<!-- {{{{ dzslides core -->\n<script>test()</script>";
        host.files.insert(
            "data/dzslides/template.html".into(),
            format!("<p>Sample</p>\n{core}").into_bytes(),
        );
        let mut context = Context::new(Box::new(host));
        context.state.user_data_dir = Some("data".into());
        assert_eq!(dzslides_core(&mut context).unwrap(), core);
        context.limits.max_resource_size = 1;
        assert!(dzslides_core(&mut context).is_err());
    }

    #[test]
    fn slide_nesting_uses_the_context_limit_on_a_two_mib_stack() {
        let nested = |count, with_pauses| {
            let mut blocks = vec![list("Leaf")];
            for index in 0..count {
                if with_pauses {
                    blocks.insert(0, pause());
                }
                let class = if index % 2 == 0 {
                    "incremental"
                } else {
                    "nonincremental"
                };
                blocks = vec![Block::Div(
                    Attr::new("", vec![class.into()], vec![]),
                    blocks,
                )];
            }
            blocks
        };
        for format in FORMATS {
            std::thread::Builder::new()
                .stack_size(2 << 20)
                .spawn(move || {
                    let options = WriterOptions::default();
                    let valid = write(format, nested(136, false), &options, 150).unwrap();
                    assert!(valid.contains("Leaf"));
                    let paused = write(format, nested(40, true), &options, 128).unwrap();
                    let class = if format == "revealjs" {
                        "fragment"
                    } else {
                        "incremental"
                    };
                    assert_eq!(
                        paused.matches(&format!("<div class=\"{class}\">")).count(),
                        40
                    );
                    let error = write(format, nested(512, true), &options, 128).unwrap_err();
                    assert!(error.to_string().contains("nested too deeply"));
                })
                .unwrap()
                .join()
                .unwrap();
        }
    }
}
