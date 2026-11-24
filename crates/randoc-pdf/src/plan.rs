use randoc_core::{Error, FormatSpec, Result, Text};

macro_rules! engines {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        /// The PDF engines in Pandoc's option error order.
        pub const PDF_ENGINES: [&str; 15] = [$($name),+];

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub(crate) enum Engine {$($variant),+}

        impl Engine {
            fn from_program(program: &str) -> Option<Self> {
                match program_stem(program) {
                    $($name => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

engines! {
    WeasyPrint => "weasyprint",
    Wkhtmltopdf => "wkhtmltopdf",
    PagedJs => "pagedjs-cli",
    Prince => "prince",
    PdfLatex => "pdflatex",
    LuaLatex => "lualatex",
    XeLatex => "xelatex",
    LatexMk => "latexmk",
    Tectonic => "tectonic",
    PdfLatexDev => "pdflatex-dev",
    LuaLatexDev => "lualatex-dev",
    Groff => "groff",
    PdfRoff => "pdfroff",
    Typst => "typst",
    Context => "context",
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Family {
    Latex,
    Html,
    Ms,
    Typst,
    Context,
}

impl Engine {
    fn family(self) -> Family {
        match self {
            Self::WeasyPrint | Self::Wkhtmltopdf | Self::PagedJs | Self::Prince => Family::Html,
            Self::PdfLatex
            | Self::LuaLatex
            | Self::XeLatex
            | Self::LatexMk
            | Self::Tectonic
            | Self::PdfLatexDev
            | Self::LuaLatexDev => Family::Latex,
            Self::Groff | Self::PdfRoff => Family::Ms,
            Self::Typst => Family::Typst,
            Self::Context => Family::Context,
        }
    }

    pub(crate) fn reruns(self) -> bool {
        matches!(
            self,
            Self::PdfLatex | Self::LuaLatex | Self::XeLatex | Self::PdfLatexDev | Self::LuaLatexDev
        )
    }

    pub(crate) fn temp_file_extension(self) -> Option<&'static str> {
        match self.family() {
            Family::Html => Some("html"),
            Family::Typst => Some("typ"),
            _ => None,
        }
    }

    pub(crate) fn converts_svg(self, program: &str) -> bool {
        // Pandoc converts images only for bare LaTeX program names.
        self.family() == Family::Latex && program == program_stem(program)
    }
}

impl Family {
    fn from_writer(writer: &str) -> Option<Self> {
        match writer {
            "latex" | "beamer" => Some(Self::Latex),
            "html" | "html5" => Some(Self::Html),
            "ms" => Some(Self::Ms),
            "typst" => Some(Self::Typst),
            "context" => Some(Self::Context),
            _ => None,
        }
    }

    fn writer(self) -> &'static str {
        match self {
            Self::Latex => "latex",
            Self::Html => "html",
            Self::Ms => "ms",
            Self::Typst => "typst",
            Self::Context => "context",
        }
    }

    fn default_program(self) -> (&'static str, Engine) {
        match self {
            Self::Latex => ("pdflatex", Engine::PdfLatex),
            Self::Html => ("weasyprint", Engine::WeasyPrint),
            Self::Ms => ("groff", Engine::Groff),
            Self::Typst => ("typst", Engine::Typst),
            Self::Context => ("context", Engine::Context),
        }
    }
}

/// A compatible intermediate writer and PDF engine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PdfPlan {
    writer: Text,
    program: Text,
    pub(crate) engine: Engine,
}

impl PdfPlan {
    /// Returns the writer specification, including any extension changes.
    pub fn writer(&self) -> &str {
        &self.writer
    }

    /// Returns the engine program, including any path supplied by the caller.
    pub fn program(&self) -> &str {
        &self.program
    }
}

/// Checks a program from `--pdf-engine` and returns the option error for an unknown engine.
pub fn validate_pdf_engine(program: &str) -> Result<()> {
    Engine::from_program(program).map(|_| ()).ok_or_else(|| {
        Error::Option(format!(
            "Argument of --pdf-engine must be one of\n\t{}\n",
            PDF_ENGINES.join("\n\t")
        ))
    })
}

fn program_stem(program: &str) -> &str {
    let name = program
        .rsplit_once(|ch| ch == '/' || (cfg!(windows) && ch == '\\'))
        .map_or(program, |(_, name)| name);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

/// Selects the writer and engine after the caller selects PDF output.
///
/// Pass the original target specification as `writer`. An absent target or
/// the exact target `pdf` selects the writer from the engine, or selects
/// LaTeX if the engine is absent. Call [`validate_pdf_engine`] during option
/// parsing for command-line values. Pass defaults-file values here directly.
pub fn resolve_pdf_plan(writer: Option<&str>, program: Option<&str>) -> Result<PdfPlan> {
    let explicit_engine = program.and_then(Engine::from_program);
    let writer = writer.filter(|writer| *writer != "pdf");
    let Some(writer) = writer else {
        let family = explicit_engine.map_or(Family::Latex, Engine::family);
        let (program, engine) = match (program, explicit_engine) {
            (Some(program), Some(engine)) => (program, engine),
            (Some(program), None) => {
                return Err(Error::App(format!(
                    "pdf-engine {} not known",
                    program_stem(program)
                )));
            }
            (None, _) => family.default_program(),
        };
        return Ok(PdfPlan {
            writer: family.writer().into(),
            program: program.into(),
            engine,
        });
    };

    let base = FormatSpec::parse(writer).name;
    let family = Family::from_writer(&base);
    let (program, engine) = match (program, explicit_engine) {
        (Some(program), Some(engine)) if family == Some(engine.family()) => (program, engine),
        (Some(program), _) => {
            return Err(Error::App(format!(
                "pdf-engine {program} is not compatible with output format {base}"
            )));
        }
        (None, _) if base == "pdf" => return Err(Error::App("pdf writer".into())),
        (None, _) => family
            .map(Family::default_program)
            .ok_or_else(|| Error::App(format!("cannot produce pdf output from {base}")))?,
    };

    Ok(PdfPlan {
        writer: writer.into(),
        program: program.into(),
        engine,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_the_default_engine_for_each_writer() -> Result<()> {
        for (writer, engine) in [
            ("latex", "pdflatex"),
            ("beamer", "pdflatex"),
            ("context", "context"),
            ("html", "weasyprint"),
            ("html5", "weasyprint"),
            ("ms", "groff"),
            ("typst", "typst"),
        ] {
            let plan = resolve_pdf_plan(Some(writer), None)?;
            assert_eq!(plan.writer(), writer);
            assert_eq!(plan.program(), engine);
        }
        assert_eq!(resolve_pdf_plan(None, None)?.writer(), "latex");
        assert_eq!(resolve_pdf_plan(Some("pdf"), None)?.program(), "pdflatex");
        Ok(())
    }

    #[test]
    fn preserves_a_program_path_and_writer_extensions() -> Result<()> {
        let path = "/work/tool bin/xelatex.exe";
        let plan = resolve_pdf_plan(None, Some(path))?;
        assert_eq!(plan.writer(), "latex");
        assert_eq!(plan.program(), path);
        let plan = resolve_pdf_plan(Some("BEAMER+smart"), Some(path))?;
        assert_eq!(plan.writer(), "BEAMER+smart");
        assert_eq!(plan.engine, Engine::XeLatex);
        Ok(())
    }

    #[test]
    fn validates_the_option_in_oracle_order() {
        let expected = Error::Option(
            "Argument of --pdf-engine must be one of\n\tweasyprint\n\twkhtmltopdf\n\tpagedjs-cli\n\tprince\n\tpdflatex\n\tlualatex\n\txelatex\n\tlatexmk\n\ttectonic\n\tpdflatex-dev\n\tlualatex-dev\n\tgroff\n\tpdfroff\n\ttypst\n\tcontext\n".into(),
        );
        assert_eq!(validate_pdf_engine("foo"), Err(expected.clone()));
        assert_eq!(validate_pdf_engine("/no/PDFLATEX"), Err(expected.clone()));
        assert_eq!(validate_pdf_engine("pdflatex/"), Err(expected.clone()));
        assert_eq!(expected.exit_code(), 6);
    }

    #[test]
    fn unknown_defaults_engines_use_exit_4() {
        for (writer, program, message) in [
            (None, "foo", "pdf-engine foo not known"),
            (Some("pdf"), "foo", "pdf-engine foo not known"),
            (None, "/x/y/foo.exe", "pdf-engine foo not known"),
            (None, "/x/pdflatex/", "pdf-engine  not known"),
            (None, "/x/pdflatex/.", "pdf-engine  not known"),
            (None, ".exe", "pdf-engine  not known"),
            (None, "foo.bar.exe", "pdf-engine foo.bar not known"),
            (
                Some("latex"),
                "/x/y/foo.exe",
                "pdf-engine /x/y/foo.exe is not compatible with output format latex",
            ),
            (
                Some("html4"),
                "foo",
                "pdf-engine foo is not compatible with output format html4",
            ),
        ] {
            let error = resolve_pdf_plan(writer, Some(program)).unwrap_err();
            assert_eq!(error, Error::App(message.into()));
            assert_eq!(error.exit_code(), 4);
            assert_eq!(validate_pdf_engine(program).unwrap_err().exit_code(), 6);
        }
    }

    #[test]
    fn rejects_incompatible_and_unsupported_writers() {
        assert_eq!(
            resolve_pdf_plan(Some("html4"), None),
            Err(Error::App("cannot produce pdf output from html4".into()))
        );
        assert_eq!(
            resolve_pdf_plan(Some("man"), Some("typst")),
            Err(Error::App(
                "pdf-engine typst is not compatible with output format man".into()
            ))
        );
        assert_eq!(
            resolve_pdf_plan(Some("pdf+smart"), None),
            Err(Error::App("pdf writer".into()))
        );
    }
}
