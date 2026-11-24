//! Lightweight text and outline formats for Randoc.

#![forbid(unsafe_code)]

use std::sync::Arc;

use randoc_ast::Pandoc;
use randoc_core::{
    Context, Error, Reader, ReaderOptions, Result, Sources, TemplateWriter, Writer, WriterOptions,
};

mod inline;
mod opml;
mod read;
mod write;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Textile,
    Muse,
    T2t,
    Haddock,
    Opml,
    Vimdoc,
}

impl Format {
    fn get(name: &str) -> Option<Self> {
        Some(match name {
            "textile" => Self::Textile,
            "muse" => Self::Muse,
            "t2t" => Self::T2t,
            "haddock" => Self::Haddock,
            "opml" => Self::Opml,
            "vimdoc" => Self::Vimdoc,
            _ => return None,
        })
    }
}

/// Returns the reader for a supported input format.
pub fn reader(name: &str) -> Option<Reader> {
    let format = Format::get(name)?;
    if format == Format::Vimdoc {
        return None;
    }
    Some(Reader::Text(Arc::new(
        move |sources: &Sources, options: &ReaderOptions, context: &mut Context| {
            let size = sources
                .0
                .iter()
                .map(|source| source.text.len() as u64)
                .fold(0, u64::saturating_add);
            if size > context.limits.max_input_size {
                return Err(Error::App("The input is too large.".into()));
            }
            let input = sources.concat();
            if format == Format::Opml {
                opml::read(&input, options, context)
            } else {
                let macros = (format == Format::T2t).then(|| {
                    let input = context
                        .state
                        .input_files
                        .first()
                        .map_or("-", String::as_str)
                        .to_owned();
                    let output = context.state.output_file.clone().unwrap_or_default();
                    let clock = context.host();
                    let seconds = clock
                        .environment_variable("SOURCE_DATE_EPOCH")
                        .and_then(|value| value.trim().parse().ok())
                        .unwrap_or_else(|| clock.current_time().unix_seconds);
                    let mtime = if input == "-" {
                        String::new()
                    } else {
                        clock
                            .path_times(&input)
                            .ok()
                            .map_or_else(String::new, |times| {
                                let seconds = times.modified.unix_seconds.rem_euclid(86_400);
                                format!(
                                    "{:02}:{:02}:{:02}",
                                    seconds / 3600,
                                    seconds / 60 % 60,
                                    seconds % 60
                                )
                            })
                    };
                    inline::T2tMacros {
                        input,
                        output,
                        date: iso_date(seconds),
                        mtime,
                    }
                });
                read::read(format, &input, options, &context.limits, macros.as_ref())
            }
        },
    )))
}

/// Returns the writer for a supported output format.
pub fn writer(name: &str) -> Option<Writer> {
    let format = Format::get(name)?;
    Some(Writer::Text(Arc::new(TemplateWriter(
        move |doc: &Pandoc, options: &WriterOptions, context: &mut Context| {
            if format == Format::Opml {
                opml::write(doc, options, context)
            } else {
                write::write(format, doc, options, context)
            }
        },
    ))))
}

// Convert the days from the Unix epoch to a Gregorian calendar date.
fn iso_date(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn check_depth(depth: usize, limit: usize) -> Result<()> {
    if depth > limit {
        return Err(Error::App("The document is nested too deeply.".into()));
    }
    Ok(())
}

fn expansion_budget(size: usize, limit: u64) -> usize {
    size.max(1024)
        .saturating_mul(64)
        .min(usize::try_from(limit).unwrap_or(usize::MAX))
}

#[cfg(test)]
mod tests;
