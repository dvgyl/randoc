#!/usr/bin/env python3
"""Makes tests/regressions.rs: review findings as tests with oracle output.

Usage: RANDOC_ORACLE_PANDOC=... python3 tools/regressions.py > tests/regressions.rs

Each entry is a name, the blocks of a document as JSON, the output format and
the options. The script runs the oracle with the matching command-line
options and writes one test for each entry.
"""
import json
import os
import subprocess
import sys

ORACLE = os.environ["RANDOC_ORACLE_PANDOC"]

def S(text): return {"t": "Str", "c": text}
def Pl(*inlines): return {"t": "Plain", "c": list(inlines)}
def Pa(*inlines): return {"t": "Para", "c": list(inlines)}
def A(ident="", classes=None, attrs=None): return [ident, classes or [], attrs or []]
def H(level, title, attr=None): return {"t": "Header", "c": [level, attr or A(), [S(title)]]}
def Div(attr, blocks): return {"t": "Div", "c": [attr, blocks]}
def Code(text, attr=None): return {"t": "Code", "c": [attr or A(), text]}
def CodeBlock(text, attr=None): return {"t": "CodeBlock", "c": [attr or A(), text]}
def Img(attr, alt, url): return {"t": "Image", "c": [attr, alt, [url, ""]]}
LB = {"t": "LineBreak"}
SP = {"t": "Space"}
def cell(blocks, align="AlignDefault", rs=1, cs=1): return [A(), {"t": align}, rs, cs, blocks]
def row(*cells): return [A(), list(cells)]
def W(x): return {"t": "ColWidth", "c": x}
D = {"t": "ColWidthDefault"}
def spec(align="AlignDefault", width=D): return [{"t": align}, width]
def table(specs, head, body, foot=None, caption=None, attr=None):
    return {"t": "Table", "c": [attr or A(), [None, caption or []], specs, [A(), head],
                                 [[A(), 0, [], body]], [A(), foot or []]]}
def Cite(suffix):
    return {"t": "Cite", "c": [[{"citationId": "k", "citationPrefix": [], "citationSuffix": suffix,
                                 "citationMode": {"t": "NormalCitation"}, "citationNoteNum": 0,
                                 "citationHash": 0}], []]}
def OL(start, style, delim, items):
    return {"t": "OrderedList", "c": [[start, {"t": style}, {"t": delim}], items]}

CASES = [
    # Codex R04, Opus M2
    ("unicode_spaces_and_spanish_ligatures", [Pa(S("x​y­z w!`e?`f"))], "latex", []),
    ("unicode_spaces_without_smart", [Pa(S("x​y­z w!`e"))], "latex-smart", []),
    ("nonbreaking_space_in_code", [Pa(Code("a b​c"))], "latex", []),
    # Codex R05
    ("dimension_without_integer_part", [Pa(Img(A("", [], [["width", ".5cm"]]), [], "a.png"))], "latex", []),
    ("dimension_without_fraction_digits", [Pa(Img(A("", [], [["width", "5.in"]]), [], "a.png"))], "latex", []),
    ("dimension_with_leading_space", [Pa(Img(A("", [], [["width", " 3cm"]]), [], "a.png"))], "latex", []),
    ("dimension_with_fractional_pixels", [Pa(Img(A("", [], [["width", "100.7px"]]), [], "a.png"))], "latex", []),
    # Codex R06, Opus M4
    ("code_with_end_verbatim", [CodeBlock("\\end{verbatim}")], "latex", []),
    ("code_with_end_verbatim_no_highlighting", [CodeBlock("x\n\\end{verbatim}")], "latex", ["--syntax-highlighting=none"]),
    # Codex R07
    ("item_with_only_a_list_is_tight", [{"t": "BulletList", "c": [[{"t": "BulletList", "c": [[Pl(S("x"))]]}]]}], "latex", []),
    ("item_with_a_loose_list_is_loose", [{"t": "BulletList", "c": [[{"t": "BulletList", "c": [[Pa(S("x"))]]}]]}], "latex", []),
    # Codex R08
    ("empty_sized_header_cell", [table([spec(width=W(1))], [row(cell([]))], [row(cell([Pl(S("x"))]))])], "latex", []),
    # Codex R09
    ("cell_after_the_last_column", [table([spec(), spec()], [], [row(cell([Pl(S("x"))], cs=2), cell([{"t": "BulletList", "c": [[Pl(S("hidden"))]]}]))])], "latex", []),
    # Codex R10
    ("inline_code_with_number_lines", [Pa(Code("a", A("", ["numberLines"])))], "latex", []),
    # Codex R11
    ("code_with_final_newline", [CodeBlock("a\n", A("", ["unknown"]))], "latex", []),
    # Codex R12
    ("row_end_at_narrow_width", [table([spec()], [], [row(cell([Pl(S("x"))]))], caption=[Pl(S("cap"))])], "latex", ["--columns=1"]),
    # Codex R13
    ("beamer_table_caption_below", [table([spec()], [], [row(cell([Pl(S("x"))]))], caption=[Pl(S("cap"))])], "beamer", ["--table-caption-position=below"]),
    # Codex R14, Opus M10
    ("listings_fortran", [CodeBlock("", A("", ["fortran"]))], "latex", ["--listings"]),
    ("listings_lisp_inline", [Pa(Code("x", A("", ["lisp"])))], "latex", ["--listings"]),
    # Opus M3
    ("code_in_figure_caption", [{"t": "Figure", "c": [A(), [None, [Pl(Code("c"))]], [Pl(Img(A(), [], "a.png"))]]}], "latex", []),
    ("code_in_table_caption", [table([spec()], [], [row(cell([Pl(S("x"))]))], caption=[Pl(Code("c"))])], "latex", []),
    # Opus M5
    ("sized_cell_with_line_break", [table([spec("AlignLeft", W(0.5)), spec("AlignLeft", W(0.5))], [], [row(cell([Pl(S("a"), LB, S("b"))]), cell([Pl(S("x"))]))])], "latex", []),
    ("sized_cell_ends_with_line_break", [table([spec("AlignLeft", W(0.5))], [], [row(cell([Pl(S("a"), LB)]))])], "latex", []),
    # Opus M6
    ("table_with_identifier_and_no_caption", [table([spec()], [], [row(cell([Pl(S("1"))]))], attr=A("tid", ["c"], [["k", "v"]]))], "latex", []),
    # Opus M7
    ("beamer_section_frame_options", [H(1, "A", A("", ["allowframebreaks"])), Pa(S("x"))], "beamer", ["--slide-level=2"]),
    ("beamer_section_frame_fragile", [H(1, "A", A("", ["shrink"])), Pa(Code("c"))], "beamer", ["--slide-level=3"]),
    ("beamer_fragile_attribute", [H(1, "A", A("", [], [["fragile", "singleslide"]])), Pa(S("x"))], "beamer", []),
    # Opus M8
    ("beamer_column_align", [H(1, "A"), Div(A("", ["columns"], [["align", "top"]]), [Div(A("", ["column"], [["align", "top"]]), [Pa(S("l"))]), Div(A("", ["column"], [["align", "bottom"], ["width", "3cm"]]), [Pa(S("r"))])])], "beamer", []),
    ("beamer_columns_unknown_align", [H(1, "A"), Div(A("", ["columns"], [["align", "weird"]]), [Div(A("", ["column"], [["align", "center"]]), [Pa(S("l"))])])], "beamer", []),
    # Opus M9
    ("beamer_heading_in_div", [Div(A("", ["x"]), [H(1, "A"), Pa(S("x"))])], "beamer", []),
    # Opus M11
    ("empty_paragraphs_extension", [Pa(), Pa(S("x")), Pa()], "latex+empty_paragraphs", []),
    ("literate_haskell_extension", [CodeBlock("x", A("", ["literate", "haskell"]))], "latex+literate_haskell", []),
    # Opus M12
    ("beamer_part_division", [H(1, "a"), H(2, "b"), H(3, "c"), H(4, "d"), H(5, "e"), H(6, "f")], "beamer", ["--top-level-division=part"]),
    ("beamer_chapter_division", [H(1, "a"), H(2, "b"), H(3, "c"), H(4, "d"), H(5, "e"), H(6, "f")], "beamer", ["--top-level-division=chapter"]),
    # Opus L1, L2
    ("link_in_strikeout", [Pa({"t": "Strikeout", "c": [{"t": "Link", "c": [A(), [S("t")], ["http://a", ""]]}]})], "latex", []),
    ("display_math_in_strikeout", [Pa({"t": "Strikeout", "c": [{"t": "Math", "c": [{"t": "DisplayMath"}, "x"]}]})], "latex", []),
    ("display_math_in_table_cell", [table([spec()], [], [row(cell([Pl({"t": "Math", "c": [{"t": "DisplayMath"}, "x"]})]))])], "latex", []),
    # Opus L3, L4
    ("curly_quotes_in_two_strings", [Pa(S("“q”"), S("“q”"))], "latex", []),
    ("note_text_after_the_paragraph", [Pa(S("a"), {"t": "Note", "c": [Pa(S("x"), {"t": "Note", "c": [Pa(S("in"))]}, S("y"))]}, S("after"), SP, S("more"))], "latex", []),
    # Opus L5 and Codex R15
    ("negative_heading_level", [H(-3, "T")], "latex", []),
    ("minimum_list_start", [OL(-9223372036854775808, "Decimal", "Period", [[Pl(S("a"))]])], "latex", []),
    ("task_marker_in_ordered_list", [OL(1, "Decimal", "Period", [[Pl(S("☐"), SP, S("a"))]])], "latex", []),
    ("underline_class_span", [Pa({"t": "Span", "c": [A("", ["underline"]), [S("a")]]})], "latex", []),
    ("underline_and_mark_span", [Pa({"t": "Span", "c": [A("", ["underline", "mark"]), [S("a")]]})], "latex", []),
    ("autolink_with_non_ascii", [Pa({"t": "Link", "c": [A(), [S("http://x.org/é")], ["http://x.org/é", ""]]})], "latex", []),
    ("image_options_order", [Pa(Img(A("", [], [["page", "2"], ["trim", "1 2 3 4"], ["clip", "true"]]), [S("a")], "a.png"))], "latex", []),
    ("figure_with_only_a_table", [{"t": "Figure", "c": [A(), [None, [Pl(S("c"))]], [table([spec()], [], [row(cell([Pl(S("x"))]))])]]}], "latex", []),
    ("table_without_column_specs", [table([], [], [row(cell([Pl(S("x"))]))])], "latex", []),
    ("code_block_in_cell", [table([spec()], [], [row(cell([CodeBlock("c")]))])], "latex", []),
    ("note_ends_with_line_break", [Pa(S("a"), {"t": "Note", "c": [Pa(S("x"), LB)]})], "latex", []),
    ("definition_starts_with_definition_list", [{"t": "DefinitionList", "c": [[[S("t")], [[{"t": "DefinitionList", "c": [[[S("u")], [[Pl(S("d"))]]]]}]]]]}], "latex", []),
    ("unnumbered_heading_note_in_superscript", [{"t": "Header", "c": [1, A("h", ["unnumbered"]), [{"t": "Superscript", "c": [S("x"), {"t": "Note", "c": [Pa(S("n"))]}]}, SP, S("y")]]}], "latex", []),
    ("listings_label_with_space", [CodeBlock("x", A("weird id_1"))], "latex", ["--listings"]),
    ("natbib_without_citations", [Pa({"t": "Cite", "c": [[], [S("x")]]})], "latex", ["--natbib"]),
    ("beamer_minimum_heading_level", [H(1, "A"), Pa(S("x")), H(-9223372036854775808, "B"), Pa(S("y"))], "beamer", []),
    ("seven_nested_ordered_lists", None, "latex", []),
    # Random trees, round 2.
    ("figure_with_two_paragraphs", [{"t": "Figure", "c": [A(), [None, [Pl(S("c"))]], [Pa(S("x")), Pa(S("y"))]]}], "latex", []),
    ("figure_with_a_table_and_more", [{"t": "Figure", "c": [A("f"), [None, []], [{"t": "HorizontalRule"}, table([spec()], [], [row(cell([Pl(S("x"))]))]), Pa(S("z"))]]}], "latex", []),
    ("heading_with_lang_and_dir", [H(2, "T", A("h", [], [["lang", "de"], ["dir", "rtl"]]))], "latex", []),
    ("figure_in_minipage_cell", [table([spec(width=W(0.5))], [], [row(cell([{"t": "Figure", "c": [A("fid"), [None, [Pl(S("cap"))]], [Pl(S("x"))]]}]))])], "latex", []),
    ("table_with_empty_head_row", [table([], [[A(), []]], [])], "latex", []),
    ("line_break_after_code", [Pa(Code("x"), LB, S("b"))], "latex", []),
    ("line_break_after_html_raw", [{"t": "LineBlock", "c": [[S("a")], [{"t": "RawInline", "c": ["html", "r"]}, LB]]}], "latex", []),
    ("strut_after_a_quote_in_a_cell", [table([spec(width=W(0.5))], [], [row(cell([Pl(S("a"), LB, S("b")), {"t": "BlockQuote", "c": [Pa(S("q"))]}]))])], "latex", []),
    ("term_with_an_internal_link", [{"t": "DefinitionList", "c": [[[{"t": "Link", "c": [A(), [S("l")], ["#x", ""]]}], [[Pl(S("d"))]]], [[{"t": "Math", "c": [{"t": "DisplayMath"}, "x"]}], [[Pl(S("d"))]]]]}], "latex", []),
    # Signatures of the random trees, round 2b.
    ("beamer_code_in_image_description_is_fragile", [Pa(Img(A(), [Code("c")], "a.png"))], "beamer", []),
    ("beamer_quote_with_definition_list_is_incremental", [{"t": "BlockQuote", "c": [{"t": "DefinitionList", "c": [[[S("t")], [[Pl(S("d"))]]]]}]}], "beamer", []),
    ("beamer_note_in_note", [Pa(S("a"), {"t": "Note", "c": [Pa(S("x"), {"t": "Note", "c": [Pa(S("in"))]})]})], "beamer", []),
    ("span_with_identifier_and_mark", [Pa({"t": "Span", "c": [A("s", ["mark"]), [S("a")]]})], "latex", []),
    ("line_break_at_the_start_of_strong", [Pa(S("a"), {"t": "Note", "c": [Pa({"t": "Strong", "c": [SP, LB, S("b")]})]})], "latex", []),
    ("line_break_after_quoted_code", [Pa({"t": "Quoted", "c": [{"t": "SingleQuote"}, [Code("c")]]}, LB, S("b"))], "latex", []),
    ("definition_starts_with_a_loose_list", [{"t": "DefinitionList", "c": [[[S("t")], [[OL(1, "Decimal", "Period", [[Pa(S("a"))]])]]]]}], "latex", []),
    ("display_math_in_a_sized_cell", [table([spec(width=W(0.5))], [], [row(cell([Pl({"t": "Emph", "c": [{"t": "Math", "c": [{"t": "DisplayMath"}, "x"]}]})]))])], "latex", []),
    ("line_break_in_emphasis_in_a_cell", [table([spec(), spec()], [], [row(cell([Pl(S("a"), {"t": "Emph", "c": [S("b"), SP, LB, S("c")]})]), cell([Pl(S("d"))]))])], "latex", []),
    ("table_of_contents_entry_without_links", [H(1, "x", A("h", ["unnumbered"])), {"t": "Header", "c": [1, A("g", ["unnumbered"]), [{"t": "Link", "c": [A(), [S("L")], ["http://x", ""]]}]]}], "latex", []),
    ("beamer_notes_div_has_no_anchor", [Div(A("n", ["notes"]), [Pa(S("word"), SP)])], "beamer", []),
    ("two_notes_in_a_note", [Pa(S("a"), {"t": "Note", "c": [Pa(S("x"), {"t": "Note", "c": [Pa(S("one"))]}, {"t": "Note", "c": [Pa(S("two"))]})]})], "latex", []),
    ("two_notes_in_a_table_cell", [table([spec()], [], [row(cell([Pl(S("a"), {"t": "Note", "c": [Pa(S("one"))]}, {"t": "Note", "c": [Pa(S("two"))]})]))])], "latex", []),
    ("line_break_in_strong_in_a_sized_cell", [table([spec(width=W(0.5))], [row(cell([Pl({"t": "Strong", "c": [S("h"), LB, S("i")]})]))], [row(cell([Pl({"t": "Strong", "c": [S("a"), LB, S("b")]})]))])], "latex", []),
    ("line_break_at_the_end_of_a_cell", [table([spec(), spec()], [], [row(cell([Pl(S("a"), {"t": "Strikeout", "c": [S("b"), SP, LB]})]), cell([Pl(S("c"), SP, LB)]))])], "latex", []),
    ("note_in_a_repeated_table_head", [table([spec()], [row(cell([Pl(S("h"), {"t": "Note", "c": [Pa(S("n"))]})]))], [row(cell([Pl(S("x"))]))], caption=[Pl(S("cap"))])], "latex", []),
    ("internal_link_in_underline", [Pa({"t": "Underline", "c": [{"t": "Link", "c": [A(), [S("t")], ["#x", ""]]}]})], "latex", []),
    ("line_break_in_an_image_description_in_a_cell", [table([spec(), spec()], [], [row(cell([Pl(Img(A(), [LB, S("a")], "a.png"))]), cell([Pl(S("w"))]))])], "latex", []),
    ("heading_takes_the_identifier_of_its_div", [Div(A("d1"), [H(1, "T"), Pa(S("x"))])], "latex", []),
    ("beamer_note_in_an_incremental_list", [H(2, "S"), {"t": "BlockQuote", "c": [{"t": "BulletList", "c": [[Pl(S("a"), {"t": "Note", "c": [Pa(S("n"))]})]]}]}], "beamer", []),
    ("beamer_frame_title_starts_with_a_line_break", [{"t": "Header", "c": [2, A(), [SP, LB, S("x")]]}, Pa(S("y"))], "beamer", []),
    ("short_caption_keeps_images", [table([spec()], [], [row(cell([Pl(S("x"))]))], caption=[Pl(Img(A(), [S("i")], "a.png"), S("c"), {"t": "Note", "c": [Pa(S("n"))]})])], "latex", []),
    # A caption whose note holds a figure keeps its short caption.
    ("caption_note_with_inner_figure", [{"t": "Figure", "c": [A(), [None, [Pl(S("Cap"), {"t": "Note", "c": [Pa(S("t")), {"t": "Figure", "c": [A(), [None, [Pl(S("in"))]], [Pl(S("x"))]]}]})]], [Pl(S("y"))]]}], "latex", []),
    # I-LATEX-09: a div that starts with a heading is the section of that heading in Beamer.
    ("beamer_section_div_attributes", [Div(A("d", ["shrink"], [["label", "L"]]), [H(1, "T", A("hid", ["plain"], [["label", "M"]])), Pa(S("x"))])], "beamer", []),
    ("beamer_section_div_with_a_subsection", [Div(A("d"), [H(1, "T"), H(2, "U"), Pa(S("x"))])], "beamer", []),
    ("beamer_section_div_after_a_frame", [H(1, "T"), Pa(S("a")), Div(A("d"), [H(1, "T"), Pa(S("x"))])], "beamer", []),
    # I-LATEX-09: a line break in a note in a cell makes the cell complex.
    ("line_break_in_a_note_in_a_cell", [table([spec()], [], [row(cell([Pl({"t": "Note", "c": [Pa(LB, S("x"))]})]))])], "latex", []),
    ("line_break_in_a_list_in_a_note_in_a_sized_cell", [table([spec(width=W(0.5))], [], [row(cell([Pl({"t": "Note", "c": [{"t": "BulletList", "c": [[Pl(S("x"), LB, S("y"))]]}]})]))])], "latex", []),
    # I-LATEX-09: code in a note in an image description makes a frame fragile.
    ("beamer_code_in_a_note_in_an_image_description", [H(1, "T"), Pa(Img(A(), [{"t": "Note", "c": [Pa(Img(A(), [Code("c")], "b.png"))]}], "a.png"))], "beamer", []),
    ("beamer_code_block_in_a_note_in_an_image_description", [H(1, "T"), Pa(Img(A(), [{"t": "Note", "c": [CodeBlock("c")]}], "a.png"))], "beamer", []),
    # I-LATEX-09: a div that holds headings and is not a section keeps its anchor and its sections.
    ("beamer_div_with_two_sections", [Div(A("d"), [H(1, "T"), Pa(S("x")), H(1, "U"), Pa(S("y"))]), Pa(S("z"))], "beamer", []),
    ("beamer_div_with_a_paragraph_before_a_section", [Div(A("d"), [Pa(S("a")), H(1, "T"), Pa(S("x"))])], "beamer", []),
    ("beamer_div_after_a_frame", [H(1, "S"), Pa(S("s")), Div(A("d"), [Pa(S("a")), H(1, "T"), Pa(S("x"))])], "beamer", []),
    ("beamer_div_starts_below_the_slide_level", [Div(A("d"), [H(2, "T"), H(1, "U"), Pa(S("x"))])], "beamer", []),
    ("beamer_blocks_after_a_div_in_a_section", [H(1, "S"), H(2, "R"), Pa(S("s")), Div(A("d"), [H(2, "T"), Pa(S("x")), H(2, "U"), Pa(S("y"))]), Pa(S("z"))], "beamer", []),
    ("beamer_rule_in_a_div_is_a_block", [Div(A("d"), [H(1, "T"), Pa(S("x")), {"t": "HorizontalRule"}, Pa(S("w")), H(1, "U"), Pa(S("y"))])], "beamer", []),
    # I-LATEX-09: the slide level looks at each list of blocks; a section div ends with the div.
    ("beamer_heading_at_the_end_of_a_div", [Div(A(), [Pa(S("a")), H(2, "B")]), Pa(S("y")), H(3, "C"), Pa(S("z"))], "beamer", []),
    ("beamer_heading_in_a_block_quote", [{"t": "BlockQuote", "c": [H(1, "A"), Pa(S("x"))]}, H(2, "B"), Pa(S("y"))], "beamer", []),
    ("beamer_blocks_after_a_section_div", [Div(A("d"), [H(1, "T"), Pa(S("x"))]), Pa(S("z"))], "beamer", []),
    ("beamer_code_after_a_section_div", [Div(A(), [H(2, "B")]), CodeBlock("c")], "beamer", []),
    # I-LATEX-09: struts after a leading line break and at the start of a caption.
    ("line_break_after_a_leading_line_break", [Pa(LB, SP, LB, S("x"))], "latex", []),
    ("line_break_after_code_in_a_figure_caption", [{"t": "Figure", "c": [A(), [None, [Pl(Code("c"), LB, S("y"))]], [Pl(S("i"))]]}], "latex", []),
    # I-LATEX-09: in the first definition, a list that starts a list of blocks follows \hfill.
    ("list_in_a_list_in_a_definition", [{"t": "DefinitionList", "c": [[[S("t")], [[{"t": "BulletList", "c": [[{"t": "BulletList", "c": [[Pl(S("x"))]]}]]}]]]]}], "latex", []),
    ("list_in_a_quote_in_a_definition", [{"t": "DefinitionList", "c": [[[S("t")], [[{"t": "BlockQuote", "c": [{"t": "BulletList", "c": [[Pl(S("x"))]]}]}]]]]}], "latex", []),
    ("list_in_a_second_definition", [{"t": "DefinitionList", "c": [[[S("t")], [[Pa(S("p"))], [{"t": "BulletList", "c": [[Pl(S("x"))]]}]]]]}], "latex", []),
    ("list_after_a_paragraph_in_a_div_in_a_definition", [{"t": "DefinitionList", "c": [[[S("t")], [[Div(A(), [Pa(S("p")), {"t": "BulletList", "c": [[{"t": "BulletList", "c": [[Pl(S("x"))]]}]]}])]]]]}], "latex", []),
    ("lists_in_later_items_in_a_definition", [{"t": "DefinitionList", "c": [[[S("t")], [[{"t": "BulletList", "c": [[Pl(S("x"))], [Pl(S("x")), {"t": "BulletList", "c": [[Pl(S("x"))]]}], [{"t": "BulletList", "c": [[Pl(S("x"))]]}]]}]]]]}], "latex", []),
    ("list_after_a_div_in_a_definition", [{"t": "DefinitionList", "c": [[[S("t")], [[Div(A(), [{"t": "LineBlock", "c": [[S("l")]]}, Pa(S("p"))]), OL(1, "Decimal", "Period", [[Pl(S("x"))]])]]]]}], "latex", []),
    # Session 4, review round 2. Opus B5: the class onlytextwidth of a columns div.
    ("beamer_columns_only_text_width", [H(2, "S"), Div(A("", ["columns", "onlytextwidth"], [["totalwidth", "5cm"]]), [Div(A("", ["column"]), [Pa(S("a"))])])], "beamer", []),
    # Opus B6 and B7: the bibliography forms.
    ("empty_refs_div", [Pa(S("x")), Div(A("refs"), [])], "latex", []),
    ("empty_refs_div_with_natbib", [Pa(S("x")), Div(A("refs"), [])], "latex", ["--natbib"]),
    ("full_refs_div_with_biblatex", [Pa(S("x")), Div(A("refs"), [Pa(S("y"))])], "latex", ["--biblatex"]),
    ("last_empty_section_with_natbib", [Pa(S("a")), H(1, "References")], "latex", ["--natbib"]),
    ("last_section_with_refs_with_biblatex", [Pa(S("a")), H(1, "Other"), Div(A("refs"), [])], "beamer", ["--biblatex"]),
    ("refs_div_between_sections_with_natbib", [H(1, "Refs"), Div(A("refs"), []), H(1, "After"), Pa(S("y"))], "latex", ["--natbib"]),
    ("last_subsection_stays_with_natbib", [Pa(S("a")), H(2, "R")], "latex", ["--natbib"]),
    ("nested_refs_div_stays_with_natbib", [Div(A(), [Div(A("refs"), [])])], "latex", ["--natbib"]),
    # Opus B8, Codex R2-01 and R2-02: inline code with listings.
    ("listings_code_in_a_heading", [{"t": "Header", "c": [1, A("h"), [S("The"), SP, Code("c")]]}], "latex", ["--listings"]),
    ("listings_code_in_a_term", [{"t": "DefinitionList", "c": [[[Code("-v")], [[Pl(S("x"))]]]]}], "latex", ["--listings"]),
    ("listings_code_in_a_caption", [{"t": "Figure", "c": [A(), [None, [Pl(Code("cap"))]], [Pl(Img(A(), [], "a.png"))]]}], "latex", ["--listings"]),
    # Opus B10: special characters in an image path.
    ("image_path_with_brackets", [Pa(Img(A(), [S("alt")], "a[1]^`{|}.png"))], "latex", []),
    # Opus B11 and Codex R2-06: identifiers with special characters.
    ("identifiers_with_special_characters", [Pa({"t": "Span", "c": [A("a[b%c#d\\e"), [S("x")]]}, {"t": "Link", "c": [A(), [S("y")], ["#a%5Bb%c#d\\e", ""]]})], "latex", []),
    # Codex R2-03: the language and direction of a Beamer section div.
    ("beamer_language_of_a_section_div", [Div(A("", [], [["lang", "fr"]]), [H(1, "x"), Pa(S("x"))])], "beamer", []),
    ("beamer_direction_of_a_slide_div", [Div(A("", [], [["lang", "fr"], ["dir", "rtl"]]), [H(2, "x"), Pa(S("x"))])], "beamer", []),
    ("beamer_language_of_a_section_above_slides", [Div(A("", [], [["lang", "fr"]]), [H(1, "S"), H(2, "x"), Pa(S("x"))])], "beamer", []),
    ("beamer_language_of_a_block", [H(1, "S"), Div(A("", [], [["lang", "fr"]]), [H(3, "x"), Pa(S("x"))])], "beamer", []),
    # Codex R2-04: blocks in captions.
    ("raw_block_in_a_caption", [{"t": "Figure", "c": [A(), [None, [{"t": "RawBlock", "c": ["latex", "x"]}]], []]}], "latex", []),
    ("blocks_in_a_caption", [{"t": "Figure", "c": [A(), [None, [Pa(S("a")), CodeBlock("c d"), {"t": "HorizontalRule"}, {"t": "BulletList", "c": [[Pl(S("b"))], [Pl(S("c"))]]}, {"t": "DefinitionList", "c": [[[S("t")], [[Pl(S("d"))]]]]}, {"t": "LineBlock", "c": [[S("l")], [S("m")]]}, table([spec(), spec()], [row(cell([Pl(S("h1"))]), cell([Pl(S("h2"))]))], [row(cell([Pl(S("b1"))]), cell([Pl(S("b2"))]))])]], [Pl(S("i"))]]}], "latex", []),
    # Codex R2-05: a note that is not one paragraph leaves the emphasis.
    ("code_note_in_emphasis", [Pa({"t": "Emph", "c": [{"t": "Note", "c": [CodeBlock("x")]}]})], "latex", []),
    ("list_note_in_strong_in_emphasis", [Pa({"t": "Emph", "c": [S("a"), {"t": "Strong", "c": [S("b"), {"t": "Note", "c": [{"t": "BulletList", "c": [[Pa(S("l"))]]}]}, S("c")]}, S("d")]})], "latex", []),
    ("code_note_in_strikeout", [Pa({"t": "Strikeout", "c": [S("a"), {"t": "Note", "c": [Pa(S("p")), CodeBlock("x")]}]})], "latex", []),
    ("paragraph_note_stays_in_emphasis", [Pa({"t": "Emph", "c": [S("a"), {"t": "Note", "c": [Pa(S("x"))]}]}), Pa({"t": "SmallCaps", "c": [S("a"), {"t": "Note", "c": [CodeBlock("x")]}]})], "latex", []),
    # Codex R2-07 and I-LATEX-03: citation suffixes with biblatex.
    ("biblatex_suffix_of_spaces", [Pa(Cite([SP, SP, S("x")]))], "latex", ["--biblatex"]),
    ("biblatex_page_label_after_spaces", [Pa(Cite([SP, SP, S("p."), SP, SP, S("4")]))], "latex", ["--biblatex"]),
    ("natbib_suffix_with_a_brace_after_the_comma", [Pa(Cite([S(",{}"), SP, S("x")]))], "latex", ["--natbib"]),
    ("biblatex_emphasized_locators", [Pa(Cite([S(","), SP, {"t": "Emph", "c": [S("4")]}, S(","), SP, {"t": "Strong", "c": [{"t": "Emph", "c": [S("p. 5")]}]}, SP, {"t": "Emph", "c": [S("x")]}]))], "latex", ["--biblatex"]),
    ("biblatex_emphasis_without_a_number", [Pa(Cite([S(","), SP, {"t": "Emph", "c": [S("iv")]}]))], "latex", ["--biblatex"]),
    # Codex I-LATEX-10: the end of the verbatim environment of a note.
    ("end_of_verbatim_in_a_note", [Pa({"t": "Note", "c": [CodeBlock("\\end{Verbatim}")]}), Pa({"t": "Note", "c": [CodeBlock("\\end{verbatim}")]})], "latex", []),
    # Codex I-LATEX-09.2 and 09.4, Opus B1 to B4: headings in Beamer divs.
    ("beamer_notes_div_with_a_heading", [Div(A("", ["notes"]), [Pa(S("x")), H(1, "x")])], "beamer", []),
    ("beamer_nested_divs_with_a_section", [Div(A(), [Div(A(), [H(1, "x"), Pa(S("x"))])])], "beamer", []),
    ("beamer_heading_in_a_column", [H(2, "Slide"), Div(A("", ["columns"]), [Div(A("", ["column"]), [H(3, "Left"), Pa(S("text"))])])], "beamer", []),
    ("beamer_slide_in_a_column", [H(2, "Slide"), Div(A("", ["columns"]), [Div(A("", ["column"]), [H(2, "Left"), Pa(S("text"))])])], "beamer", []),
    ("beamer_section_div_with_two_identifiers", [Div(A("sec-a", ["c"]), [H(1, "Div section", A("div-section")), Pa(S("Text."))])], "beamer", ["--slide-level=2"]),
    ("beamer_div_of_two_slides_under_a_section", [H(1, "A"), Div(A("d1"), [H(2, "B"), Pa(S("x")), H(2, "C"), Pa(S("y"))]), Div(A("d2"), [Pa(S("Text."))])], "beamer", ["--slide-level=2"]),
    ("beamer_notes_div_with_a_slide_heading", [H(2, "S"), Pa(S("x")), Div(A("", ["notes"]), [H(2, "N"), Pa(S("text"))])], "beamer", ["--slide-level=3"]),
    # Codex I-LATEX-05: five more Babel names.
    ("babel_names_of_five_languages", [Pa(*[{"t": "Span", "c": [A("", [], [["lang", tag]]), [S("x")]]} for tag in ("as", "gu", "ie", "or", "ti")])], "latex", []),
]

def nested_lists():
    item = [Pl(S("x"))]
    for _ in range(7):
        item = [Pl(S("x")), OL(1, "LowerAlpha", "OneParen", [item])]
    return item[1:]

FLAGS = {
    "--listings": "options.listings = true;",
    "--columns=1": "options.columns = 1;",
    "--table-caption-position=below": "options.table_caption_position = CaptionPosition::CaptionBelow;",
    "--syntax-highlighting=none": "options.highlight_style = None;",
    "--slide-level=2": "options.slide_level = Some(2);",
    "--slide-level=3": "options.slide_level = Some(3);",
    "--top-level-division=part": "options.top_level_division = TopLevelDivision::TopLevelPart;",
    "--top-level-division=chapter": "options.top_level_division = TopLevelDivision::TopLevelChapter;",
    "--natbib": "options.cite_method = CiteMethod::Natbib;",
    "--biblatex": "options.cite_method = CiteMethod::Biblatex;",
}

def main():
    out = sys.stdout
    out.write("//! Review findings as tests. Generated by `tools/regressions.py` from oracle\n")
    out.write("//! output. Do not edit by hand.\n\n")
    out.write("#![allow(clippy::unwrap_used)]\n\n")
    out.write("use randoc_ast::json::from_json;\n")
    out.write("use randoc_core::{\n    CaptionPosition, CiteMethod, Context, FormatSpec, TopLevelDivision, WriterOptions,\n};\n\n")
    out.write("fn render(input: &str, format: &str, set: impl FnOnce(&mut WriterOptions)) -> String {\n")
    out.write("    let doc = from_json(input).unwrap();\n")
    out.write("    let spec = FormatSpec::parse(format);\n")
    out.write("    let mut options = WriterOptions::default();\n")
    out.write("    options.extensions = spec.extensions().unwrap();\n")
    out.write("    set(&mut options);\n")
    out.write("    let mut context = Context::pure();\n")
    out.write("    let text = if spec.name == \"beamer\" {\n")
    out.write("        randoc_latex::write_beamer(&doc, &options, &mut context)\n")
    out.write("    } else {\n")
    out.write("        randoc_latex::write_latex(&doc, &options, &mut context)\n")
    out.write("    };\n")
    out.write("    text.unwrap()\n}\n")
    for name, blocks, fmt, flags in CASES:
        if blocks is None:
            blocks = nested_lists()
        doc = json.dumps({"pandoc-api-version": [1, 23, 1, 2], "meta": {}, "blocks": blocks})
        result = subprocess.run([ORACLE, "-f", "json", "-t", fmt, *flags], input=doc,
                                capture_output=True, text=True, check=True, timeout=60)
        expected = result.stdout[:-1] if result.stdout.endswith("\n") else result.stdout
        setter = " ".join(FLAGS[flag] for flag in flags)
        out.write(f"\n#[test]\nfn {name}() {{\n")
        out.write(f"    let input = r###\"{doc}\"###;\n")
        out.write(f"    let expected = r###\"{expected}\"###;\n")
        out.write(f"    assert_eq!(render(input, \"{fmt}\", |options| {{ let _ = &options; {setter} }}), expected);\n}}\n")

main()
