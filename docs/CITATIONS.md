# Citations

Randoc includes an author-date style and locale tables for 60 language tags.
A locale supplies terms such as page labels and can also supply date formats
and punctuation rules. Randoc wrote the built-in Citation Style Language
(CSL) data under `MIT OR Apache-2.0`. You can use `--citeproc` with your
document and bibliography immediately.

```sh
randoc paper.md --citeproc --bibliography=references.bib -o paper.html
```

## Style and locale selection

Randoc selects the first style in this list.

1. The file from `--csl` or the document's `csl:` metadata. The command-line
   option takes precedence over the document metadata.
2. `default.csl` in the data directory.
3. The built-in [`randoc-author-date` style](../crates/randoc-data/data/csl/randoc-author-date.csl).

Randoc uses the document language, then the style's default locale, then
`en-US` to select a locale. In order of precedence, it uses these sources
for locale data.

1. Applicable `<locale>` elements in the style.
2. `csl/locales/locales-TAG.xml` in the data directory. Randoc tries the
   exact tag first, then the primary dialect. The file takes precedence
   over the built-in data for the fields it supplies.
3. The built-in data for the exact tag, then the primary dialect.
4. Built-in `en-US` for missing fields. If the selected locale is unavailable
   or its file is malformed, Randoc tries the `en-US` file before the
   built-in `en-US` locale.

## Use another style or install upstream files

For a journal style, get its CSL file and pass it to `--csl`.

```sh
randoc paper.md --citeproc --bibliography=references.bib --csl=journal.csl -o paper.html
```

To use byte-exact copies of the upstream `default.csl` and the 60 locale
XML files used with Pandoc 3.13, run the
[CSL download script](../tools/fetch-csl-locales.sh) from the Randoc
repository. These files use Creative Commons Attribution-ShareAlike 3.0
(CC BY-SA 3.0). Choose a data directory outside the repository and pass
the same directory to Randoc.

```sh
data_dir="$HOME/.local/share/randoc-csl"
sh tools/fetch-csl-locales.sh "$data_dir"
randoc paper.md --data-dir="$data_dir" --citeproc --bibliography=references.bib -o paper.html
```

## Built-in locales

The full locales supply general citation terms, date formats, and
punctuation rules.

| Tag | Language |
| --- | --- |
| `en-US` | English (United States) |
| `en-GB` | English (United Kingdom) |
| `de-DE` | German (Germany) |
| `fr-FR` | French (France) |
| `it-IT` | Italian (Italy) |
| `zh-CN` | Chinese (China) |
| `es-ES` | Spanish (Spain) |

All 60 tags in the [locale table](../crates/randoc-data/data/csl/locales.tsv)
supply page labels. With `--biblatex`, Randoc uses these labels to recognize
page references in citation suffixes so BibLaTeX can add its own label.
An absent or invalid language tag uses `en-US`. For a valid tag, Randoc
tries the exact tag and then the primary dialect. If both are unavailable,
Randoc does not recognize a page label for that tag.

| Tag | Tag | Tag | Tag | Tag | Tag |
| --- | --- | --- | --- | --- | --- |
| `af-ZA` | `ar` | `bal-PK` | `bg-BG` | `brh-PK` | `ca-AD` |
| `cs-CZ` | `cy-GB` | `da-DK` | `de-AT` | `de-CH` | `de-DE` |
| `el-GR` | `en-GB` | `en-US` | `es-CL` | `es-ES` | `es-MX` |
| `et-EE` | `eu` | `fa-IR` | `fi-FI` | `fr-CA` | `fr-FR` |
| `gl-ES` | `he-IL` | `hi-IN` | `hr-HR` | `hu-HU` | `id-ID` |
| `is-IS` | `it-IT` | `ja-JP` | `km-KH` | `ko-KR` | `la` |
| `lij-IT` | `lt-LT` | `lv-LV` | `mn-MN` | `ms-MY` | `nb-NO` |
| `nl-NL` | `nn-NO` | `pa-PK` | `pl-PL` | `pt-BR` | `pt-PT` |
| `ro-RO` | `ru-RU` | `sk-SK` | `sl-SI` | `sr-RS` | `sv-SE` |
| `th-TH` | `tr-TR` | `uk-UA` | `vi-VN` | `zh-CN` | `zh-TW` |
