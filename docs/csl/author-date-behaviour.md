# Pandoc 3.13 default author-date behaviour

The tables give observed output from `--citeproc` with no style argument.
All references and citation text are synthetic. Source IDs identify rows in
[the probe log](probe-log.tsv). Each log row gives the command, exact inputs,
observed citation content, bibliography order, layout attributes and standard error
where applicable. Run the commands with the environment from `env/env.sh`.

Italics and links show the formatting Pandoc emitted in HTML. Each citation cell
shows the body of the citation element. A dash means that the result has no
citation or no bibliography entry. Each bibliography cell gives the reference
ID and the contents of its entry. Line breaks in a cell separate results.

## How to use the matrix

Each row is an independent document with one reference and one citation,
`[@x]`. The first row of each table gives the baseline. Other rows change
only the fields in their input description. The source ID gives the full
Citation Style Language (CSL) JSON record. The type headings give the additional
baseline fields, including fields that Pandoc ignored for that type.

The base title is `the map of a river: a field guide`. The first author is
Ada J. Reed. The full author lists are in the log. The base publication date
is 24 September 2031. `article-with-genre` is type `article` with
`genre: Preprint`.

The tables record output for field combinations with one reference. Use the
scenario tables for effects that need more than one reference, such as sorting
and year suffixes. Apply output punctuation and links to the complete group
of fields shown in each row.

## Rules observed across probes

| Area | Observed behaviour | Source |
| --- | --- | --- |
| Citation names | Print one family name or join two family names with “and”. With three or more authors, print the first family name and “et al.”. For a book with no author, try the editor, then the translator, then the title. | `M01.01`–`M01.09`, `S52` |
| Year and status | Insert a space between the name and a numeric year. Insert a comma before the undated term or a status that replaces the year. Put the original year in brackets in a citation and in parentheses in the bibliography. | `M01.10`–`M01.15`, `S39`, `S40` |
| Locators | Insert a comma before the locator. Print page numbers without a label. Use the short term for non-page labels, such as “chap.”. Shorten page ranges such as 101–129 to 101–29. | `S08`, `S47` |
| Clusters | Keep citation order. Separate different authors with a semicolon and space. Group matching consecutive authors. Keep each year and locator when locators are present. | `S48`, `S49`, `S53` |
| Disambiguation | Assign suffix letters by bibliography order. Keep those letters when the citation order changes. Include initials when same-family names need disambiguation. Add a hyphen before the suffix on an undated term. | `S28`, `S37`, `S48`, `S53` |
| Bibliography names | Invert the first personal name. Use given-name-first order for the other names. Print all four names in a four-author book. Print three names and “et al.” in an eleven-author book. Show the editor or translator role if that name takes the place of the author. | `M01.04`–`M01.08` |
| Titles | Use title case for English titles in these probes. Italicize book titles. Put chapter and journal article titles in quotation marks. Preserve case in the tested German and French titles. | `M01.01`, `M02.01`, `M03.01`, `S03`, `S26`, `S27` |
| Publication | When both fields are present, print the publisher for the tested book and chapter. An anonymous book with only a publisher place prints that place. A date after a serial title depends on the type and the issue or volume fields. | `M01.01`, `M02.01`, `M03.01`, `M05.01`, `S31`, `S49` |
| Links and access | Prefer a DOI link when both DOI and URL are present. A dated web page with a URL omits the access date. An undated web page with a URL prints “Accessed” before the date. | `M09.20`, `S50` |
| Bibliography layout | The references list has a hanging indent. The output has no explicit entry-spacing or line-spacing attribute. A references title or a supplied references div can come from the document. | `M01.01`, `S16`, `S18` |

## Matrix inputs

### Changes to the baseline

A null value removes a field. The listed author count gives the number of names
in the baseline order.

| Input change | Fields |
| --- | --- |
| Editor only | <code>{&quot;author&quot;:null,&quot;editor&quot;:[{&quot;given&quot;:&quot;Eve&quot;,&quot;family&quot;:&quot;Holt&quot;}]}</code> |
| Translator | <code>{&quot;translator&quot;:[{&quot;given&quot;:&quot;Tam&quot;,&quot;family&quot;:&quot;Vale&quot;}]}</code> |
| Translator only | <code>{&quot;author&quot;:null,&quot;translator&quot;:[{&quot;given&quot;:&quot;Tam&quot;,&quot;family&quot;:&quot;Vale&quot;}]}</code> |
| No date | <code>{&quot;issued&quot;:null}</code> |
| Date range | <code>{&quot;issued&quot;:{&quot;date-parts&quot;:[[2030,12,29],[2031,1,2]]}}</code> |
| Season | <code>{&quot;issued&quot;:{&quot;date-parts&quot;:[[2031]],&quot;season&quot;:2}}</code> |
| Status without date | <code>{&quot;issued&quot;:null,&quot;status&quot;:&quot;forthcoming&quot;}</code> |
| Status with date | <code>{&quot;status&quot;:&quot;in press&quot;}</code> |
| Original date | <code>{&quot;original-date&quot;:{&quot;date-parts&quot;:[[1913]]}}</code> |
| Edition | <code>{&quot;edition&quot;:3}</code> |
| Number of volumes | <code>{&quot;number-of-volumes&quot;:4}</code> |
| Collection | <code>{&quot;collection-title&quot;:&quot;Survey series&quot;,&quot;collection-number&quot;:2}</code> |
| DOI | <code>{&quot;DOI&quot;:&quot;10.1234/river&quot;}</code> |
| URL and accessed | <code>{&quot;URL&quot;:&quot;https://example.org/maps&quot;,&quot;accessed&quot;:{&quot;date-parts&quot;:[[2032,1,4]]}}</code> |
| Short container title | <code>{&quot;container-title-short&quot;:&quot;Survey Q.&quot;}</code> |

The author order is <code>[{&quot;given&quot;:&quot;Ada J.&quot;,&quot;family&quot;:&quot;Reed&quot;},{&quot;given&quot;:&quot;Bo&quot;,&quot;family&quot;:&quot;Lake&quot;},{&quot;given&quot;:&quot;Cy&quot;,&quot;family&quot;:&quot;Baker&quot;},{&quot;given&quot;:&quot;Di&quot;,&quot;family&quot;:&quot;Moor&quot;},{&quot;given&quot;:&quot;Eli&quot;,&quot;family&quot;:&quot;Ford&quot;},{&quot;given&quot;:&quot;Flo&quot;,&quot;family&quot;:&quot;Green&quot;},{&quot;given&quot;:&quot;Gil&quot;,&quot;family&quot;:&quot;Banks&quot;},{&quot;given&quot;:&quot;Hal&quot;,&quot;family&quot;:&quot;Clark&quot;},{&quot;given&quot;:&quot;Ira&quot;,&quot;family&quot;:&quot;Dale&quot;},{&quot;given&quot;:&quot;Jo&quot;,&quot;family&quot;:&quot;Elm&quot;},{&quot;given&quot;:&quot;Kai&quot;,&quot;family&quot;:&quot;Snow&quot;}]</code>.

## Type and field matrix

### `book`

Additional baseline fields <code>{&quot;publisher&quot;:&quot;Map House&quot;,&quot;publisher-place&quot;:&quot;North Port&quot;}</code>.

| Input change | Citation | Bibliography entry | Source |
| --- | --- | --- | --- |
| 1 author | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. Map House. | `M01.01` |
| 2 authors | (Reed and Lake 2031) | `x` Reed, Ada J., and Bo Lake. 2031. <em>The Map of a River: A Field Guide</em>. Map House. | `M01.02` |
| 3 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, and Cy Baker. 2031. <em>The Map of a River: A Field Guide</em>. Map House. | `M01.03` |
| 4 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, and Di Moor. 2031. <em>The Map of a River: A Field Guide</em>. Map House. | `M01.04` |
| 11 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, et al. 2031. <em>The Map of a River: A Field Guide</em>. Map House. | `M01.05` |
| Editor only | (Holt 2031) | `x` Holt, Eve, ed. 2031. <em>The Map of a River: A Field Guide</em>. Map House. | `M01.06` |
| Translator | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. Translated by Tam Vale. Map House. | `M01.07` |
| Translator only | (Vale 2031) | `x` Vale, Tam, trans. 2031. <em>The Map of a River: A Field Guide</em>. Map House. | `M01.08` |
| No author | (<em>The Map of a River: A Field Guide</em> 2031) | `x` <em>The Map of a River: A Field Guide</em>. 2031. Map House. | `M01.09` |
| No date | (Reed, n.d.) | `x` Reed, Ada J. n.d. <em>The Map of a River: A Field Guide</em>. Map House. | `M01.10` |
| Date range | (Reed 2030–2031) | `x` Reed, Ada J. 2030–2031. <em>The Map of a River: A Field Guide</em>. Map House. | `M01.11` |
| Season | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. Map House. | `M01.12` |
| Status without date | (Reed, forthcoming) | `x` Reed, Ada J. Forthcoming. <em>The Map of a River: A Field Guide</em>. Map House. | `M01.13` |
| Status with date | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. Map House, in press. | `M01.14` |
| Original date | (Reed [1913] 2031) | `x` Reed, Ada J. (1913) 2031. <em>The Map of a River: A Field Guide</em>. Map House. | `M01.15` |
| Edition | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. 3rd ed. Map House. | `M01.16` |
| Number of volumes | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. 4 vols. Map House. | `M01.17` |
| Collection | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. Survey Series 2. Map House. | `M01.18` |
| DOI | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. Map House. <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>. | `M01.19` |
| URL and accessed | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. Map House. <a href="https://example.org/maps">https://example.org/maps</a>. | `M01.20` |
| Short container title | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. Map House. | `M01.21` |

### `chapter`

Additional baseline fields <code>{&quot;publisher&quot;:&quot;Map House&quot;,&quot;publisher-place&quot;:&quot;North Port&quot;,&quot;container-title&quot;:&quot;the atlas of river lands&quot;,&quot;page&quot;:&quot;101-129&quot;}</code>.

| Input change | Citation | Bibliography entry | Source |
| --- | --- | --- | --- |
| 1 author | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>. Map House. | `M02.01` |
| 2 authors | (Reed and Lake 2031) | `x` Reed, Ada J., and Bo Lake. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>. Map House. | `M02.02` |
| 3 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, and Cy Baker. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>. Map House. | `M02.03` |
| 4 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, and Di Moor. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>. Map House. | `M02.04` |
| 11 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, et al. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>. Map House. | `M02.05` |
| Editor only | (Holt 2031) | `x` Holt, Eve, ed. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>. Map House. | `M02.06` |
| Translator | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>, translated by Tam Vale. Map House. | `M02.07` |
| Translator only | (Vale 2031) | `x` Vale, Tam, trans. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>. Map House. | `M02.08` |
| No author | (<span>“The Map of a River: A Field Guide”</span> 2031) | `x` <span>“The Map of a River: A Field Guide.”</span> 2031. In <em>The Atlas of River Lands</em>. Map House. | `M02.09` |
| No date | (Reed, n.d.) | `x` Reed, Ada J. n.d. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>. Map House. | `M02.10` |
| Date range | (Reed 2030–2031) | `x` Reed, Ada J. 2030–2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>. Map House. | `M02.11` |
| Season | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>. Map House. | `M02.12` |
| Status without date | (Reed, forthcoming) | `x` Reed, Ada J. Forthcoming. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>. Map House. | `M02.13` |
| Status with date | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>. Map House, in press. | `M02.14` |
| Original date | (Reed [1913] 2031) | `x` Reed, Ada J. (1913) 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>. Map House. | `M02.15` |
| Edition | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>, 3rd ed. Map House. | `M02.16` |
| Number of volumes | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>, 4 vols. Map House. | `M02.17` |
| Collection | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>. Survey Series 2. Map House. | `M02.18` |
| DOI | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>. Map House. <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>. | `M02.19` |
| URL and accessed | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>. Map House. <a href="https://example.org/maps">https://example.org/maps</a>. | `M02.20` |
| Short container title | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Atlas of River Lands</em>. Map House. | `M02.21` |

### `article-journal`

Additional baseline fields <code>{&quot;container-title&quot;:&quot;the survey quarterly&quot;,&quot;volume&quot;:12,&quot;issue&quot;:3,&quot;page&quot;:&quot;101-129&quot;}</code>.

| Input change | Citation | Bibliography entry | Source |
| --- | --- | --- | --- |
| 1 author | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M03.01` |
| 2 authors | (Reed and Lake 2031) | `x` Reed, Ada J., and Bo Lake. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M03.02` |
| 3 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, and Cy Baker. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M03.03` |
| 4 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, and Di Moor. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M03.04` |
| 11 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, et al. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M03.05` |
| Editor only | (Holt 2031) | `x` Holt, Eve, ed. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M03.06` |
| Translator | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Translated by Tam Vale. <em>The Survey Quarterly</em> 12 (3): 101–29. | `M03.07` |
| Translator only | (Vale 2031) | `x` Vale, Tam, trans. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M03.08` |
| No author | (<span>“The Map of a River: A Field Guide”</span> 2031) | `x` <span>“The Map of a River: A Field Guide.”</span> 2031. <em>The Survey Quarterly</em> 12 (3): 101–29. | `M03.09` |
| No date | (Reed, n.d.) | `x` Reed, Ada J. n.d. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M03.10` |
| Date range | (Reed 2030–2031) | `x` Reed, Ada J. 2030–2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M03.11` |
| Season | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M03.12` |
| Status without date | (Reed, forthcoming) | `x` Reed, Ada J. Forthcoming. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M03.13` |
| Status with date | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M03.14` |
| Original date | (Reed [1913] 2031) | `x` Reed, Ada J. (1913) 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M03.15` |
| Edition | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M03.16` |
| Number of volumes | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M03.17` |
| Collection | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em>, Survey series, vol. 12 (3): 101–29. | `M03.18` |
| DOI | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>. | `M03.19` |
| URL and accessed | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. <a href="https://example.org/maps">https://example.org/maps</a>. | `M03.20` |
| Short container title | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M03.21` |

### `article-magazine`

Additional baseline fields <code>{&quot;container-title&quot;:&quot;the survey quarterly&quot;,&quot;volume&quot;:12,&quot;issue&quot;:3,&quot;page&quot;:&quot;101-129&quot;}</code>.

| Input change | Citation | Bibliography entry | Source |
| --- | --- | --- | --- |
| 1 author | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M04.01` |
| 2 authors | (Reed and Lake 2031) | `x` Reed, Ada J., and Bo Lake. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M04.02` |
| 3 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, and Cy Baker. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M04.03` |
| 4 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, and Di Moor. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M04.04` |
| 11 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, et al. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M04.05` |
| Editor only | (Holt 2031) | `x` Holt, Eve, ed. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M04.06` |
| Translator | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Translated by Tam Vale. <em>The Survey Quarterly</em> 12 (3): 101–29. | `M04.07` |
| Translator only | (Vale 2031) | `x` Vale, Tam, trans. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M04.08` |
| No author | (<em>The Survey Quarterly</em> 2031) | `x` <em>The Survey Quarterly</em>. 2031. <span>“The Map of a River: A Field Guide.”</span> Vol. 12 (3): 101–29. | `M04.09` |
| No date | (Reed, n.d.) | `x` Reed, Ada J. n.d. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M04.10` |
| Date range | (Reed 2030–2031) | `x` Reed, Ada J. 2030–2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M04.11` |
| Season | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M04.12` |
| Status without date | (Reed, forthcoming) | `x` Reed, Ada J. Forthcoming. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M04.13` |
| Status with date | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M04.14` |
| Original date | (Reed [1913] 2031) | `x` Reed, Ada J. (1913) 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M04.15` |
| Edition | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M04.16` |
| Number of volumes | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M04.17` |
| Collection | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em>, Survey series, vol. 12 (3): 101–29. | `M04.18` |
| DOI | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>. | `M04.19` |
| URL and accessed | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. <a href="https://example.org/maps">https://example.org/maps</a>. | `M04.20` |
| Short container title | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>The Survey Quarterly</em> 12 (3): 101–29. | `M04.21` |

### `article-newspaper`

Additional baseline fields <code>{&quot;container-title&quot;:&quot;the survey quarterly&quot;,&quot;volume&quot;:12,&quot;issue&quot;:3,&quot;page&quot;:&quot;101-129&quot;,&quot;section&quot;:&quot;Science&quot;,&quot;edition&quot;:&quot;Evening&quot;}</code>.

| Input change | Citation | Bibliography entry | Source |
| --- | --- | --- | --- |
| 1 author | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Science. <em>The Survey Quarterly</em>, September 24. | `M05.01` |
| 2 authors | (Reed and Lake 2031) | `x` Reed, Ada J., and Bo Lake. 2031. <span>“The Map of a River: A Field Guide.”</span> Science. <em>The Survey Quarterly</em>, September 24. | `M05.02` |
| 3 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, and Cy Baker. 2031. <span>“The Map of a River: A Field Guide.”</span> Science. <em>The Survey Quarterly</em>, September 24. | `M05.03` |
| 4 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, and Di Moor. 2031. <span>“The Map of a River: A Field Guide.”</span> Science. <em>The Survey Quarterly</em>, September 24. | `M05.04` |
| 11 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, et al. 2031. <span>“The Map of a River: A Field Guide.”</span> Science. <em>The Survey Quarterly</em>, September 24. | `M05.05` |
| Editor only | (Holt 2031) | `x` Holt, Eve, ed. 2031. <span>“The Map of a River: A Field Guide.”</span> Science. <em>The Survey Quarterly</em>, September 24. | `M05.06` |
| Translator | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Science. Translated by Tam Vale. <em>The Survey Quarterly</em>, September 24. | `M05.07` |
| Translator only | (Vale 2031) | `x` Vale, Tam, trans. 2031. <span>“The Map of a River: A Field Guide.”</span> Science. <em>The Survey Quarterly</em>, September 24. | `M05.08` |
| No author | (<em>The Survey Quarterly</em> 2031) | `x` <em>The Survey Quarterly</em>. 2031. <span>“The Map of a River: A Field Guide.”</span> Science. September 24. | `M05.09` |
| No date | (Reed, n.d.) | `x` Reed, Ada J. n.d. <span>“The Map of a River: A Field Guide.”</span> Science. <em>The Survey Quarterly</em>. | `M05.10` |
| Date range | (Reed 2030–2031) | `x` Reed, Ada J. 2030–2031. <span>“The Map of a River: A Field Guide.”</span> Science. <em>The Survey Quarterly</em>, December 29–January 2. | `M05.11` |
| Season | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Science. <em>The Survey Quarterly</em>, Summer. | `M05.12` |
| Status without date | (Reed, forthcoming) | `x` Reed, Ada J. Forthcoming. <span>“The Map of a River: A Field Guide.”</span> Science. <em>The Survey Quarterly</em>. | `M05.13` |
| Status with date | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Science. <em>The Survey Quarterly</em>, In press September 24. | `M05.14` |
| Original date | (Reed [1913] 2031) | `x` Reed, Ada J. (1913) 2031. <span>“The Map of a River: A Field Guide.”</span> Science. <em>The Survey Quarterly</em>, September 24. | `M05.15` |
| Edition | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Science. <em>The Survey Quarterly</em>, September 24. | `M05.16` |
| Number of volumes | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Science. <em>The Survey Quarterly</em>, September 24. | `M05.17` |
| Collection | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Science. <em>The Survey Quarterly</em>, Survey series, September 24. | `M05.18` |
| DOI | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Science. <em>The Survey Quarterly</em>, September 24. <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>. | `M05.19` |
| URL and accessed | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Science. <em>The Survey Quarterly</em>, September 24. <a href="https://example.org/maps">https://example.org/maps</a>. | `M05.20` |
| Short container title | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Science. <em>The Survey Quarterly</em>, September 24. | `M05.21` |

### `paper-conference`

Additional baseline fields <code>{&quot;publisher&quot;:&quot;Map House&quot;,&quot;publisher-place&quot;:&quot;North Port&quot;,&quot;container-title&quot;:&quot;proceedings of the survey congress&quot;,&quot;event-title&quot;:&quot;River Congress&quot;,&quot;event-place&quot;:&quot;East Port&quot;,&quot;page&quot;:&quot;101-129&quot;}</code>.

| Input change | Citation | Bibliography entry | Source |
| --- | --- | --- | --- |
| 1 author | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>Proceedings of the Survey Congress</em>, September 24, 101–29. | `M06.01` |
| 2 authors | (Reed and Lake 2031) | `x` Reed, Ada J., and Bo Lake. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>Proceedings of the Survey Congress</em>, September 24, 101–29. | `M06.02` |
| 3 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, and Cy Baker. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>Proceedings of the Survey Congress</em>, September 24, 101–29. | `M06.03` |
| 4 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, and Di Moor. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>Proceedings of the Survey Congress</em>, September 24, 101–29. | `M06.04` |
| 11 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, et al. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>Proceedings of the Survey Congress</em>, September 24, 101–29. | `M06.05` |
| Editor only | (Holt 2031) | `x` Holt, Eve, ed. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>Proceedings of the Survey Congress</em>. Map House. | `M06.06` |
| Translator | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Translated by Tam Vale. <em>Proceedings of the Survey Congress</em>, September 24, 101–29. | `M06.07` |
| Translator only | (Vale 2031) | `x` Vale, Tam, trans. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>Proceedings of the Survey Congress</em>, September 24, 101–29. | `M06.08` |
| No author | (<em>Proceedings of the Survey Congress</em> 2031) | `x` <em>Proceedings of the Survey Congress</em>. 2031. <span>“The Map of a River: A Field Guide.”</span> September 24, 101–29. | `M06.09` |
| No date | (Reed, n.d.) | `x` Reed, Ada J. n.d. <span>“The Map of a River: A Field Guide.”</span> <em>Proceedings of the Survey Congress</em>, 101–29. | `M06.10` |
| Date range | (Reed 2030–2031) | `x` Reed, Ada J. 2030–2031. <span>“The Map of a River: A Field Guide.”</span> <em>Proceedings of the Survey Congress</em>, December 29–January 2, 101–29. | `M06.11` |
| Season | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>Proceedings of the Survey Congress</em>, Summer, 101–29. | `M06.12` |
| Status without date | (Reed, forthcoming) | `x` Reed, Ada J. Forthcoming. <span>“The Map of a River: A Field Guide.”</span> <em>Proceedings of the Survey Congress</em>, 101–29. | `M06.13` |
| Status with date | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>Proceedings of the Survey Congress</em>, in press September 24, 101–29. | `M06.14` |
| Original date | (Reed [1913] 2031) | `x` Reed, Ada J. (1913) 2031. <span>“The Map of a River: A Field Guide.”</span> <em>Proceedings of the Survey Congress</em>, September 24, 101–29. | `M06.15` |
| Edition | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>Proceedings of the Survey Congress</em>, September 24. 3rd ed., 101–29. | `M06.16` |
| Number of volumes | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>Proceedings of the Survey Congress</em>, September 24, 101–29. | `M06.17` |
| Collection | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>Proceedings of the Survey Congress</em>, Survey series, September 24, 101–29. | `M06.18` |
| DOI | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>Proceedings of the Survey Congress</em>, September 24, 101–29. <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>. | `M06.19` |
| URL and accessed | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>Proceedings of the Survey Congress</em>, September 24, 101–29. <a href="https://example.org/maps">https://example.org/maps</a>. | `M06.20` |
| Short container title | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>Proceedings of the Survey Congress</em>, September 24, 101–29. | `M06.21` |

### `report`

Additional baseline fields <code>{&quot;publisher&quot;:&quot;Map House&quot;,&quot;publisher-place&quot;:&quot;North Port&quot;,&quot;number&quot;:&quot;R-12&quot;,&quot;institution&quot;:&quot;Survey Institute&quot;}</code>.

| Input change | Citation | Bibliography entry | Source |
| --- | --- | --- | --- |
| 1 author | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. R-12. Map House. | `M07.01` |
| 2 authors | (Reed and Lake 2031) | `x` Reed, Ada J., and Bo Lake. 2031. <em>The Map of a River: A Field Guide</em>. R-12. Map House. | `M07.02` |
| 3 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, and Cy Baker. 2031. <em>The Map of a River: A Field Guide</em>. R-12. Map House. | `M07.03` |
| 4 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, and Di Moor. 2031. <em>The Map of a River: A Field Guide</em>. R-12. Map House. | `M07.04` |
| 11 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, et al. 2031. <em>The Map of a River: A Field Guide</em>. R-12. Map House. | `M07.05` |
| Editor only | (Holt 2031) | `x` Holt, Eve, ed. 2031. <em>The Map of a River: A Field Guide</em>. R-12. Map House. | `M07.06` |
| Translator | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. R-12. Translated by Tam Vale. Map House. | `M07.07` |
| Translator only | (Vale 2031) | `x` Vale, Tam, trans. 2031. <em>The Map of a River: A Field Guide</em>. R-12. Map House. | `M07.08` |
| No author | (<em>The Map of a River: A Field Guide</em> 2031) | `x` <em>The Map of a River: A Field Guide</em>. 2031. R-12. Map House. | `M07.09` |
| No date | (Reed, n.d.) | `x` Reed, Ada J. n.d. <em>The Map of a River: A Field Guide</em>. R-12. Map House. | `M07.10` |
| Date range | (Reed 2030–2031) | `x` Reed, Ada J. 2030–2031. <em>The Map of a River: A Field Guide</em>. R-12. Map House. | `M07.11` |
| Season | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. R-12. Map House. | `M07.12` |
| Status without date | (Reed, forthcoming) | `x` Reed, Ada J. Forthcoming. <em>The Map of a River: A Field Guide</em>. R-12. Map House. | `M07.13` |
| Status with date | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. R-12. Map House, in press. | `M07.14` |
| Original date | (Reed [1913] 2031) | `x` Reed, Ada J. (1913) 2031. <em>The Map of a River: A Field Guide</em>. R-12. Map House. | `M07.15` |
| Edition | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. R-12. 3rd ed. Map House. | `M07.16` |
| Number of volumes | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. R-12. 4 vols. Map House. | `M07.17` |
| Collection | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. R-12. Survey Series 2. Map House. | `M07.18` |
| DOI | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. R-12. Map House. <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>. | `M07.19` |
| URL and accessed | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. R-12. Map House. <a href="https://example.org/maps">https://example.org/maps</a>. | `M07.20` |
| Short container title | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. R-12. Map House. | `M07.21` |

### `thesis`

Additional baseline fields <code>{&quot;publisher&quot;:&quot;Map House&quot;,&quot;publisher-place&quot;:&quot;North Port&quot;,&quot;genre&quot;:&quot;PhD thesis&quot;}</code>.

| Input change | Citation | Bibliography entry | Source |
| --- | --- | --- | --- |
| 1 author | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> PhD thesis, Map House. | `M08.01` |
| 2 authors | (Reed and Lake 2031) | `x` Reed, Ada J., and Bo Lake. 2031. <span>“The Map of a River: A Field Guide.”</span> PhD thesis, Map House. | `M08.02` |
| 3 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, and Cy Baker. 2031. <span>“The Map of a River: A Field Guide.”</span> PhD thesis, Map House. | `M08.03` |
| 4 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, and Di Moor. 2031. <span>“The Map of a River: A Field Guide.”</span> PhD thesis, Map House. | `M08.04` |
| 11 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, et al. 2031. <span>“The Map of a River: A Field Guide.”</span> PhD thesis, Map House. | `M08.05` |
| Editor only | (Holt 2031) | `x` Holt, Eve, ed. 2031. <span>“The Map of a River: A Field Guide.”</span> PhD thesis, Map House. | `M08.06` |
| Translator | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Translated by Tam Vale. PhD thesis, Map House. | `M08.07` |
| Translator only | (Vale 2031) | `x` Vale, Tam, trans. 2031. <span>“The Map of a River: A Field Guide.”</span> PhD thesis, Map House. | `M08.08` |
| No author | (<span>“The Map of a River: A Field Guide”</span> 2031) | `x` <span>“The Map of a River: A Field Guide.”</span> 2031. PhD thesis, Map House. | `M08.09` |
| No date | (Reed, n.d.) | `x` Reed, Ada J. n.d. <span>“The Map of a River: A Field Guide.”</span> PhD thesis, Map House. | `M08.10` |
| Date range | (Reed 2030–2031) | `x` Reed, Ada J. 2030–2031. <span>“The Map of a River: A Field Guide.”</span> PhD thesis, Map House. | `M08.11` |
| Season | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> PhD thesis, Map House. | `M08.12` |
| Status without date | (Reed, forthcoming) | `x` Reed, Ada J. Forthcoming. <span>“The Map of a River: A Field Guide.”</span> PhD thesis, Map House. | `M08.13` |
| Status with date | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> PhD thesis, Map House, in press. | `M08.14` |
| Original date | (Reed [1913] 2031) | `x` Reed, Ada J. (1913) 2031. <span>“The Map of a River: A Field Guide.”</span> PhD thesis, Map House. | `M08.15` |
| Edition | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> 3rd ed. PhD thesis, Map House. | `M08.16` |
| Number of volumes | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> 4 vols. PhD thesis, Map House. | `M08.17` |
| Collection | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Survey Series 2. PhD thesis, Map House. | `M08.18` |
| DOI | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> PhD thesis, Map House. <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>. | `M08.19` |
| URL and accessed | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> PhD thesis, Map House. <a href="https://example.org/maps">https://example.org/maps</a>. | `M08.20` |
| Short container title | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> PhD thesis, Map House. | `M08.21` |

### `webpage`

Additional baseline fields <code>{&quot;container-title&quot;:&quot;River Survey Online&quot;}</code>.

| Input change | Citation | Bibliography entry | Source |
| --- | --- | --- | --- |
| 1 author | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> River Survey Online, September 24. | `M09.01` |
| 2 authors | (Reed and Lake 2031) | `x` Reed, Ada J., and Bo Lake. 2031. <span>“The Map of a River: A Field Guide.”</span> River Survey Online, September 24. | `M09.02` |
| 3 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, and Cy Baker. 2031. <span>“The Map of a River: A Field Guide.”</span> River Survey Online, September 24. | `M09.03` |
| 4 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, and Di Moor. 2031. <span>“The Map of a River: A Field Guide.”</span> River Survey Online, September 24. | `M09.04` |
| 11 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, et al. 2031. <span>“The Map of a River: A Field Guide.”</span> River Survey Online, September 24. | `M09.05` |
| Editor only | (Holt 2031) | `x` Holt, Eve, ed. 2031. <span>“The Map of a River: A Field Guide.”</span> River Survey Online, September 24. | `M09.06` |
| Translator | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Translated by Tam Vale. River Survey Online, September 24. | `M09.07` |
| Translator only | (Vale 2031) | `x` Vale, Tam, trans. 2031. <span>“The Map of a River: A Field Guide.”</span> River Survey Online, September 24. | `M09.08` |
| No author | (River Survey Online 2031) | `x` River Survey Online. 2031. <span>“The Map of a River: A Field Guide.”</span> September 24. | `M09.09` |
| No date | (Reed, n.d.) | `x` Reed, Ada J. n.d. <span>“The Map of a River: A Field Guide.”</span> River Survey Online. | `M09.10` |
| Date range | (Reed 2030–2031) | `x` Reed, Ada J. 2030–2031. <span>“The Map of a River: A Field Guide.”</span> River Survey Online, December 29–January 2. | `M09.11` |
| Season | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> River Survey Online, Summer. | `M09.12` |
| Status without date | (Reed, forthcoming) | `x` Reed, Ada J. Forthcoming. <span>“The Map of a River: A Field Guide.”</span> River Survey Online. | `M09.13` |
| Status with date | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> River Survey Online, In press September 24. | `M09.14` |
| Original date | (Reed [1913] 2031) | `x` Reed, Ada J. (1913) 2031. <span>“The Map of a River: A Field Guide.”</span> River Survey Online, September 24. | `M09.15` |
| Edition | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> 3rd ed. River Survey Online, September 24. | `M09.16` |
| Number of volumes | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> 4 vols. River Survey Online, September 24. | `M09.17` |
| Collection | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> River Survey Online, September 24. | `M09.18` |
| DOI | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> River Survey Online, September 24. <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>. | `M09.19` |
| URL and accessed | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> River Survey Online, September 24. <a href="https://example.org/maps">https://example.org/maps</a>. | `M09.20` |
| Short container title | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> River Survey Online, September 24. | `M09.21` |

### `post-weblog`

Additional baseline fields <code>{&quot;container-title&quot;:&quot;River Survey Online&quot;,&quot;genre&quot;:&quot;Field blog&quot;}</code>.

| Input change | Citation | Bibliography entry | Source |
| --- | --- | --- | --- |
| 1 author | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Field blog. <em>River Survey Online</em>, September 24. | `M10.01` |
| 2 authors | (Reed and Lake 2031) | `x` Reed, Ada J., and Bo Lake. 2031. <span>“The Map of a River: A Field Guide.”</span> Field blog. <em>River Survey Online</em>, September 24. | `M10.02` |
| 3 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, and Cy Baker. 2031. <span>“The Map of a River: A Field Guide.”</span> Field blog. <em>River Survey Online</em>, September 24. | `M10.03` |
| 4 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, and Di Moor. 2031. <span>“The Map of a River: A Field Guide.”</span> Field blog. <em>River Survey Online</em>, September 24. | `M10.04` |
| 11 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, et al. 2031. <span>“The Map of a River: A Field Guide.”</span> Field blog. <em>River Survey Online</em>, September 24. | `M10.05` |
| Editor only | (Holt 2031) | `x` Holt, Eve, ed. 2031. <span>“The Map of a River: A Field Guide.”</span> Field blog. <em>River Survey Online</em>, September 24. | `M10.06` |
| Translator | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Field blog. Translated by Tam Vale. <em>River Survey Online</em>, September 24. | `M10.07` |
| Translator only | (Vale 2031) | `x` Vale, Tam, trans. 2031. <span>“The Map of a River: A Field Guide.”</span> Field blog. <em>River Survey Online</em>, September 24. | `M10.08` |
| No author | (<span>“The Map of a River: A Field Guide”</span> 2031) | `x` <span>“The Map of a River: A Field Guide.”</span> 2031. Field blog. <em>River Survey Online</em>, September 24. | `M10.09` |
| No date | (Reed, n.d.) | `x` Reed, Ada J. n.d. <span>“The Map of a River: A Field Guide.”</span> Field blog. <em>River Survey Online</em>. | `M10.10` |
| Date range | (Reed 2030–2031) | `x` Reed, Ada J. 2030–2031. <span>“The Map of a River: A Field Guide.”</span> Field blog. <em>River Survey Online</em>, December 29–January 2. | `M10.11` |
| Season | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Field blog. <em>River Survey Online</em>, Summer. | `M10.12` |
| Status without date | (Reed, forthcoming) | `x` Reed, Ada J. Forthcoming. <span>“The Map of a River: A Field Guide.”</span> Field blog. <em>River Survey Online</em>. | `M10.13` |
| Status with date | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Field blog. <em>River Survey Online</em>, In press September 24. | `M10.14` |
| Original date | (Reed [1913] 2031) | `x` Reed, Ada J. (1913) 2031. <span>“The Map of a River: A Field Guide.”</span> Field blog. <em>River Survey Online</em>, September 24. | `M10.15` |
| Edition | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Field blog. <em>River Survey Online</em>, September 24. 3rd ed. | `M10.16` |
| Number of volumes | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Field blog. <em>River Survey Online</em>, September 24. | `M10.17` |
| Collection | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Field blog. <em>River Survey Online</em>, Survey series, September 24. | `M10.18` |
| DOI | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Field blog. <em>River Survey Online</em>, September 24. <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>. | `M10.19` |
| URL and accessed | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Field blog. <em>River Survey Online</em>, September 24. <a href="https://example.org/maps">https://example.org/maps</a>. | `M10.20` |
| Short container title | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Field blog. <em>River Survey Online</em>, September 24. | `M10.21` |

### `motion_picture`

Additional baseline fields <code>{&quot;publisher&quot;:&quot;Map House&quot;,&quot;publisher-place&quot;:&quot;North Port&quot;,&quot;director&quot;:[{&quot;given&quot;:&quot;Dee&quot;,&quot;family&quot;:&quot;Wood&quot;}],&quot;medium&quot;:&quot;DVD&quot;,&quot;genre&quot;:&quot;documentary&quot;}</code>.

| Input change | Citation | Bibliography entry | Source |
| --- | --- | --- | --- |
| 1 author | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. Documentary. Directed by Dee Wood. Map House. DVD. | `M11.01` |
| 2 authors | (Reed and Lake 2031) | `x` Reed, Ada J., and Bo Lake. 2031. <em>The Map of a River: A Field Guide</em>. Documentary. Directed by Dee Wood. Map House. DVD. | `M11.02` |
| 3 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, and Cy Baker. 2031. <em>The Map of a River: A Field Guide</em>. Documentary. Directed by Dee Wood. Map House. DVD. | `M11.03` |
| 4 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, and Di Moor. 2031. <em>The Map of a River: A Field Guide</em>. Documentary. Directed by Dee Wood. Map House. DVD. | `M11.04` |
| 11 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, et al. 2031. <em>The Map of a River: A Field Guide</em>. Documentary. Directed by Dee Wood. Map House. DVD. | `M11.05` |
| Editor only | (Holt 2031) | `x` Holt, Eve, ed. 2031. <em>The Map of a River: A Field Guide</em>. Documentary. Directed by Dee Wood. Map House. DVD. | `M11.06` |
| Translator | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. Documentary. Translated by Tam Vale. Directed by Dee Wood. Map House. DVD. | `M11.07` |
| Translator only | (Vale 2031) | `x` Vale, Tam, trans. 2031. <em>The Map of a River: A Field Guide</em>. Documentary. Directed by Dee Wood. Map House. DVD. | `M11.08` |
| No author | (<em>The Map of a River: A Field Guide</em> 2031) | `x` <em>The Map of a River: A Field Guide</em>. 2031. Documentary. Map House. DVD. | `M11.09` |
| No date | (Reed, n.d.) | `x` Reed, Ada J. n.d. <em>The Map of a River: A Field Guide</em>. Documentary. Directed by Dee Wood. Map House. DVD. | `M11.10` |
| Date range | (Reed 2030–2031) | `x` Reed, Ada J. 2030–2031. <em>The Map of a River: A Field Guide</em>. Documentary. Directed by Dee Wood. Map House. DVD. | `M11.11` |
| Season | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. Documentary. Directed by Dee Wood. Map House. DVD. | `M11.12` |
| Status without date | (Reed, forthcoming) | `x` Reed, Ada J. Forthcoming. <em>The Map of a River: A Field Guide</em>. Documentary. Directed by Dee Wood. Map House. DVD. | `M11.13` |
| Status with date | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. Documentary. Directed by Dee Wood. Map House, in press. DVD. | `M11.14` |
| Original date | (Reed [1913] 2031) | `x` Reed, Ada J. (1913) 2031. <em>The Map of a River: A Field Guide</em>. Documentary. Directed by Dee Wood. Map House. DVD. | `M11.15` |
| Edition | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. Documentary. 3rd ed. Directed by Dee Wood. Map House. DVD. | `M11.16` |
| Number of volumes | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. Documentary. Directed by Dee Wood. 4 vols. Map House. DVD. | `M11.17` |
| Collection | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. Documentary. Directed by Dee Wood. Survey Series 2. Map House. DVD. | `M11.18` |
| DOI | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. Documentary. Directed by Dee Wood. Map House. DVD. <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>. | `M11.19` |
| URL and accessed | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. Documentary. Directed by Dee Wood. Map House. DVD. <a href="https://example.org/maps">https://example.org/maps</a>. | `M11.20` |
| Short container title | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. Documentary. Directed by Dee Wood. Map House. DVD. | `M11.21` |

### `dataset`

Additional baseline fields <code>{&quot;publisher&quot;:&quot;Map House&quot;,&quot;publisher-place&quot;:&quot;North Port&quot;,&quot;version&quot;:&quot;2.1&quot;,&quot;number&quot;:&quot;D-12&quot;}</code>.

| Input change | Citation | Bibliography entry | Source |
| --- | --- | --- | --- |
| 1 author | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. Map House, September 24. | `M12.01` |
| 2 authors | (Reed and Lake 2031) | `x` Reed, Ada J., and Bo Lake. 2031. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. Map House, September 24. | `M12.02` |
| 3 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, and Cy Baker. 2031. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. Map House, September 24. | `M12.03` |
| 4 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, and Di Moor. 2031. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. Map House, September 24. | `M12.04` |
| 11 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, et al. 2031. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. Map House, September 24. | `M12.05` |
| Editor only | (Holt 2031) | `x` Holt, Eve, ed. 2031. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. Map House, September 24. | `M12.06` |
| Translator | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. Translated by Tam Vale. Map House, September 24. | `M12.07` |
| Translator only | (Vale 2031) | `x` Vale, Tam, trans. 2031. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. Map House, September 24. | `M12.08` |
| No author | (<span>“The Map of a River: A Field Guide”</span> 2031) | `x` <span>“The Map of a River: A Field Guide.”</span> 2031. D-12. Version 2.1. Map House, September 24. | `M12.09` |
| No date | (Reed, n.d.) | `x` Reed, Ada J. n.d. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. Map House. | `M12.10` |
| Date range | (Reed 2030–2031) | `x` Reed, Ada J. 2030–2031. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. Map House, December 29–January 2. | `M12.11` |
| Season | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. Map House, Summer. | `M12.12` |
| Status without date | (Reed, forthcoming) | `x` Reed, Ada J. Forthcoming. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. Map House. | `M12.13` |
| Status with date | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. Map House, in press September 24. | `M12.14` |
| Original date | (Reed [1913] 2031) | `x` Reed, Ada J. (1913) 2031. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. Map House, September 24. | `M12.15` |
| Edition | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. 3rd ed. Map House, September 24. | `M12.16` |
| Number of volumes | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. 4 vols. Map House, September 24. | `M12.17` |
| Collection | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. Survey Series 2. Map House, September 24. | `M12.18` |
| DOI | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. Map House, September 24. <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>. | `M12.19` |
| URL and accessed | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. Map House, September 24. <a href="https://example.org/maps">https://example.org/maps</a>. | `M12.20` |
| Short container title | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> D-12. Version 2.1. Map House, September 24. | `M12.21` |

### `software`

Additional baseline fields <code>{&quot;publisher&quot;:&quot;Map House&quot;,&quot;publisher-place&quot;:&quot;North Port&quot;,&quot;version&quot;:&quot;2.1&quot;,&quot;medium&quot;:&quot;software&quot;}</code>.

| Input change | Citation | Bibliography entry | Source |
| --- | --- | --- | --- |
| 1 author | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. V. 2.1. Map House, released September 24. Software. | `M13.01` |
| 2 authors | (Reed and Lake 2031) | `x` Reed, Ada J., and Bo Lake. 2031. <em>The Map of a River: A Field Guide</em>. V. 2.1. Map House, released September 24. Software. | `M13.02` |
| 3 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, and Cy Baker. 2031. <em>The Map of a River: A Field Guide</em>. V. 2.1. Map House, released September 24. Software. | `M13.03` |
| 4 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, and Di Moor. 2031. <em>The Map of a River: A Field Guide</em>. V. 2.1. Map House, released September 24. Software. | `M13.04` |
| 11 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, et al. 2031. <em>The Map of a River: A Field Guide</em>. V. 2.1. Map House, released September 24. Software. | `M13.05` |
| Editor only | (Holt 2031) | `x` Holt, Eve, ed. 2031. <em>The Map of a River: A Field Guide</em>. V. 2.1. Map House, released September 24. Software. | `M13.06` |
| Translator | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. V. 2.1. Translated by Tam Vale. Map House, released September 24. Software. | `M13.07` |
| Translator only | (Vale 2031) | `x` Vale, Tam, trans. 2031. <em>The Map of a River: A Field Guide</em>. V. 2.1. Map House, released September 24. Software. | `M13.08` |
| No author | (Map House 2031) | `x` Map House. 2031. <em>The Map of a River: A Field Guide</em>. V. 2.1. Released September 24. Software. | `M13.09` |
| No date | (Reed, n.d.) | `x` Reed, Ada J. n.d. <em>The Map of a River: A Field Guide</em>. V. 2.1. Map House. Software. | `M13.10` |
| Date range | (Reed 2030–2031) | `x` Reed, Ada J. 2030–2031. <em>The Map of a River: A Field Guide</em>. V. 2.1. Map House, released December 29–January 2. Software. | `M13.11` |
| Season | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. V. 2.1. Map House, released Summer. Software. | `M13.12` |
| Status without date | (Reed, forthcoming) | `x` Reed, Ada J. Forthcoming. <em>The Map of a River: A Field Guide</em>. V. 2.1. Map House. Software. | `M13.13` |
| Status with date | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. V. 2.1. Map House, in press September 24. Software. | `M13.14` |
| Original date | (Reed [1913] 2031) | `x` Reed, Ada J. (1913) 2031. <em>The Map of a River: A Field Guide</em>. V. 2.1. Map House, released September 24. Software. | `M13.15` |
| Edition | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. V. 2.1. 3rd ed. Map House, released September 24. Software. | `M13.16` |
| Number of volumes | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. V. 2.1. 4 vols. Map House, released September 24. Software. | `M13.17` |
| Collection | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. V. 2.1. Survey Series 2. Map House, released September 24. Software. | `M13.18` |
| DOI | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. V. 2.1. Map House, released September 24. Software. <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>. | `M13.19` |
| URL and accessed | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. V. 2.1. Map House, released September 24. Software. <a href="https://example.org/maps">https://example.org/maps</a>. | `M13.20` |
| Short container title | (Reed 2031) | `x` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. V. 2.1. Map House, released September 24. Software. | `M13.21` |

### `manuscript`

Additional baseline fields <code>{&quot;publisher&quot;:&quot;Map House&quot;,&quot;publisher-place&quot;:&quot;North Port&quot;,&quot;archive&quot;:&quot;Map Archive&quot;,&quot;archive_location&quot;:&quot;Box 3&quot;}</code>.

| Input change | Citation | Bibliography entry | Source |
| --- | --- | --- | --- |
| 1 author | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Map House, September 24. Box 3. Map Archive. | `M14.01` |
| 2 authors | (Reed and Lake 2031) | `x` Reed, Ada J., and Bo Lake. 2031. <span>“The Map of a River: A Field Guide.”</span> Map House, September 24. Box 3. Map Archive. | `M14.02` |
| 3 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, and Cy Baker. 2031. <span>“The Map of a River: A Field Guide.”</span> Map House, September 24. Box 3. Map Archive. | `M14.03` |
| 4 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, and Di Moor. 2031. <span>“The Map of a River: A Field Guide.”</span> Map House, September 24. Box 3. Map Archive. | `M14.04` |
| 11 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, et al. 2031. <span>“The Map of a River: A Field Guide.”</span> Map House, September 24. Box 3. Map Archive. | `M14.05` |
| Editor only | (Holt 2031) | `x` Holt, Eve, ed. 2031. <span>“The Map of a River: A Field Guide.”</span> Map House, September 24. Box 3. Map Archive. | `M14.06` |
| Translator | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Translated by Tam Vale. Map House, September 24. Box 3. Map Archive. | `M14.07` |
| Translator only | (Vale 2031) | `x` Vale, Tam, trans. 2031. <span>“The Map of a River: A Field Guide.”</span> Map House, September 24. Box 3. Map Archive. | `M14.08` |
| No author | (<span>“The Map of a River: A Field Guide”</span> 2031) | `x` <span>“The Map of a River: A Field Guide.”</span> 2031. Map House, September 24. Box 3. Map Archive. | `M14.09` |
| No date | (Reed, n.d.) | `x` Reed, Ada J. n.d. <span>“The Map of a River: A Field Guide.”</span> Map House. Box 3. Map Archive. | `M14.10` |
| Date range | (Reed 2030–2031) | `x` Reed, Ada J. 2030–2031. <span>“The Map of a River: A Field Guide.”</span> Map House, December 29–January 2. Box 3. Map Archive. | `M14.11` |
| Season | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Map House, Summer. Box 3. Map Archive. | `M14.12` |
| Status without date | (Reed, forthcoming) | `x` Reed, Ada J. Forthcoming. <span>“The Map of a River: A Field Guide.”</span> Map House. Box 3. Map Archive. | `M14.13` |
| Status with date | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Map House, in press September 24. Box 3. Map Archive. | `M14.14` |
| Original date | (Reed [1913] 2031) | `x` Reed, Ada J. (1913) 2031. <span>“The Map of a River: A Field Guide.”</span> Map House, September 24. Box 3. Map Archive. | `M14.15` |
| Edition | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> 3rd ed. Map House, September 24. Box 3. Map Archive. | `M14.16` |
| Number of volumes | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> 4 vols. Map House, September 24. Box 3. Map Archive. | `M14.17` |
| Collection | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Survey Series 2. Map House, September 24. Box 3. Map Archive. | `M14.18` |
| DOI | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Map House, September 24. Box 3. Map Archive. <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>. | `M14.19` |
| URL and accessed | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Map House, September 24. Box 3. Map Archive. <a href="https://example.org/maps">https://example.org/maps</a>. | `M14.20` |
| Short container title | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Map House, September 24. Box 3. Map Archive. | `M14.21` |

### `legal_case`

Additional baseline fields <code>{&quot;authority&quot;:&quot;River Court&quot;,&quot;container-title&quot;:&quot;River Reports&quot;,&quot;volume&quot;:12,&quot;page&quot;:&quot;101&quot;,&quot;number&quot;:&quot;31-5&quot;}</code>.

| Input change | Citation | Bibliography entry | Source |
| --- | --- | --- | --- |
| 1 author | (<em>the map of a river: a field guide</em> 2031) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court 2031). | `M15.01` |
| 2 authors | (<em>the map of a river: a field guide</em> 2031) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court 2031). | `M15.02` |
| 3 authors | (<em>the map of a river: a field guide</em> 2031) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court 2031). | `M15.03` |
| 4 authors | (<em>the map of a river: a field guide</em> 2031) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court 2031). | `M15.04` |
| 11 authors | (<em>the map of a river: a field guide</em> 2031) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court 2031). | `M15.05` |
| Editor only | (<em>the map of a river: a field guide</em> 2031) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court 2031). | `M15.06` |
| Translator | (<em>the map of a river: a field guide</em> 2031) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court 2031). | `M15.07` |
| Translator only | (<em>the map of a river: a field guide</em> 2031) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court 2031). | `M15.08` |
| No author | (<em>the map of a river: a field guide</em> 2031) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court 2031). | `M15.09` |
| No date | (<em>the map of a river: a field guide</em>, n.d.) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court). | `M15.10` |
| Date range | (<em>the map of a river: a field guide</em> 2030–2031) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court 2030–2031). | `M15.11` |
| Season | (<em>the map of a river: a field guide</em> 2031) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court 2031). | `M15.12` |
| Status without date | (<em>the map of a river: a field guide</em>, forthcoming) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court). | `M15.13` |
| Status with date | (<em>the map of a river: a field guide</em> 2031) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court 2031). | `M15.14` |
| Original date | (<em>the map of a river: a field guide</em> [1913] 2031) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court 2031). | `M15.15` |
| Edition | (<em>the map of a river: a field guide</em> 2031) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court 2031). | `M15.16` |
| Number of volumes | (<em>the map of a river: a field guide</em> 2031) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court 2031). | `M15.17` |
| Collection | (<em>the map of a river: a field guide</em> 2031) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court 2031). | `M15.18` |
| DOI | (<em>the map of a river: a field guide</em> 2031) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court 2031). <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>. | `M15.19` |
| URL and accessed | (<em>the map of a river: a field guide</em> 2031) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court 2031). <a href="https://example.org/maps">https://example.org/maps</a>. | `M15.20` |
| Short container title | (<em>the map of a river: a field guide</em> 2031) | `x` The Map of a River: A Field Guide, 12 River Reports 101 (River Court 2031). | `M15.21` |

### `patent`

Additional baseline fields <code>{&quot;publisher&quot;:&quot;Map House&quot;,&quot;publisher-place&quot;:&quot;North Port&quot;,&quot;number&quot;:&quot;2031-12&quot;,&quot;authority&quot;:&quot;Patent Office&quot;}</code>.

| Input change | Citation | Bibliography entry | Source |
| --- | --- | --- | --- |
| 1 author | (Reed 2031) | `x` Reed, Ada J. 2031. The map of a river: a field guide. Patent Office Patent 2031-12, issued September 24, 2031. | `M16.01` |
| 2 authors | (Reed and Lake 2031) | `x` Reed, Ada J., and Bo Lake. 2031. The map of a river: a field guide. Patent Office Patent 2031-12, issued September 24, 2031. | `M16.02` |
| 3 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, and Cy Baker. 2031. The map of a river: a field guide. Patent Office Patent 2031-12, issued September 24, 2031. | `M16.03` |
| 4 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, and Di Moor. 2031. The map of a river: a field guide. Patent Office Patent 2031-12, issued September 24, 2031. | `M16.04` |
| 11 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, et al. 2031. The map of a river: a field guide. Patent Office Patent 2031-12, issued September 24, 2031. | `M16.05` |
| Editor only | (Holt 2031) | `x` Holt, Eve, ed. 2031. The map of a river: a field guide. Patent Office Patent 2031-12, issued September 24, 2031. | `M16.06` |
| Translator | (Reed 2031) | `x` Reed, Ada J. 2031. The map of a river: a field guide. Patent Office Patent 2031-12, issued September 24, 2031. | `M16.07` |
| Translator only | (Vale 2031) | `x` Vale, Tam, trans. 2031. The map of a river: a field guide. Patent Office Patent 2031-12, issued September 24, 2031. | `M16.08` |
| No author | (The map of a river: a field guide 2031) | `x` The map of a river: a field guide. 2031. Patent Office Patent 2031-12, issued September 24, 2031. | `M16.09` |
| No date | (Reed, n.d.) | `x` Reed, Ada J. n.d. The map of a river: a field guide. Patent Office Patent 2031-12. | `M16.10` |
| Date range | (Reed 2030–2031) | `x` Reed, Ada J. 2030–2031. The map of a river: a field guide. Patent Office Patent 2031-12, issued December 29, 2030–January 2, 2031. | `M16.11` |
| Season | (Reed 2031) | `x` Reed, Ada J. 2031. The map of a river: a field guide. Patent Office Patent 2031-12, issued Summer 2031. | `M16.12` |
| Status without date | (Reed, forthcoming) | `x` Reed, Ada J. Forthcoming. The map of a river: a field guide. Patent Office Patent 2031-12. | `M16.13` |
| Status with date | (Reed 2031) | `x` Reed, Ada J. 2031. The map of a river: a field guide. Patent Office Patent 2031-12, issued September 24, 2031. | `M16.14` |
| Original date | (Reed [1913] 2031) | `x` Reed, Ada J. (1913) 2031. The map of a river: a field guide. Patent Office Patent 2031-12, issued September 24, 2031. | `M16.15` |
| Edition | (Reed 2031) | `x` Reed, Ada J. 2031. The map of a river: a field guide. Patent Office Patent 2031-12, issued September 24, 2031. | `M16.16` |
| Number of volumes | (Reed 2031) | `x` Reed, Ada J. 2031. The map of a river: a field guide. Patent Office Patent 2031-12, issued September 24, 2031. | `M16.17` |
| Collection | (Reed 2031) | `x` Reed, Ada J. 2031. The map of a river: a field guide. Patent Office Patent 2031-12, issued September 24, 2031. | `M16.18` |
| DOI | (Reed 2031) | `x` Reed, Ada J. 2031. The map of a river: a field guide. Patent Office Patent 2031-12, issued September 24, 2031. <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>. | `M16.19` |
| URL and accessed | (Reed 2031) | `x` Reed, Ada J. 2031. The map of a river: a field guide. Patent Office Patent 2031-12, issued September 24, 2031. <a href="https://example.org/maps">https://example.org/maps</a>. | `M16.20` |
| Short container title | (Reed 2031) | `x` Reed, Ada J. 2031. The map of a river: a field guide. Patent Office Patent 2031-12, issued September 24, 2031. | `M16.21` |

### `entry-encyclopedia`

Additional baseline fields <code>{&quot;publisher&quot;:&quot;Map House&quot;,&quot;publisher-place&quot;:&quot;North Port&quot;,&quot;container-title&quot;:&quot;encyclopedia of rivers&quot;,&quot;volume&quot;:12,&quot;page&quot;:&quot;101-129&quot;}</code>.

| Input change | Citation | Bibliography entry | Source |
| --- | --- | --- | --- |
| 1 author | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>Encyclopedia of Rivers</em>, vol. 12. Map House. | `M17.01` |
| 2 authors | (Reed and Lake 2031) | `x` Reed, Ada J., and Bo Lake. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>Encyclopedia of Rivers</em>, vol. 12. Map House. | `M17.02` |
| 3 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, and Cy Baker. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>Encyclopedia of Rivers</em>, vol. 12. Map House. | `M17.03` |
| 4 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, and Di Moor. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>Encyclopedia of Rivers</em>, vol. 12. Map House. | `M17.04` |
| 11 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, et al. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>Encyclopedia of Rivers</em>, vol. 12. Map House. | `M17.05` |
| Editor only | (<em>Encyclopedia of Rivers</em> 2031, under <span>“the map of a river: a field guide”</span>) | `x`  | `M17.06` |
| Translator | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>Encyclopedia of Rivers</em>, translated by Tam Vale, vol. 12. Map House. | `M17.07` |
| Translator only | (<em>Encyclopedia of Rivers</em> 2031, under <span>“the map of a river: a field guide”</span>) | `x`  | `M17.08` |
| No author | (<em>Encyclopedia of Rivers</em> 2031, under <span>“the map of a river: a field guide”</span>) | `x`  | `M17.09` |
| No date | (Reed) | `x` Reed, Ada J. n.d. <span>“The Map of a River: A Field Guide.”</span> In <em>Encyclopedia of Rivers</em>, vol. 12. Map House. | `M17.10` |
| Date range | (Reed 2030–2031) | `x` Reed, Ada J. 2030–2031. <span>“The Map of a River: A Field Guide.”</span> In <em>Encyclopedia of Rivers</em>, vol. 12. Map House. | `M17.11` |
| Season | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>Encyclopedia of Rivers</em>, vol. 12. Map House. | `M17.12` |
| Status without date | (Reed, forthcoming) | `x` Reed, Ada J. Forthcoming. <span>“The Map of a River: A Field Guide.”</span> In <em>Encyclopedia of Rivers</em>, vol. 12. Map House. | `M17.13` |
| Status with date | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>Encyclopedia of Rivers</em>, vol. 12. Map House, in press. | `M17.14` |
| Original date | (Reed [1913] 2031) | `x` Reed, Ada J. (1913) 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>Encyclopedia of Rivers</em>, vol. 12. Map House. | `M17.15` |
| Edition | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>Encyclopedia of Rivers</em>, 3rd ed., vol. 12. Map House. | `M17.16` |
| Number of volumes | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>Encyclopedia of Rivers</em>, vol. 12. Map House. | `M17.17` |
| Collection | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>Encyclopedia of Rivers</em>, vol. 12. Survey Series 2. Map House. | `M17.18` |
| DOI | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>Encyclopedia of Rivers</em>, vol. 12. Map House, September 24. <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>. | `M17.19` |
| URL and accessed | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>Encyclopedia of Rivers</em>, vol. 12. Map House, September 24. <a href="https://example.org/maps">https://example.org/maps</a>. | `M17.20` |
| Short container title | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>Encyclopedia of Rivers</em>, vol. 12. Map House. | `M17.21` |

### `article`

Additional baseline fields <code>{&quot;publisher&quot;:&quot;Map House&quot;,&quot;publisher-place&quot;:&quot;North Port&quot;,&quot;container-title&quot;:&quot;the survey quarterly&quot;,&quot;volume&quot;:12,&quot;issue&quot;:3,&quot;page&quot;:&quot;101-129&quot;}</code>.

| Input change | Citation | Bibliography entry | Source |
| --- | --- | --- | --- |
| 1 author | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, vol. 12. no. 3. Preprint, Map House, September 24. | `M18.01` |
| 2 authors | (Reed and Lake 2031) | `x` Reed, Ada J., and Bo Lake. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, vol. 12. no. 3. Preprint, Map House, September 24. | `M18.02` |
| 3 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, and Cy Baker. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, vol. 12. no. 3. Preprint, Map House, September 24. | `M18.03` |
| 4 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, and Di Moor. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, vol. 12. no. 3. Preprint, Map House, September 24. | `M18.04` |
| 11 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, et al. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, vol. 12. no. 3. Preprint, Map House, September 24. | `M18.05` |
| Editor only | (Holt 2031) | `x` Holt, Eve, ed. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, vol. 12. no. 3. Preprint, Map House, September 24. | `M18.06` |
| Translator | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, translated by Tam Vale, vol. 12. no. 3. Preprint, Map House, September 24. | `M18.07` |
| Translator only | (Vale 2031) | `x` Vale, Tam, trans. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, vol. 12. no. 3. Preprint, Map House, September 24. | `M18.08` |
| No author | (<span>“The Map of a River: A Field Guide”</span> 2031) | `x` <span>“The Map of a River: A Field Guide.”</span> 2031. In <em>The Survey Quarterly</em>, vol. 12. no. 3. Preprint, Map House, September 24. | `M18.09` |
| No date | (Reed, n.d.) | `x` Reed, Ada J. n.d. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, vol. 12. no. 3. Preprint, Map House. | `M18.10` |
| Date range | (Reed 2030–2031) | `x` Reed, Ada J. 2030–2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, vol. 12. no. 3. Preprint, Map House, December 29–January 2. | `M18.11` |
| Season | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, vol. 12. no. 3. Preprint, Map House, Summer. | `M18.12` |
| Status without date | (Reed, forthcoming) | `x` Reed, Ada J. Forthcoming. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, vol. 12. no. 3. Preprint, Map House. | `M18.13` |
| Status with date | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, vol. 12. no. 3. Preprint, Map House, in press September 24. | `M18.14` |
| Original date | (Reed [1913] 2031) | `x` Reed, Ada J. (1913) 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, vol. 12. no. 3. Preprint, Map House, September 24. | `M18.15` |
| Edition | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, 3rd ed., vol. 12. no. 3. Preprint, Map House, September 24. | `M18.16` |
| Number of volumes | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, vol. 12. no. 3. Preprint, Map House, September 24. | `M18.17` |
| Collection | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, vol. 12. Survey Series, vol. 2, no. 3. Preprint, Map House, September 24. | `M18.18` |
| DOI | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, vol. 12. no. 3. Preprint, Map House, September 24. <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>. | `M18.19` |
| URL and accessed | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, vol. 12. no. 3. Preprint, Map House, September 24. <a href="https://example.org/maps">https://example.org/maps</a>. | `M18.20` |
| Short container title | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Survey Quarterly</em>, vol. 12. no. 3. Preprint, Map House, September 24. | `M18.21` |

### `article-with-genre`

Additional baseline fields <code>{&quot;publisher&quot;:&quot;Map House&quot;,&quot;publisher-place&quot;:&quot;North Port&quot;,&quot;container-title&quot;:&quot;the survey quarterly&quot;,&quot;volume&quot;:12,&quot;issue&quot;:3,&quot;page&quot;:&quot;101-129&quot;,&quot;genre&quot;:&quot;Preprint&quot;}</code>.

| Input change | Citation | Bibliography entry | Source |
| --- | --- | --- | --- |
| 1 author | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, vol. 12. no. 3. Map House, September 24. | `M19.01` |
| 2 authors | (Reed and Lake 2031) | `x` Reed, Ada J., and Bo Lake. 2031. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, vol. 12. no. 3. Map House, September 24. | `M19.02` |
| 3 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, and Cy Baker. 2031. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, vol. 12. no. 3. Map House, September 24. | `M19.03` |
| 4 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, and Di Moor. 2031. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, vol. 12. no. 3. Map House, September 24. | `M19.04` |
| 11 authors | (Reed et al. 2031) | `x` Reed, Ada J., Bo Lake, Cy Baker, et al. 2031. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, vol. 12. no. 3. Map House, September 24. | `M19.05` |
| Editor only | (Holt 2031) | `x` Holt, Eve, ed. 2031. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, vol. 12. no. 3. Map House, September 24. | `M19.06` |
| Translator | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, translated by Tam Vale, vol. 12. no. 3. Map House, September 24. | `M19.07` |
| Translator only | (Vale 2031) | `x` Vale, Tam, trans. 2031. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, vol. 12. no. 3. Map House, September 24. | `M19.08` |
| No author | (<span>“The Map of a River: A Field Guide”</span> 2031) | `x` <span>“The Map of a River: A Field Guide.”</span> 2031. Preprint. In <em>The Survey Quarterly</em>, vol. 12. no. 3. Map House, September 24. | `M19.09` |
| No date | (Reed, n.d.) | `x` Reed, Ada J. n.d. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, vol. 12. no. 3. Map House. | `M19.10` |
| Date range | (Reed 2030–2031) | `x` Reed, Ada J. 2030–2031. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, vol. 12. no. 3. Map House, December 29–January 2. | `M19.11` |
| Season | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, vol. 12. no. 3. Map House, Summer. | `M19.12` |
| Status without date | (Reed, forthcoming) | `x` Reed, Ada J. Forthcoming. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, vol. 12. no. 3. Map House. | `M19.13` |
| Status with date | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, vol. 12. no. 3. Map House, in press September 24. | `M19.14` |
| Original date | (Reed [1913] 2031) | `x` Reed, Ada J. (1913) 2031. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, vol. 12. no. 3. Map House, September 24. | `M19.15` |
| Edition | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, 3rd ed., vol. 12. no. 3. Map House, September 24. | `M19.16` |
| Number of volumes | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, vol. 12. no. 3. Map House, September 24. | `M19.17` |
| Collection | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, vol. 12. Survey Series, vol. 2, no. 3. Map House, September 24. | `M19.18` |
| DOI | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, vol. 12. no. 3. Map House, September 24. <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>. | `M19.19` |
| URL and accessed | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, vol. 12. no. 3. Map House, September 24. <a href="https://example.org/maps">https://example.org/maps</a>. | `M19.20` |
| Short container title | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> Preprint. In <em>The Survey Quarterly</em>, vol. 12. no. 3. Map House, September 24. | `M19.21` |

## Citation clusters and default-style scenarios

The first 46 rows use new reference data for the 46 upstream command scenarios
listed in the Wave 10 blueprint. The scenario ID identifies the upstream case.
The probes use Markdown input and HTML output to make the style result visible
in a common form. For cases whose original input or output uses another format,
the row covers the citation or bibliography feature at the citation filter
boundary. Additional rows test interactions used by the default style.

The log also records selected rendered locale terms under `L-<tag>`. The
`P-<tag>` rows test the four observed page term forms as candidate labels in
`--biblatex` output. The `P-9275-<tag>` rows test the same candidate matrix
across language tags, including missing and invalid tags. A cut means the
result has `33` as the full citation postnote. The term probes use a new
minimal style that renders only the terms listed in the input.

| Scenario | Synthetic input | Citation output in document order | Bibliography in sort order | Source |
| --- | --- | --- | --- | --- |
| `6783.md#1` | Published proceedings with no volume<br><code>Probe[@x].</code> | (Reed 2031) | `x` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>Proceedings of the Survey Congress</em>, September 24, 101–29. | `S01` |
| `6873.md#1` | Missing reference and quoted suffix with brackets<br><code>[@missing, „A […] B“]</code> | (<strong>missing?</strong>, „A […] B“) | — | `S02` |
| `6890.md#1` | Textual citations in body and note. Three authors, edition, title language and short title<br><code>@a<br><br>Body.^[@a]</code> | Reed et al. (2031)<br>Reed et al. (2031) | `a` Reed, Ada J., Bo Lake, and Cy Baker. 2031. <em>karten und flüsse: ein atlas</em>. 3rd ed. Map House. | `S03` |
| `6951.md#1` | No author and no date. Mixed with an authored work<br><code>[@b; @a]</code> | (<em>Banks and Streams</em>, n.d.; Reed 2031) | `b` <em>Banks and Streams</em>. n.d. In <em>River Studies</em>, vol. 3.<br>`a` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. | `S04` |
| `7042.md#1` | Cite all. Partial dates, name particles, literal East Asian names, DOI and journal fields | — | `b` Lake, Bo. 2031. <span>“Banks and Streams.”</span> <em>River Studies</em> 3 (September): 125–39.<br>`p` Meer, Jan van, and Lea de Noor. 2031. <em>The Map of a River: A Field Guide</em>. September 24.<br>`d` <span class="nocase">Reed, Ada J. et al.</span> 2031. <em>The Map of a River: A Field Guide</em>. September 24. <a href="https://doi.org/10.1234/maps">https://doi.org/10.1234/maps</a>.<br>`e` 青木 葵, and 中村 雪. 2031. <em>The Map of a River: A Field Guide</em>. | `S05` |
| `7219.md#1` | Suppress author. Journal series before volume<br><code>-@b</code> | (2031) | `b` Lake, Bo. 2031. <span>“Banks and Streams.”</span> <em>River Studies</em>, New series, vol. 3 (September): 125–39. | `S06` |
| `7321.md#1` | Math in a citation suffix<br><code>[@b, $x^2+y^2=z^2$]</code> | (Lake 2031, <math display="inline" xmlns="http://www.w3.org/1998/Math/MathML"><semantics><mrow><msup><mi>x</mi><mn>2</mn></msup><mo>+</mo><msup><mi>y</mi><mn>2</mn></msup><mo>=</mo><msup><mi>z</mi><mn>2</mn></msup></mrow><annotation encoding="application/x-tex">x^2+y^2=z^2</annotation></semantics></math>) | `b` Lake, Bo. 2031. <span>“Banks and Streams.”</span> <em>River Studies</em> 3 (September): 125–39. | `S07` |
| `7323.md#1` | Chapter label variants<br><code>@a [chap. 6]<br><br>@a [chapter 6]<br><br>@a [Chap. 6]<br><br>@a [Chapter 6]</code> | Reed (2031, chap. 6)<br>Reed (2031, chap. 6)<br>Reed (2031, chap. 6)<br>Reed (2031, chap. 6) | `a` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. | `S08` |
| `7324.md#1` | Textual suffix with comma, leading space or neither<br><code>@a [, and others]<br><br>@a [ and others]<br><br>@a [and others]</code> | Reed (2031, and others)<br>Reed (2031 and others)<br>Reed (2031, and others) | `a` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. | `S09` |
| `7329.md#2` | Textual, suppress-author, prefixes, suffixes and clusters<br><code>@a<br><br>@a [p. 33]<br><br>@a [p.33; see also @b]<br><br>[see @a p. 33]<br><br>[see @a, p. 33]<br><br>[see @a, p. 33 and *nearby*]<br><br>[@b; @a]<br><br>[see @b; @a]<br><br>[-@a]</code> | Reed (2031)<br>Reed (2031, 33)<br>Reed (2031, 33; see also Lake 2031)<br>(see Reed 2031, 33)<br>(see Reed 2031, 33)<br>(see Reed 2031, 33 and <em>nearby</em>)<br>(Lake 2031; Reed 2031)<br>(see Lake 2031; Reed 2031)<br>(2031) | `b` Lake, Bo. 2031. <span>“Banks and Streams.”</span> <em>River Studies</em> 3 (September): 125–39.<br>`a` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. | `S10` |
| `7329.md#3` | Same citation forms with a different upstream writer mode<br><code>@a<br><br>@a [p. 33]<br><br>@a [p.33; see also @b]<br><br>[see @a p. 33]<br><br>[see @a, p. 33]<br><br>[see @a, p. 33 and *nearby*]<br><br>[@b; @a]<br><br>[see @b; @a]<br><br>[-@a]</code> | Reed (2031)<br>Reed (2031, 33)<br>Reed (2031, 33; see also Lake 2031)<br>(see Reed 2031, 33)<br>(see Reed 2031, 33)<br>(see Reed 2031, 33 and <em>nearby</em>)<br>(Lake 2031; Reed 2031)<br>(see Lake 2031; Reed 2031)<br>(2031) | `b` Lake, Bo. 2031. <span>“Banks and Streams.”</span> <em>River Studies</em> 3 (September): 125–39.<br>`a` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. | `S11` |
| `7376.md#1` | Uncited inline reference<br><code>A sentence.</code> | — | — | `S12` |
| `7826.md#5` | Suppress bibliography with notes-after-punctuation<br><code>Probe [@b; @a], next.</code> | (Lake 2031; Reed 2031) | — | `S13` |
| `8354.md#1` | Cite all and link a DOI. Reference heading<br><code>## References</code> | — | `b` Lake, Bo. 2031. <span>“Banks and Streams.”</span> <em>River Studies</em> 3 (September): 125–39. <a href="https://doi.org/10.1234/maps">https://doi.org/10.1234/maps</a>. | `S14` |
| `9017.md#1` | Default type in bibliography<br><code># Probe<br><br>[@a].</code> | (Reed 2031) | `a` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. | `S15` |
| `9021.md#1` | Reference section title metadata<br><code>[@a].</code> | (Reed 2031) | `a` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. | `S16` |
| `9904.md#1` | Textual and parenthetical citations with prefix and locator<br><code>@a [p. 17]<br><br>[see @a]<br><br>[see @a, p. 17]</code> | Reed (2031, 17)<br>(see Reed 2031)<br>(see Reed 2031, 17) | `a` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. | `S17` |
| `10185.md#2` | Explicit references div and no date<br><code>[@a].<br><br># Sources<br><br>::: {#refs}<br>:::</code> | (Reed, n.d.) | `a` Reed, Ada J. n.d. <em>The Map of a River: A Field Guide</em>. | `S18` |
| `citeproc-87.md#1` | No date and nested quotation in suffix<br><code>[@a 50: «Note: &quot;inner&quot;»].</code> | (Survey Office, s.d., 50: «Note: “inner”») | — | `S19` |
| `pandoc-citeproc-7.md#1` | Magazine date range within a month<br><code>@a</code> | Reed (2031) | `a` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. September 24–26. | `S20` |
| `pandoc-citeproc-14.md#1` | Book, volume with its own title, and chapter in that volume<br><code>[@a, 1:12]. [@v, 12]. [@c, 12].</code> | (Reed 2031b, 1:12)<br>(Reed 2031c, 12)<br>(Reed 2031a, 12) | `c` Reed, Ada J. 2031a. <span>“River Towns.”</span> In <em>The Upper Valley</em>, vol. 1 of <em>The Map of a River: A Field Guide</em>. Map House.<br>`a` Reed, Ada J. 2031b. <em>The Map of a River: A Field Guide</em>.<br>`v` Reed, Ada J. 2031c. <em>The Upper Valley</em>. In <em>The Map of a River: A Field Guide</em>, vol. 1. | `S21` |
| `pandoc-citeproc-25.md#1` | Subtitle case and missing journal<br><code>[@a]</code> | (Reed 2031) | `a` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. September 24. | `S22` |
| `pandoc-citeproc-38.md#1` | Three authors with repeated family name<br><code>@b</code> | Reed et al. (2031) | `b` Reed, Ada J., Cal Reed, and Cy Baker. 2031. <span>“Banks and Streams.”</span> <em>River Studies</em> 3 (September): 125–39. | `S23` |
| `pandoc-citeproc-47.md#1` | Sort single author, editor substitute and coauthors. Repeat author substitution<br><code>[@a]. [@b]. [@d]. [@c].</code> | (Reed 2031a)<br>(Reed and Lake 2031)<br>(Reed 2031b)<br>(Reed et al. 2031) | `a` Reed, Ada J. 2031a. <em>The Map of a River: A Field Guide</em>.<br>`d` Reed, Ada J., ed. 2031b. <em>The Map of a River: A Field Guide</em>.<br>`c` Reed, Ada J., Cy Baker, and Di Moor. 2031. <em>The Map of a River: A Field Guide</em>.<br>`b` Reed, Ada J., and Bo Lake. 2031. <em>The Map of a River: A Field Guide</em>. | `S24` |
| `pandoc-citeproc-51.md#1` | Year ranges in journals<br><code>@b; @a</code> | Lake (2030–2031)<br>Reed (2031) | `b` Lake, Bo. 2030–2031. <span>“Banks and Streams.”</span> <em>River Studies</em> 3: 125–39.<br>`a` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. | `S25` |
| `pandoc-citeproc-65.md#1` | Literal date, series and total volumes<br><code>[@a]</code> | (Reed 2030–2034) | `a` Reed, Ada J. 2030–2034. <em>karten der flüsse</em>. 5 vols. Kartenreihe, 2.5. Map House. | `S26` |
| `pandoc-citeproc-70.md#1` | Non-English titles, two editors, chapter series and journal page ranges<br><code>[@b; @a]</code> | (Lake 2031; Reed 2031) | `b` Lake, Bo. 2031. <span>“banks and streams.”</span> <em>river studies</em> 3 (September): 163--193.<br>`a` Reed, Ada J. 2031. <span>“les cartes anciennes.”</span> In <em>cartes des vallées</em>, edited by Eve Holt and Bo Lake. Études du fleuve 50. Map House. | `S27` |
| `pandoc-citeproc-76.md#1` | Same author and year. Subtitle initial articles affect sort<br><code>@0, @1, @2, @3</code> | Reed (2031c)<br>Reed (2031d)<br>Reed (2031a)<br>Reed (2031b) | `2` Reed, Ada J. 2031a. <em>Maps and Rivers: A Survey Guide</em>. September 24.<br>`3` Reed, Ada J. 2031b. <em>Maps and Rivers: An Atlas</em>. September 24.<br>`0` Reed, Ada J. 2031c. <em>Maps and Rivers: Survey Notes</em>. September 24.<br>`1` Reed, Ada J. 2031d. <em>Maps and Rivers: The Survey Notes</em>. September 24. | `S28` |
| `pandoc-citeproc-87.md#1` | Web and newspaper dates. Same month, different months and different years<br><code>@0<br><br>@1<br><br>@2<br><br>@3<br><br>@4<br><br>@5</code> | Reed (2031a)<br>Reed (2031c)<br>Reed (2031b)<br>Reed (2031–2032)<br>Reed (2031e)<br>Reed (2031d) | `0` Reed, Ada J. 2031a. <span>“The Map of a River: A Field Guide.”</span> Survey Online, September 24. <a href="https://example.org/w">https://example.org/w</a>.<br>`2` Reed, Ada J. 2031b. <span>“The Map of a River: A Field Guide.”</span> Survey Online, September–October. <a href="https://example.org/w">https://example.org/w</a>.<br>`1` Reed, Ada J. 2031c. <span>“The Map of a River: A Field Guide.”</span> Survey Online, September 24–October 25. <a href="https://example.org/w">https://example.org/w</a>.<br>`5` Reed, Ada J. 2031d. <span>“The Map of a River: A Field Guide.”</span> <em>Survey Online</em>, September–October. <a href="https://example.org/w">https://example.org/w</a>.<br>`4` Reed, Ada J. 2031e. <span>“The Map of a River: A Field Guide.”</span> <em>Survey Online</em>, September 24–October 25. <a href="https://example.org/w">https://example.org/w</a>.<br>`3` Reed, Ada J. 2031–2032. <span>“The Map of a River: A Field Guide.”</span> Survey Online, December 31–January 1. <a href="https://example.org/w">https://example.org/w</a>. | `S29` |
| `pandoc-citeproc-118.md#1` | Director when there is no author<br><code>[@x]</code> | (Wood 2031) | `x` Wood, Dee, dir. 2031. <em>The Map of a River: A Field Guide</em>. Documentary. Map House. DVD. | `S30` |
| `pandoc-citeproc-136.md#1` | Anonymous Italian title and publisher place alone<br><code>@a</code> | <em>la carta di un fiume</em> (2031) | `a` <em>la carta di un fiume</em>. 2031. Torino. | `S31` |
| `pandoc-citeproc-175.md#1` | Magazine month-only range and pages<br><code>[@a]</code> | (Reed 2031) | `a` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> <em>Survey Quarterly</em>, January–February, 33–44. | `S32` |
| `pandoc-citeproc-250.md#1` | Author-in-text citation links and no date<br><code>@a</code> | Reed (<a href="#ref-a" role="doc-biblioref">n.d.</a>) | `a` Reed, Ada J. n.d. <em>The Map of a River: A Field Guide</em>. | `S33` |
| `pandoc-citeproc-301.md#1` | Anonymous default-type titles with punctuation and emphasis<br><code>@a; @b</code> | <em>Maps for N.R. Wood (on Rivers)</em> (n.d.)<br><em><em>Trial:</em> A Survey: An Abridgement</em> (n.d.) | `a` <em>Maps for N.R. Wood (on Rivers)</em>. n.d.<br>`b` <em><em>Trial:</em> A Survey: An Abridgement</em>. n.d. | `S34` |
| `pandoc-citeproc-307.md#1` | French document, adjacent citation and punctuation<br><code>Bonjour[@b] !</code> | (Lake 2031) | `b` Lake, Bo. 2031. <span>« La carte du fleuve »</span>. <em>river studies</em> 3 (septembre): 125‑39. | `S35` |
| `pandoc-citeproc-320a.md#1` | Sort names with particles, apostrophes and transliteration marks<br><code>[@0; @1; @2; @3; @4; @5; @6]</code> | (al-ʾNūr 2031; al-ʿNūr 2031; al-’Nūr 2031a, 2031b; al-‘Nūr 2031; al-Nuss 2031; al-Nyman 2031) | `3` ‘Nūr, Jan al-. 2031. <em>The Map of a River: A Field Guide</em>.<br>`0` ʾNūr, Jan al-. 2031. <em>The Map of a River: A Field Guide</em>.<br>`1` ʿNūr, Jan al-. 2031. <em>The Map of a River: A Field Guide</em>.<br>`2` ’Nūr, Jan al-. 2031a. <em>The Map of a River: A Field Guide</em>.<br>`4` ’Nūr, Jan al-. 2031b. <em>The Map of a River: A Field Guide</em>.<br>`5` Nuss, Jan al-. 2031. <em>The Map of a River: A Field Guide</em>.<br>`6` Nyman, Jan al-. 2031. <em>The Map of a River: A Field Guide</em>. | `S36` |
| `pandoc-citeproc-325.md#1` | Disambiguate two undated works with the same author and no title<br><code>[@a; @b]</code> | (Reed, n.d.-a, n.d.-b) | `a` Reed, Ada J. n.d.-a.<br>`b` Reed, Ada J. n.d.-b. | `S37` |
| `pandoc-citeproc-356.md#1` | A citation to an alternate ID produces a missing-reference warning<br><code>[@alias]</code> | (<strong>alias?</strong>) | — | `S38` |
| `pandoc-citeproc-371.md#1` | Status without a date after dated works of the same author<br><code>[@b; @a]</code> | (Reed 2031, in press) | `b` Reed, Ada J. 2031. <em>Maps of Cities</em>.<br>`a` Reed, Ada J. In press. <em>Maps of Lakes</em>. | `S39` |
| `pandoc-citeproc-401.md#1` | Same author and year with different original years, suppress-author cluster<br><code>Reed [-@a; @b] wrote.</code> | ([2003] 2031, [2000] 2031) | `b` Reed, Ada J. (2000) 2031. <em>The Map of a River: A Field Guide</em>.<br>`a` Reed, Ada J. (2003) 2031. <em>The Map of a River: A Field Guide</em>. | `S40` |
| `pandoc-citeproc-408.md#1` | One author versus two with the same family name<br><code>[@b; @a]</code> | (Reed and Reed 2031; Reed 2031) | `a` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>.<br>`b` Reed, Ada J., and Bea Reed. 2031. <em>Maps of Lakes</em>. | `S41` |
| `pandoc-citeproc-416.md#1` | Dated newspaper entries of the same author, sorting and year collapse<br><code>[@a; @b; @c]<br><br># References {-}</code> | (Reed 2031, 2027, 2028) | `b` Reed, Ada J. 2027. <em>The Map of a River: A Field Guide</em>. December 12–13.<br>`c` Reed, Ada J. 2028. <em>The Map of a River: A Field Guide</em>.<br>`a` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. December 13. | `S42` |
| `pandoc-citeproc-chicago-author-date.md#1` | Missing references, linked citation modes, notes, varied locators, markup and suffixes<br><code>[@missing]<br><br>@missing<br><br>@a<br><br>@a [p. 33]<br><br>@a [p.33; see also @b]<br><br>[see @a p. 33]<br><br>[see @a, p. 33]<br><br>[see @a, p. 33 and *nearby*]<br><br>[@b; @a]<br><br>[see @b; @a]<br><br>[-@a]<br><br>[@a, 33]<br><br>[@a, pp. 101-129]<br><br>[@a, 2:33]<br><br>[@a, chap. 3]<br><br>[@a, sec. 3]<br><br>[@a, para. 4]<br><br>[@a, fig. 5]<br><br>Body.^[See @a [p. 12] and [-@b].]<br><br>[@a pp. 33, 35-37, and later]<br><br>[*see* @a p. **32**]</code> | (<a href="#ref-missing" role="doc-biblioref"><strong>missing?</strong></a>)<br>(<a href="#ref-missing" role="doc-biblioref"><strong>missing?</strong></a>)<br>Reed (<a href="#ref-a" role="doc-biblioref">2031</a>)<br>Reed (<a href="#ref-a" role="doc-biblioref">2031, 33</a>)<br>Reed (<a href="#ref-a" role="doc-biblioref">2031, 33</a>; see also <a href="#ref-b" role="doc-biblioref">Lake 2031</a>)<br>(see <a href="#ref-a" role="doc-biblioref">Reed 2031, 33</a>)<br>(see <a href="#ref-a" role="doc-biblioref">Reed 2031, 33</a>)<br>(see <a href="#ref-a" role="doc-biblioref">Reed 2031, 33</a> and <em>nearby</em>)<br>(<a href="#ref-b" role="doc-biblioref">Lake 2031</a>; <a href="#ref-a" role="doc-biblioref">Reed 2031</a>)<br>(see <a href="#ref-b" role="doc-biblioref">Lake 2031</a>; <a href="#ref-a" role="doc-biblioref">Reed 2031</a>)<br>(<a href="#ref-a" role="doc-biblioref">2031</a>)<br>(<a href="#ref-a" role="doc-biblioref">Reed 2031, 33</a>)<br>(<a href="#ref-a" role="doc-biblioref">Reed 2031, 101–29</a>)<br>(<a href="#ref-a" role="doc-biblioref">Reed 2031, 2:33</a>)<br>(<a href="#ref-a" role="doc-biblioref">Reed 2031, chap. 3</a>)<br>(<a href="#ref-a" role="doc-biblioref">Reed 2031, sec. 3</a>)<br>(<a href="#ref-a" role="doc-biblioref">Reed 2031, para. 4</a>)<br>(<a href="#ref-a" role="doc-biblioref">Reed 2031, fig. 5</a>)<br>(<a href="#ref-a" role="doc-biblioref">Reed 2031, 33, 35–37</a>, and later)<br>(<em>see</em> <a href="#ref-a" role="doc-biblioref">Reed 2031, 32</a>)<br>Reed (<a href="#ref-a" role="doc-biblioref">2031, 12</a>)<br>(<a href="#ref-b" role="doc-biblioref">2031</a>) | `b` Lake, Bo. 2031. <span>“Banks and Streams.”</span> <em>River Studies</em> 3 (September): 125–39.<br>`a` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. | `S43` |
| `pandoc-citeproc-no-author.md#1` | Anonymous articles in the same and different containers<br><code>@0 [p. 3], @1, @2, @3, @4, @5</code> | <em>Survey Notes</em> (2031a, 3)<br><em>Survey Notes</em> (2031b)<br><em>Survey Notes</em> (2031c)<br><em>Survey Notes</em> (2031d)<br><em>River News</em> (2031a)<br><em>River News</em> (2031b) | `4` <em>River News</em>. 2031a. <span>“Map 4.”</span> September 24.<br>`5` <em>River News</em>. 2031b. <span>“Map 5.”</span> September 24.<br>`0` <em>Survey Notes</em>. 2031a. <span>“Map 0.”</span> September 24.<br>`1` <em>Survey Notes</em>. 2031b. <span>“Map 1.”</span> September 24.<br>`2` <em>Survey Notes</em>. 2031c. <span>“Map 2.”</span> September 24.<br>`3` <em>Survey Notes</em>. 2031d. <span>“Map 3.”</span> September 24. | `S44` |
| `pandoc-citeproc-number-of-volumes.md#1` | Book with total volumes and a publisher<br><code>@a</code> | Reed (2031) | `a` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. 2 vols. Map House. | `S45` |
| `unicode-collation.md#3` | Cite all with French backward accent collation | — | `0` <span class="nocase">cote</span>. s. d.<br>`3` <span class="nocase">côte</span>. s. d.<br>`2` <span class="nocase">coté</span>. s. d.<br>`1` <span class="nocase">côté</span>. s. d. | `S46` |
| `extra` | Locator types and ranges<br><code>[@a, 33]<br><br>[@a, pp. 101-129]<br><br>[@a, 2:33]<br><br>[@a, chap. 3]<br><br>[@a, sec. 3]<br><br>[@a, para. 4]<br><br>[@a, fig. 5]</code> | (Reed 2031, 33)<br>(Reed 2031, 101–29)<br>(Reed 2031, 2:33)<br>(Reed 2031, chap. 3)<br>(Reed 2031, sec. 3)<br>(Reed 2031, para. 4)<br>(Reed 2031, fig. 5) | `a` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. | `S47` |
| `extra` | Year suffix, collapse, reversed cluster and same family name across years<br><code>[@a; @b]<br><br>[@b; @a]<br><br>[@a; @c; @d]</code> | (A. J. Reed 2031b, 2031a)<br>(A. J. Reed 2031a, 2031b)<br>(A. J. Reed 2031b; Reed 2032; C. Reed 2031) | `b` Reed, Ada J. 2031a. <em>An Atlas of Lakes</em>.<br>`a` Reed, Ada J. 2031b. <em>The Map of a River: A Field Guide</em>.<br>`c` Reed, Bea. 2032. <em>The Map of a River: A Field Guide</em>.<br>`d` Reed, Cai. 2031. <em>The Map of a River: A Field Guide</em>. | `S48` |
| `extra` | Citations retain input order across authors. Bibliography sorts authors<br><code>[@a; @b]<br><br>[@b; @a]</code> | (Reed 2031; Lake 2031)<br>(Lake 2031; Reed 2031) | `b` Lake, Bo. 2031. <span>“Banks and Streams.”</span> <em>River Studies</em> 3 (September): 125–39.<br>`a` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. | `S49` |
| `extra` | DOI versus URL, undated web access, and an explicit year suffix<br><code>[@a; @d]</code> | (Reed, n.d., 2031) | `d` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. September 24. <a href="https://doi.org/10.1234/river">https://doi.org/10.1234/river</a>.<br>`a` Reed, Ada J. n.d. <span>“The Map of a River: A Field Guide.”</span> Accessed January 4, 2032. <a href="https://example.org/maps">https://example.org/maps</a>. | `S50` |
| `extra` | Unpublished conference paper, with and without an event date<br><code>[@a; @b]</code> | (Reed 2031a, 2031b) | `a` Reed, Ada J. 2031a. <span>“The Map of a River: A Field Guide.”</span> Paper presented at River Congress, April 4, 2032, North Port. September 24.<br>`b` Reed, Ada J. 2031b. <span>“The Map of a River: A Field Guide.”</span> Paper presented at Survey Congress, East Port. September 24. | `S51` |
| `extra` | Short titles substitute for missing names. Long identical titles with different short forms<br><code>@a<br><br>[@b]</code> | <em>River Map</em> (2031)<br>(<em>The Atlas</em> 2031) | `a` <em>The Map of a River: A Field Guide</em>. 2031.<br>`b` <em>The Map of a River: A Field Guide</em>. 2031. September 24. | `S52` |
| `extra` | Same author and year with locators. The name collapses, and both years and locators stay<br><code>[@a, 10; @b, 12]<br><br>[@b; @a]</code> | (Reed 2031b, 10; 2031a, 12)<br>(Reed 2031a, 2031b) | `b` Reed, Ada J. 2031a. <em>An Atlas of Lakes</em>.<br>`a` Reed, Ada J. 2031b. <em>The Map of a River: A Field Guide</em>. | `S53` |
| `extra` | Status with issued year and separate access date without URL<br><code>[@a; @b]</code> | (Reed 2031, n.d.) | `a` Reed, Ada J. 2031. <em>The Map of a River: A Field Guide</em>. Submitted.<br>`b` Reed, Ada J. n.d. <span>“The Map of a River: A Field Guide.”</span> | `S54` |
| `extra` | Repeated authors sorted by year and title. Single and double editor or translator roles<br><code>[@a; @b]</code> | (Reed 2030; Holt and Baker 2031) | `b` Holt, Eve, and Cy Baker, eds. 2031. <em>The Map of a River: A Field Guide</em>.<br>`a` Reed, Ada J. 2030. <em>The Map of a River: A Field Guide</em>. Edited by Eve Holt and Cy Baker. Translated by Tam Vale. | `S55` |
| `extra` | Undated French reference and citation<br><code>[@a]</code> | (Reed, s. d.) | `a` Reed, Ada J. s. d. <em>the map of a river: a field guide</em>. | `S56` |
| `extra` | Explicit volume, event details and number of volumes in a chapter<br><code>[@a]</code> | (Reed 2031) | `a` Reed, Ada J. 2031. <span>“The Map of a River: A Field Guide.”</span> In <em>The Coast</em>, edited by Eve Holt, vol. 2 of <em>River Anthology</em>. Atlases 3. | `S57` |
| `extra` | Two metadata records with the same ID in a reference list<br><code>[@a]</code> | (Reed 2031) | `a` Reed, Ada J. 2031. <em>New Title</em>. | `S58` |

## Exact scenario references

Each block gives the Citation Style Language (CSL) JSON references and
document metadata. Use the citation text in the scenario table.

<details><summary>Show scenario inputs</summary>

### S01

```json
{"references":[{"id":"x","type":"paper-conference","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]},"publisher":"Map House","publisher-place":"North Port","container-title":"proceedings of the survey congress","event-title":"River Congress","event-place":"East Port","page":"101-129"}],"metadata":{}}
```

### S02

```json
{"references":[],"metadata":{}}
```

### S03

```json
{"references":[{"id":"a","type":"book","title":"karten und flüsse: ein atlas","author":[{"given":"Ada J.","family":"Reed"},{"given":"Bo","family":"Lake"},{"given":"Cy","family":"Baker"}],"issued":{"date-parts":[[2031,9,24]]},"edition":3,"language":"de-DE","title-short":"Flusskarten","publisher":"Map House","publisher-place":"North Port"}],"metadata":{}}
```

### S04

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"b","type":"book","title":"banks and streams","container-title":"river studies","volume":3,"page":"125-139"}],"metadata":{}}
```

### S05

```json
{"references":[{"id":"b","type":"article-journal","title":"banks and streams","author":[{"given":"Bo","family":"Lake"}],"issued":{"date-parts":[[2031,9]]},"container-title":"river studies","volume":3,"page":"125-139"},{"id":"p","type":"article-journal","title":"the map of a river: a field guide","author":[{"given":"Jan","family":"Meer","non-dropping-particle":"van"},{"given":"Lea","family":"Noor","dropping-particle":"de"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"e","type":"book","title":"the map of a river: a field guide","author":[{"literal":"青木 葵"},{"literal":"中村 雪"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"d","type":"article-journal","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"},{"literal":"others"}],"issued":{"date-parts":[[2031,9,24]]},"DOI":"10.1234/maps"}],"metadata":{"nocite":"[@*]"}}
```

### S06

```json
{"references":[{"id":"b","type":"article-journal","title":"banks and streams","author":[{"given":"Bo","family":"Lake"}],"issued":{"date-parts":[[2031,9,24]]},"container-title":"river studies","volume":3,"page":"125-139","collection-title":"New series"}],"metadata":{}}
```

### S07

```json
{"references":[{"id":"b","type":"article-journal","title":"banks and streams","author":[{"given":"Bo","family":"Lake"}],"issued":{"date-parts":[[2031,9,24]]},"container-title":"river studies","volume":3,"page":"125-139"}],"metadata":{}}
```

### S08

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}}],"metadata":{}}
```

### S09

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}}],"metadata":{}}
```

### S10

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"b","type":"article-journal","title":"banks and streams","author":[{"given":"Bo","family":"Lake"}],"issued":{"date-parts":[[2031,9,24]]},"container-title":"river studies","volume":3,"page":"125-139"}],"metadata":{}}
```

### S11

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"b","type":"article-journal","title":"banks and streams","author":[{"given":"Bo","family":"Lake"}],"issued":{"date-parts":[[2031,9,24]]},"container-title":"river studies","volume":3,"page":"125-139"}],"metadata":{}}
```

### S12

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}}],"metadata":{}}
```

### S13

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"b","type":"article-journal","title":"banks and streams","author":[{"given":"Bo","family":"Lake"}],"issued":{"date-parts":[[2031,9,24]]},"container-title":"river studies","volume":3,"page":"125-139"}],"metadata":{"suppress-bibliography":true,"notes-after-punctuation":true}}
```

### S14

```json
{"references":[{"id":"b","type":"article-journal","title":"banks and streams","author":[{"given":"Bo","family":"Lake"}],"issued":{"date-parts":[[2031,9,24]]},"container-title":"river studies","volume":3,"page":"125-139","DOI":"10.1234/maps"}],"metadata":{"nocite":"[@*]"}}
```

### S15

```json
{"references":[{"id":"a","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}}],"metadata":{}}
```

### S16

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}}],"metadata":{"reference-section-title":"Sources"}}
```

### S17

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}}],"metadata":{}}
```

### S18

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}]}],"metadata":{}}
```

### S19

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"literal":"Survey Office"}]}],"metadata":{"lang":"it-IT","suppress-bibliography":true}}
```

### S20

```json
{"references":[{"id":"a","type":"article-magazine","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24],[2031,9,26]]}}],"metadata":{}}
```

### S21

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"v","type":"book","title":"the upper valley","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]},"container-title":"the map of a river: a field guide","volume":1},{"id":"c","type":"chapter","title":"river towns","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]},"container-title":"the map of a river: a field guide","volume":1,"volume-title":"the upper valley","page":"31-48","publisher":"Map House"}],"metadata":{}}
```

### S22

```json
{"references":[{"id":"a","type":"article-journal","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}}],"metadata":{}}
```

### S23

```json
{"references":[{"id":"b","type":"article-journal","title":"banks and streams","author":[{"given":"Ada J.","family":"Reed"},{"given":"Cal","family":"Reed"},{"given":"Cy","family":"Baker"}],"issued":{"date-parts":[[2031,9,24]]},"container-title":"river studies","volume":3,"page":"125-139"}],"metadata":{}}
```

### S24

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"d","type":"book","title":"the map of a river: a field guide","issued":{"date-parts":[[2031,9,24]]},"editor":[{"given":"Ada J.","family":"Reed"}]},{"id":"b","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"},{"given":"Bo","family":"Lake"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"c","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"},{"given":"Cy","family":"Baker"},{"given":"Di","family":"Moor"}],"issued":{"date-parts":[[2031,9,24]]}}],"metadata":{}}
```

### S25

```json
{"references":[{"id":"b","type":"article-journal","title":"banks and streams","author":[{"given":"Bo","family":"Lake"}],"issued":{"date-parts":[[2030],[2031]]},"container-title":"river studies","volume":3,"page":"125-139"},{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}}],"metadata":{}}
```

### S26

```json
{"references":[{"id":"a","type":"book","title":"karten der flüsse","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"literal":"2030_2034"},"language":"de","number-of-volumes":5,"collection-title":"Kartenreihe","collection-number":"2.5","publisher":"Map House"}],"metadata":{}}
```

### S27

```json
{"references":[{"id":"a","type":"chapter","title":"les cartes anciennes","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]},"language":"fr","editor":[{"given":"Eve","family":"Holt"},{"given":"Bo","family":"Lake"}],"container-title":"cartes des vallées","page":"155--174","publisher":"Map House","collection-title":"Études du fleuve","collection-number":50},{"id":"b","type":"article-journal","title":"banks and streams","author":[{"given":"Bo","family":"Lake"}],"issued":{"date-parts":[[2031,9,24]]},"container-title":"river studies","volume":3,"page":"163--193","language":"la"}],"metadata":{}}
```

### S28

```json
{"references":[{"id":"0","type":"article-journal","title":"maps and rivers: survey notes","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"1","type":"article-journal","title":"maps and rivers: the survey notes","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"2","type":"article-journal","title":"maps and rivers: a survey guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"3","type":"article-journal","title":"maps and rivers: an atlas","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}}],"metadata":{}}
```

### S29

```json
{"references":[{"id":"0","type":"webpage","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]},"URL":"https://example.org/w","container-title":"Survey Online"},{"id":"1","type":"webpage","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24],[2031,10,25]]},"URL":"https://example.org/w","container-title":"Survey Online"},{"id":"2","type":"webpage","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9],[2031,10]]},"URL":"https://example.org/w","container-title":"Survey Online"},{"id":"3","type":"webpage","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,12,31],[2032,1,1]]},"URL":"https://example.org/w","container-title":"Survey Online"},{"id":"4","type":"article-newspaper","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24],[2031,10,25]]},"URL":"https://example.org/w","container-title":"Survey Online"},{"id":"5","type":"article-newspaper","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9],[2031,10]]},"URL":"https://example.org/w","container-title":"Survey Online"}],"metadata":{}}
```

### S30

```json
{"references":[{"id":"x","type":"motion_picture","title":"the map of a river: a field guide","issued":{"date-parts":[[2031,9,24]]},"publisher":"Map House","publisher-place":"North Port","director":[{"given":"Dee","family":"Wood"}],"medium":"DVD","genre":"documentary"}],"metadata":{}}
```

### S31

```json
{"references":[{"id":"a","type":"book","title":"la carta di un fiume","issued":{"date-parts":[[2031,9,24]]},"publisher-place":"Torino","language":"it-IT"}],"metadata":{}}
```

### S32

```json
{"references":[{"id":"a","type":"article-magazine","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,1],[2031,2]]},"container-title":"Survey Quarterly","page":"33-44"}],"metadata":{}}
```

### S33

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}]}],"metadata":{"link-citations":true}}
```

### S34

```json
{"references":[{"id":"a","type":"book","title":"maps for N.R. Wood (on rivers)"},{"id":"b","type":"book","title":"<i>Trial:</i> a survey: an abridgement"}],"metadata":{}}
```

### S35

```json
{"references":[{"id":"b","type":"article-journal","title":"La carte du fleuve","author":[{"given":"Bo","family":"Lake"}],"issued":{"date-parts":[[2031,9,24]]},"container-title":"river studies","volume":3,"page":"125-139"}],"metadata":{"lang":"fr-FR"}}
```

### S36

```json
{"references":[{"id":"0","type":"book","title":"the map of a river: a field guide","author":[{"family":"ʾNūr","given":"Jan","non-dropping-particle":"al-"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"1","type":"book","title":"the map of a river: a field guide","author":[{"family":"ʿNūr","given":"Jan","non-dropping-particle":"al-"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"2","type":"book","title":"the map of a river: a field guide","author":[{"family":"'Nūr","given":"Jan","non-dropping-particle":"al-"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"3","type":"book","title":"the map of a river: a field guide","author":[{"family":"‘Nūr","given":"Jan","non-dropping-particle":"al-"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"4","type":"book","title":"the map of a river: a field guide","author":[{"family":"’Nūr","given":"Jan","non-dropping-particle":"al-"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"5","type":"book","title":"the map of a river: a field guide","author":[{"family":"Nuss","given":"Jan","non-dropping-particle":"al-"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"6","type":"book","title":"the map of a river: a field guide","author":[{"family":"Nyman","given":"Jan","non-dropping-particle":"al-"}],"issued":{"date-parts":[[2031,9,24]]}}],"metadata":{}}
```

### S37

```json
{"references":[{"id":"a","type":"book","author":[{"given":"Ada J.","family":"Reed"}]},{"id":"b","type":"book","author":[{"given":"Ada J.","family":"Reed"}]}],"metadata":{}}
```

### S38

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]},"other-ids":["alias","second"]}],"metadata":{}}
```

### S39

```json
{"references":[{"id":"a","type":"book","title":"maps of lakes","author":[{"given":"Ada J.","family":"Reed"}],"status":"in press"},{"id":"b","type":"book","title":"maps of cities","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}}],"metadata":{}}
```

### S40

```json
{"references":[{"id":"a","type":"chapter","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]},"original-date":{"date-parts":[[2003]]}},{"id":"b","type":"chapter","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]},"original-date":{"date-parts":[[2000]]}}],"metadata":{}}
```

### S41

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"b","type":"book","title":"maps of lakes","author":[{"given":"Ada J.","family":"Reed"},{"given":"Bea","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}}],"metadata":{}}
```

### S42

```json
{"references":[{"id":"a","type":"article-newspaper","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,12,13]]}},{"id":"b","type":"article-newspaper","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2027,12,12],[2027,12,13]]}},{"id":"c","type":"article-newspaper","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2028]]}}],"metadata":{}}
```

### S43

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"b","type":"article-journal","title":"banks and streams","author":[{"given":"Bo","family":"Lake"}],"issued":{"date-parts":[[2031,9,24]]},"container-title":"river studies","volume":3,"page":"125-139"}],"metadata":{"link-citations":true}}
```

### S44

```json
{"references":[{"id":"0","type":"article-magazine","title":"map 0","issued":{"date-parts":[[2031,9,24]]},"container-title":"Survey Notes"},{"id":"1","type":"article-magazine","title":"map 1","issued":{"date-parts":[[2031,9,24]]},"container-title":"Survey Notes"},{"id":"2","type":"article-magazine","title":"map 2","issued":{"date-parts":[[2031,9,24]]},"container-title":"Survey Notes"},{"id":"3","type":"article-magazine","title":"map 3","issued":{"date-parts":[[2031,9,24]]},"container-title":"Survey Notes"},{"id":"4","type":"article-magazine","title":"map 4","issued":{"date-parts":[[2031,9,24]]},"container-title":"River News"},{"id":"5","type":"article-magazine","title":"map 5","issued":{"date-parts":[[2031,9,24]]},"container-title":"River News"}],"metadata":{}}
```

### S45

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]},"number-of-volumes":2,"publisher":"Map House","publisher-place":"North Port"}],"metadata":{}}
```

### S46

```json
{"references":[{"id":"0","type":"book","author":[{"literal":"cote"}]},{"id":"1","type":"book","author":[{"literal":"côté"}]},{"id":"2","type":"book","author":[{"literal":"coté"}]},{"id":"3","type":"book","author":[{"literal":"côte"}]}],"metadata":{"nocite":"[@*]","lang":"fr-FR-u-kb-true"}}
```

### S47

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}}],"metadata":{}}
```

### S48

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"b","type":"book","title":"an atlas of lakes","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"c","type":"book","title":"the map of a river: a field guide","author":[{"family":"Reed","given":"Bea"}],"issued":{"date-parts":[[2032]]}},{"id":"d","type":"book","title":"the map of a river: a field guide","author":[{"family":"Reed","given":"Cai"}],"issued":{"date-parts":[[2031,9,24]]}}],"metadata":{}}
```

### S49

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"b","type":"article-journal","title":"banks and streams","author":[{"given":"Bo","family":"Lake"}],"issued":{"date-parts":[[2031,9,24]]},"container-title":"river studies","volume":3,"page":"125-139"}],"metadata":{}}
```

### S50

```json
{"references":[{"id":"a","type":"webpage","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"URL":"https://example.org/maps","accessed":{"date-parts":[[2032,1,4]]}},{"id":"d","type":"article-journal","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]},"URL":"https://example.org/ignored","DOI":"10.1234/river","year-suffix":"z"}],"metadata":{}}
```

### S51

```json
{"references":[{"id":"a","type":"paper-conference","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]},"event-title":"River Congress","event-place":"North Port","event-date":{"date-parts":[[2032,4,4]]}},{"id":"b","type":"paper-conference","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]},"event-title":"Survey Congress","event-place":"East Port"}],"metadata":{}}
```

### S52

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","issued":{"date-parts":[[2031,9,24]]},"title-short":"River Map"},{"id":"b","type":"article-journal","title":"the map of a river: a field guide","issued":{"date-parts":[[2031,9,24]]},"title-short":"The Atlas"}],"metadata":{}}
```

### S53

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"b","type":"book","title":"an atlas of lakes","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}}],"metadata":{}}
```

### S54

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]},"status":"submitted"},{"id":"b","type":"webpage","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"accessed":{"date-parts":[[2032,1,4]]}}],"metadata":{}}
```

### S55

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2030]]},"editor":[{"given":"Eve","family":"Holt"},{"given":"Cy","family":"Baker"}],"translator":[{"given":"Tam","family":"Vale"}]},{"id":"b","type":"book","title":"the map of a river: a field guide","issued":{"date-parts":[[2031]]},"editor":[{"given":"Eve","family":"Holt"},{"given":"Cy","family":"Baker"}],"translator":[{"given":"Eve","family":"Holt"},{"given":"Cy","family":"Baker"}]}],"metadata":{}}
```

### S56

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}]}],"metadata":{"lang":"fr-FR"}}
```

### S57

```json
{"references":[{"id":"a","type":"chapter","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]},"volume":2,"editor":[{"given":"Eve","family":"Holt"}],"container-title":"river anthology","volume-title":"the coast","number-of-volumes":5,"collection-title":"Atlases","collection-number":3,"page":"101-129"}],"metadata":{}}
```

### S58

```json
{"references":[{"id":"a","type":"book","title":"the map of a river: a field guide","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}},{"id":"a","type":"book","title":"new title","author":[{"given":"Ada J.","family":"Reed"}],"issued":{"date-parts":[[2031,9,24]]}}],"metadata":{}}
```

</details>
