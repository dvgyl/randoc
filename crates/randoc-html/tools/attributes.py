#!/usr/bin/env python3
"""Asks the Pandoc oracle how the HTML writer treats each attribute name and
generates `src/attributes.rs`.

Method: the candidate names below come from the attribute index and the event
handler list of the WHATWG HTML standard, the ARIA attributes, RDFa, microdata
and obsolete HTML4 attributes. The author wrote the list; nothing comes from
the Pandoc source. For each name the script writes one element of each kind
(span, div, heading, link, image, inline code, table, row, cell) with the
attribute `NAME="v"`, converts it with `pandoc -f json -t html` and `-t html4`,
and records the name that the output has: NAME, data-NAME or nothing.

Usage: RANDOC_ORACLE_PANDOC=... python3 tools/attributes.py
The script writes `src/attributes.rs`. Run it again when the oracle changes.

Result for Pandoc 3.13: the form of a name is the same for each element kind,
except headings (HTML4 keeps only a few names on a heading; `role` and
`epub:type` move to the section) and the `width` and `height` of images.
"""
import json, os, re, subprocess, sys

HTML = """abbr accept accept-charset accesskey action allow allowfullscreen alpha alt as async
autocapitalize autocomplete autocorrect autofocus autoplay blocking charset checked cite class
closedby color colorspace cols colspan command commandfor content contenteditable controls coords
crossorigin data datetime decoding default defer dir dirname disabled download draggable enctype
enterkeyhint fetchpriority for form formaction formenctype formmethod formnovalidate formtarget
headers height hidden high href hreflang http-equiv imagesizes imagesrcset inert inputmode
integrity is ismap itemid itemprop itemref itemscope itemtype kind label lang list loading loop low
max maxlength media method min minlength multiple muted name nomodule nonce novalidate open optimum
pattern ping placeholder playsinline popover popovertarget popovertargetaction poster preload
readonly referrerpolicy rel required reversed rows rowspan sandbox scope selected
shadowrootclonable shadowrootdelegatesfocus shadowrootmode shadowrootserializable shape size sizes
slot span spellcheck src srcdoc srclang srcset start step style tabindex target title translate
type usemap value width wrap writingsuggestions id"""
EVENTS = """onabort onafterprint onauxclick onbeforeinput onbeforematch onbeforeprint onbeforetoggle
onbeforeunload onblur oncancel oncanplay oncanplaythrough onchange onclick onclose oncommand
oncontextlost oncontextmenu oncontextrestored oncopy oncuechange oncut ondblclick ondrag ondragend
ondragenter ondragexit ondragleave ondragover ondragstart ondrop ondurationchange onemptied onended
onerror onfocus onformdata onhashchange oninput oninvalid onkeydown onkeypress onkeyup
onlanguagechange onload onloadeddata onloadedmetadata onloadend onloadstart onmessage
onmessageerror onmousedown onmouseenter onmouseleave onmousemove onmouseout onmouseover onmouseup
onmousewheel onoffline ononline onpagehide onpagereveal onpageshow onpageswap onpaste onpause
onplay onplaying onpopstate onprogress onratechange onrejectionhandled onreset onresize onscroll
onscrollend onsecuritypolicyviolation onseeked onseeking onselect onshow onslotchange onsort
onstalled onstorage onsubmit onsuspend ontimeupdate ontoggle onunhandledrejection onunload
onvolumechange onwaiting onwheel"""
OTHER = """role aria-label aria-hidden about datatype inlist prefix property resource rev typeof vocab
align alink archive axis background bgcolor border cellpadding cellspacing char charoff classid
clear codebase codetype compact declare face frame frameborder hspace language link longdesc
marginheight marginwidth noresize noshade nowrap profile rules scheme scrolling standby summary
text valign valuetype version vlink vspace manifest typemustmatch updateviacache workertype
allowpaymentrequest allowusermedia xmlns xml:lang epub:type data-x x-foo foo number"""
NAMES = sorted(set((HTML + " " + EVENTS + " " + OTHER).split()))

P = os.environ["RANDOC_ORACLE_PANDOC"]
A = ["", [], []]
S = lambda t: {"t": "Str", "c": t}


def element(kind, key, index):
    kv = [[key, "v"]]
    mark = f"k{index}"
    if kind == "span":
        return {"t": "Para", "c": [{"t": "Span", "c": [["", [], kv], [S(mark)]]}]}
    if kind == "div":
        return {"t": "Div", "c": [["", [], kv], [{"t": "Para", "c": [S(mark)]}]]}
    if kind == "heading":
        return {"t": "Header", "c": [2, [mark, [], kv], [S(mark)]]}
    if kind == "link":
        return {"t": "Para", "c": [{"t": "Link", "c": [["", [], kv], [S(mark)], ["u", ""]]}]}
    if kind == "image":
        return {"t": "Para", "c": [{"t": "Image", "c": [["", [], kv], [S(mark)], ["u", ""]]}]}
    if kind == "code":
        return {"t": "Para", "c": [{"t": "Code", "c": [["", [], kv], mark]}]}
    attr = ["", [], kv]
    cell = [attr if kind == "cell" else A, {"t": "AlignDefault"}, 1, 1, [{"t": "Plain", "c": [S(mark)]}]]
    row = [attr if kind == "row" else A, [cell]]
    return {"t": "Table", "c": [attr if kind == "table" else A, [None, []],
                                [[{"t": "AlignDefault"}, {"t": "ColWidthDefault"}]], [A, []],
                                [[A, 0, [], [row]]], [A, []]]}


TAG = {"span": "span", "div": "div", "heading": "h2", "link": "a", "image": "img", "code": "code",
       "table": "table", "row": "tr", "cell": "td"}


def probe(kind, fmt):
    """Returns {name: output form} for one element kind and one format."""
    from concurrent.futures import ThreadPoolExecutor
    with ThreadPoolExecutor(2) as pool:
        forms = list(pool.map(lambda i: probe_one(kind, fmt, i), range(len(NAMES))))
    return dict(zip(NAMES, forms))


def probe_one(kind, fmt, i):
    """Returns the output form of name number `i`: plain, data, dropped, or section-..."""
    # One document for each name keeps the parse simple and robust.
    block = element(kind, NAMES[i], i)
    if True:
        doc = json.dumps({"pandoc-api-version": [1, 23, 1, 2], "meta": {}, "blocks": [block]})
        try:
            r = subprocess.run([P, "-f", "json", "-t", fmt, "--wrap=none"], input=doc.encode(),
                               capture_output=True, timeout=20)
            text = r.stdout.decode()
        except subprocess.TimeoutExpired:
            text = None
        key = NAMES[i]
        if text is None:
            return "timeout"
        tag = TAG[kind]
        m = re.search(r"<" + tag + r"(\s[^>]*)?>", text)
        attrs = m.group(1) or "" if m else ""
        # Only a pair with the probe value counts, not the identifier of the element.
        names = re.findall(r'\s([^\s=]+)="v"', attrs)
        section = re.search(r"<(section|div)\b([^>]*)>\s*<h2", text)
        form = "dropped"
        if key in names:
            form = "plain"
        elif "data-" + key in names:
            form = "data"
        if kind == "heading" and section:
            sec_names = re.findall(r'\s([^\s=]+)="v"', section.group(2))
            if key in sec_names or "data-" + key in sec_names:
                form = "section-" + form
        return form


def main():
    kinds = ["span", "div", "heading", "link", "image", "code", "table", "row", "cell"]
    table = {}
    for fmt in ["html", "html4"]:
        for kind in kinds:
            table[(fmt, kind)] = probe(kind, fmt)
            print(fmt, kind, "done", file=sys.stderr)
    generate(table)


def by_rule(name):
    """Names that Pandoc writes without `data-` by a rule, not by the list."""
    return name.startswith("data-") or name.startswith("aria-") or ":" in name


def generate(table):
    span5 = table[("html", "span")]
    heading4 = table[("html4", "heading")]
    heading5 = table[("html", "heading")]
    html5 = sorted(n for n in NAMES if span5[n] == "plain" and not by_rule(n))
    html4_heading = sorted(n for n in NAMES if heading4[n] == "plain")
    section = sorted(n for n in NAMES if heading5[n].startswith("section-"))
    for kind in ["div", "link", "code", "table", "row", "cell"]:
        for fmt in ["html", "html4"]:
            other = table[(fmt, kind)]
            base = table[(fmt, "span")]
            assert other == base, (fmt, kind)
    def rust_list(names):
        return "".join(f'    "{n}",\n' for n in names)
    text = f"""// Generated by `tools/attributes.py` from the output of Pandoc 3.13. Do not edit.
//! The attribute names that the HTML writer treats specially.

/// The names that HTML5 output writes without a `data-` prefix, in sorted
/// order. Names that start with `data-` or `aria-`, and names with a `:`,
/// also have no prefix.
pub(crate) const HTML5_ATTRIBUTES: &[&str] = &[
{rust_list(html5)}];

/// The names that HTML4 output keeps on a heading, in sorted order.
pub(crate) const HTML4_HEADING_ATTRIBUTES: &[&str] = &[
{rust_list(html4_heading)}];

/// The heading names that Pandoc moves to the section of the heading.
pub(crate) const SECTION_ATTRIBUTES: &[&str] = &[
{rust_list(section)}];
"""
    out = os.path.join(os.path.dirname(__file__), "..", "src", "attributes.rs")
    with open(out, "w") as f:
        f.write(text)
    print(len(html5), len(html4_heading), section, file=sys.stderr)


if __name__ == "__main__":
    main()
