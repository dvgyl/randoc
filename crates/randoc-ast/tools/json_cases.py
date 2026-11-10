#!/usr/bin/env python3
"""Writes the JSON conformance case files in conformance/cases/json/.

Usage: json_cases.py WORKTREE_ROOT. Then run `randoc-harness record --filter json/`.
"""
import json, random, struct, sys, os

ROOT = sys.argv[1]
OUT = os.path.join(ROOT, "conformance/cases/json")
V = '"pandoc-api-version":[1,23,1,2]'
A = '["",[],[]]'


def doc(blocks="[]", meta="{}"):
    return '{%s,"meta":%s,"blocks":%s}' % (V, meta, blocks)


def para(*inl):
    return '{"t":"Para","c":[%s]}' % ",".join(inl)


def blk(b):
    return doc("[%s]" % b)


def inl(i):
    return blk(para(i))


def meta(m):
    return doc("[]", m)


def table(colspecs="[]", head='[%s,[]]' % A, bodies="[]", foot='[%s,[]]' % A, caption="[null,[]]"):
    return '{"t":"Table","c":[%s,%s,%s,%s,%s,%s]}' % (A, caption, colspecs, head, bodies, foot)


def widths(values):
    return blk(table("[%s]" % ",".join('[{"t":"AlignDefault"},{"t":"ColWidth","c":%s}]' % v for v in values)))


def tstr(s):
    s = json.dumps(s, ensure_ascii=False).replace("\x7f", "\\u007F")
    return s


def write(name, comment, cases, extra=""):
    ids = set()
    lines = ["# " + comment, 'from = "json"', 'to = ["json"]', extra, ""]
    for case in cases:
        cid = case["id"]
        assert cid not in ids, cid
        assert "\t" not in case.get("input", ""), cid
        ids.add(cid)
        lines.append("[[case]]")
        lines.append("id = %s" % tstr(cid))
        if "cmd" in case:
            lines.append("cmd = [%s]" % ", ".join(tstr(a) for a in case["cmd"]))
            lines.append("[case.files]")
            for k, v in case["files"].items():
                lines.append("%s = %s" % (tstr(k), tstr(v)))
            if "input" in case:
                pass
        if "input" in case:
            lines.insert(len(lines) - (len(case.get("files", {})) + 1 if "cmd" in case else 0), "input = %s" % tstr(case["input"]))
        lines.append("")
    with open(os.path.join(OUT, name), "w") as f:
        f.write("\n".join(lines))
    return len(cases)


total = 0

# Nodes: each node type in valid form.
S = '{"t":"Str","c":"x"}'
nodes = []
inlines = {
    "str": S, "emph": '{"t":"Emph","c":[%s]}' % S, "underline": '{"t":"Underline","c":[%s]}' % S,
    "strong": '{"t":"Strong","c":[%s]}' % S, "strikeout": '{"t":"Strikeout","c":[%s]}' % S,
    "superscript": '{"t":"Superscript","c":[%s]}' % S, "subscript": '{"t":"Subscript","c":[%s]}' % S,
    "smallcaps": '{"t":"SmallCaps","c":[%s]}' % S, "quoted-single": '{"t":"Quoted","c":[{"t":"SingleQuote"},[%s]]}' % S,
    "quoted-double": '{"t":"Quoted","c":[{"t":"DoubleQuote"},[%s]]}' % S,
    "cite": '{"t":"Cite","c":[[{"citationId":"k","citationPrefix":[%s],"citationSuffix":[],"citationMode":{"t":"AuthorInText"},"citationNoteNum":3,"citationHash":7}],[%s]]}' % (S, S),
    "cite-suppress": '{"t":"Cite","c":[[{"citationId":"a","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"SuppressAuthor"},"citationNoteNum":0,"citationHash":0},{"citationId":"b","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":-1,"citationHash":9}],[]]}',
    "cite-key-order": '{"t":"Cite","c":[[{"citationHash":1,"citationNoteNum":2,"citationMode":{"t":"NormalCitation"},"citationSuffix":[],"citationPrefix":[],"citationId":"z"}],[]]}',
    "code": '{"t":"Code","c":[["id",["a","b"],[["k","v"],["k","w"]]],"x = 1"]}',
    "space": '{"t":"Space"}', "softbreak": '{"t":"SoftBreak"}', "linebreak": '{"t":"LineBreak"}',
    "math-inline": '{"t":"Math","c":[{"t":"InlineMath"},"x^2"]}', "math-display": '{"t":"Math","c":[{"t":"DisplayMath"},"\\\\int"]}',
    "rawinline": '{"t":"RawInline","c":["HTML","<br>"]}',
    "link": '{"t":"Link","c":[%s,[%s],["http://e.com/?a=1&b=2","title"]]}' % (A, S),
    "image": '{"t":"Image","c":[["i",["c"],[["width","50%%"]]],[%s],["img.png",""]]}' % S,
    "note": '{"t":"Note","c":[%s]}' % para(S), "span": '{"t":"Span","c":[["s",[],[["data-x","1"]]],[%s]]}' % S,
    "empty-containers": '{"t":"Emph","c":[]},{"t":"Span","c":[%s,[]]},{"t":"Note","c":[]}' % A,
}
for k, v in inlines.items():
    nodes.append({"id": "inline-" + k, "input": inl(v)})
blocks = {
    "plain": '{"t":"Plain","c":[%s]}' % S, "para": para(S), "lineblock": '{"t":"LineBlock","c":[[%s],[],[%s,{"t":"Space"}]]}' % (S, S),
    "codeblock": '{"t":"CodeBlock","c":[["c",["haskell"],[["startFrom","3"]]],"main = do\\n  print 1"]}',
    "rawblock": '{"t":"RawBlock","c":["latex","\\\\begin{x}"]}', "blockquote": '{"t":"BlockQuote","c":[%s,{"t":"BlockQuote","c":[]}]}' % para(S),
    "orderedlist": '{"t":"OrderedList","c":[[1,{"t":"DefaultStyle"},{"t":"DefaultDelim"}],[[%s],[]]]}' % para(S),
    "bulletlist": '{"t":"BulletList","c":[[{"t":"Plain","c":[%s]}],[]]}' % S,
    "definitionlist": '{"t":"DefinitionList","c":[[[%s],[[%s],[]]],[[],[]]]}' % (S, para(S)),
    "header": '{"t":"Header","c":[6,["h",["a"],[]],[%s]]}' % S, "header-zero": '{"t":"Header","c":[0,%s,[]]}' % A,
    "horizontalrule": '{"t":"HorizontalRule"}',
    "figure": '{"t":"Figure","c":[["fig",[],[]],[[%s],[%s]],[%s]]}' % (S, para(S), para(S)),
    "figure-no-short": '{"t":"Figure","c":[%s,[null,[]],[]]}' % A,
    "div": '{"t":"Div","c":[["d",["note"],[["k","v"]]],[%s]]}' % para(S),
    "table-empty": table(),
    "table-full": table('[[{"t":"AlignLeft"},{"t":"ColWidth","c":0.25}],[{"t":"AlignRight"},{"t":"ColWidthDefault"}],[{"t":"AlignCenter"},{"t":"ColWidth","c":0.75}]]',
                        '[["th",[],[]],[[%s,[[%s,{"t":"AlignDefault"},1,2,[%s]]]]]]' % (A, A, '{"t":"Plain","c":[%s]}' % S),
                        '[[["tb",[],[]],1,[[%s,[[%s,{"t":"AlignLeft"},1,1,[]]]]],[[%s,[[%s,{"t":"AlignCenter"},2,1,[%s]]]]]]]' % (A, A, A, A, para(S)),
                        '[["tf",[],[]],[[%s,[]]]]' % A, '[[%s],[%s]]' % (S, para(S))),
}
for style in ["Example", "Decimal", "LowerRoman", "UpperRoman", "LowerAlpha", "UpperAlpha"]:
    for delim in ["Period", "OneParen", "TwoParens"]:
        blocks["ol-%s-%s" % (style.lower(), delim.lower())] = '{"t":"OrderedList","c":[[7,{"t":"%s"},{"t":"%s"}],[]]}' % (style, delim)
for k, v in blocks.items():
    nodes.append({"id": "block-" + k, "input": blk(v)})
metas = {
    "map": '{"m":{"t":"MetaMap","c":{"z":{"t":"MetaString","c":"1"},"a":{"t":"MetaBool","c":false}}}}',
    "list": '{"l":{"t":"MetaList","c":[{"t":"MetaString","c":"a"},{"t":"MetaList","c":[]}]}}',
    "bool": '{"b":{"t":"MetaBool","c":true}}', "string": '{"s":{"t":"MetaString","c":"text"}}',
    "inlines": '{"i":{"t":"MetaInlines","c":[%s]}}' % S, "blocks": '{"b":{"t":"MetaBlocks","c":[%s]}}' % para(S),
    "sorted-keys": '{"b":{"t":"MetaBool","c":true},"B":{"t":"MetaBool","c":true},"a":{"t":"MetaBool","c":true},"é":{"t":"MetaBool","c":true},"ä":{"t":"MetaBool","c":true},"1":{"t":"MetaBool","c":true},"":{"t":"MetaBool","c":true}}',
    "nested-map": '{"m":{"t":"MetaMap","c":{"n":{"t":"MetaMap","c":{"o":{"t":"MetaMap","c":{}}}}}}}',
}
for k, v in metas.items():
    nodes.append({"id": "meta-" + k, "input": meta(v)})
nodes.append({"id": "whitespace-around", "input": "  \n " + doc() + " \n\n  "})
nodes.append({"id": "whitespace-inside", "input": '{ "pandoc-api-version" : [ 1 , 23 ] ,\n "meta" : { } , "blocks" : [ { "t" : "Para" , "c" : [ ] } ] }'})
nodes.append({"id": "extra-keys", "input": '{"x":1,%s,"meta":{},"y":[1,{"z":null}],"blocks":[{"t":"Para","c":[],"q":5}]}' % V})
nodes.append({"id": "version-long", "input": '{"pandoc-api-version":[1,23,1,2,7,8],"meta":{},"blocks":[]}'})
nodes.append({"id": "version-negative-tail", "input": '{"pandoc-api-version":[1,23,-1],"meta":{},"blocks":[]}'})
nodes.append({"id": "version-float-forms", "input": '{"pandoc-api-version":[1,23.0,1e0],"meta":{},"blocks":[]}'})
total += write("nodes.toml", "Each node type of the document model in valid form.", nodes)

# Error classes.
errs = []
E = {}
def err(i, text):
    E[i] = text
for name, v in {
    "block-not-object": blk("5"), "block-array": blk("[]"), "block-null": blk("null"),
    "block-unknown-tag": blk('{"t":"Nonsense"}'), "block-lowercase-tag": blk('{"t":"para","c":[]}'),
    "block-missing-t": blk('{"c":[]}'), "block-t-number": blk('{"t":5,"c":[]}'), "block-t-null": blk('{"t":null,"c":[]}'),
    "block-t-array": blk('{"t":["Para"],"c":[]}'), "block-missing-c": blk('{"t":"Para"}'), "block-c-null": blk('{"t":"Para","c":null}'),
    "block-c-number": blk('{"t":"Para","c":5}'), "block-c-object": blk('{"t":"Para","c":{}}'),
    "header-short": blk('{"t":"Header","c":[1,%s]}' % A), "header-long": blk('{"t":"Header","c":[1,%s,[],4]}' % A),
    "header-c-number": blk('{"t":"Header","c":5}'), "header-c-object": blk('{"t":"Header","c":{}}'), "header-c-null": blk('{"t":"Header","c":null}'),
    "header-c-string": blk('{"t":"Header","c":"x"}'), "header-missing-c": blk('{"t":"Header"}'),
    "header-level-string": blk('{"t":"Header","c":["x",%s,[]]}' % A), "header-level-null": blk('{"t":"Header","c":[null,%s,[]]}' % A),
    "header-level-fraction": blk('{"t":"Header","c":[1.5,%s,[]]}' % A), "header-level-overflow": blk('{"t":"Header","c":[9223372036854775808,%s,[]]}' % A),
    "codeblock-text-number": blk('{"t":"CodeBlock","c":[%s,5]}' % A), "codeblock-class-number": blk('{"t":"CodeBlock","c":[["",[5],[]],""]}'),
    "codeblock-attr-pair-short": blk('{"t":"CodeBlock","c":[["",[],[["a"]]],""]}'), "codeblock-attr-pair-long": blk('{"t":"CodeBlock","c":[["",[],[["a","b","c"]]],""]}'),
    "codeblock-attr-pair-object": blk('{"t":"CodeBlock","c":[["",[],[{}]],""]}'),
    "codeblock-attr-short": blk('{"t":"CodeBlock","c":[["",[]],""]}'), "codeblock-attr-number": blk('{"t":"CodeBlock","c":[5,""]}'),
    "codeblock-attr-object": blk('{"t":"CodeBlock","c":[{},""]}'), "codeblock-id-number": blk('{"t":"CodeBlock","c":[[1,[],[]],""]}'),
    "codeblock-classes-object": blk('{"t":"CodeBlock","c":[["",{},[]],""]}'),
    "rawblock-format-number": blk('{"t":"RawBlock","c":[5,""]}'),
    "orderedlist-style-unknown": blk('{"t":"OrderedList","c":[[1,{"t":"Foo"},{"t":"Period"}],[]]}'),
    "orderedlist-style-string": blk('{"t":"OrderedList","c":[[1,"Decimal",{"t":"Period"}],[]]}'),
    "orderedlist-delim-unknown": blk('{"t":"OrderedList","c":[[1,{"t":"Decimal"},{"t":"Dot"}],[]]}'),
    "orderedlist-attrs-short": blk('{"t":"OrderedList","c":[[1,{"t":"Decimal"}],[]]}'),
    "orderedlist-attrs-object": blk('{"t":"OrderedList","c":[{},[]]}'), "orderedlist-start-string": blk('{"t":"OrderedList","c":[["1",{"t":"Decimal"},{"t":"Period"}],[]]}'),
    "orderedlist-item-not-list": blk('{"t":"OrderedList","c":[[1,{"t":"Decimal"},{"t":"Period"}],[5]]}'),
    "bulletlist-item-number": blk('{"t":"BulletList","c":[[5]]}'), "bulletlist-items-object": blk('{"t":"BulletList","c":{}}'),
    "lineblock-inline-number": blk('{"t":"LineBlock","c":[[5]]}'), "lineblock-line-string": blk('{"t":"LineBlock","c":["x"]}'),
    "deflist-item-short": blk('{"t":"DefinitionList","c":[[[]]]}'), "deflist-def-number": blk('{"t":"DefinitionList","c":[[[],[5]]]}'),
    "deflist-item-object": blk('{"t":"DefinitionList","c":[{}]}'),
    "figure-caption-number": blk('{"t":"Figure","c":[%s,5,[]]}' % A), "figure-caption-short": blk('{"t":"Figure","c":[%s,[null],[]]}' % A),
    "figure-short-number": blk('{"t":"Figure","c":[%s,[5,[]],[]]}' % A), "figure-short-string": blk('{"t":"Figure","c":[%s,["x",[]],[]]}' % A),
    "div-blocks-number": blk('{"t":"Div","c":[%s,[5]]}' % A),
    "table-head-short": blk(table(head='[%s]' % A)), "table-head-number": blk(table(head="5")),
    "table-body-number": blk(table(bodies="[5]")), "table-body-short": blk(table(bodies='[[%s,0,[]]]' % A)),
    "table-body-rhc-string": blk(table(bodies='[[%s,"x",[],[]]]' % A)), "table-body-rhc-fraction": blk(table(bodies='[[%s,0.5,[],[]]]' % A)),
    "table-row-number": blk(table(head='[%s,[5]]' % A)), "table-row-short": blk(table(head='[%s,[[%s]]]' % (A, A))),
    "table-cell-number": blk(table(head='[%s,[[%s,[5]]]]' % (A, A))), "table-cell-short": blk(table(head='[%s,[[%s,[[%s,{"t":"AlignDefault"},1,1]]]]]' % (A, A, A))),
    "table-cell-span-fraction": blk(table(head='[%s,[[%s,[[%s,{"t":"AlignDefault"},1.5,1,[]]]]]]' % (A, A, A))),
    "table-cell-align-unknown": blk(table(head='[%s,[[%s,[[%s,{"t":"AlignMiddle"},1,1,[]]]]]]' % (A, A, A))),
    "table-foot-object": blk(table(foot="{}")), "table-colspec-number": blk(table("[5]")),
    "table-colspec-short": blk(table('[[{"t":"AlignDefault"}]]')), "table-colspec-long": blk(table('[[{"t":"AlignDefault"},{"t":"ColWidthDefault"},1]]')),
    "table-width-string": widths(['"x"']), "table-width-inf-word": widths(['"inf"']), "table-width-nan-word": widths(['"NaN"']),
    "table-width-true": widths(["true"]), "table-width-array": widths(["[]"]), "table-width-object": widths(["{}"]),
    "table-width-missing-c": blk(table('[[{"t":"AlignDefault"},{"t":"ColWidth"}]]')),
    "table-width-unknown": blk(table('[[{"t":"AlignDefault"},{"t":"ColWidthAuto"}]]')),
    "table-caption-number": blk(table(caption="5")), "table-short": blk('{"t":"Table","c":[%s,[null,[]],[]]}' % A),
    "table-c-object": blk('{"t":"Table","c":{}}'),
    "inline-not-object": inl("5"), "inline-unknown": inl('{"t":"Foo"}'), "inline-block-tag": inl('{"t":"Para","c":[]}'),
    "inline-str-number": inl('{"t":"Str","c":5}'), "inline-str-null": inl('{"t":"Str","c":null}'), "inline-str-missing-c": inl('{"t":"Str"}'),
    "inline-nested-unknown": inl('{"t":"Emph","c":[{"t":"Foo"}]}'), "inline-deep-error": inl('{"t":"Emph","c":[{"t":"Strong","c":[%s,{"t":"Span","c":[%s,[5]]}]}]}' % (S, A)),
    "quoted-type-unknown": inl('{"t":"Quoted","c":[{"t":"X"},[]]}'), "math-type-unknown": inl('{"t":"Math","c":[{"t":"X"},""]}'),
    "math-text-number": inl('{"t":"Math","c":[{"t":"InlineMath"},1]}'),
    "link-target-short": inl('{"t":"Link","c":[%s,[],["u"]]}' % A), "link-target-number": inl('{"t":"Link","c":[%s,[],5]}' % A),
    "image-target-long": inl('{"t":"Image","c":[%s,[],["u","t","x"]]}' % A), "link-short": inl('{"t":"Link","c":[%s,[]]}' % A),
    "note-blocks-object": inl('{"t":"Note","c":{}}'), "code-attr-null": inl('{"t":"Code","c":[null,""]}'),
    "rawinline-text-object": inl('{"t":"RawInline","c":["html",{}]}'),
    "cite-citation-array": inl('{"t":"Cite","c":[[["k",[],[],{"t":"NormalCitation"},1,0]],[]]}'),
    "cite-citation-number": inl('{"t":"Cite","c":[[5],[]]}'), "cite-citations-object": inl('{"t":"Cite","c":[{},[]]}'),
    "cite-missing-id": inl('{"t":"Cite","c":[[{"citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":1,"citationHash":0}],[]]}'),
    "cite-missing-prefix": inl('{"t":"Cite","c":[[{"citationId":"k"}],[]]}'),
    "cite-missing-mode": inl('{"t":"Cite","c":[[{"citationId":"k","citationPrefix":[],"citationSuffix":[],"citationNoteNum":1}],[]]}'),
    "cite-missing-hash": inl('{"t":"Cite","c":[[{"citationId":"k","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":1}],[]]}'),
    "cite-id-number": inl('{"t":"Cite","c":[[{"citationId":5}],[]]}'),
    "cite-prefix-null": inl('{"t":"Cite","c":[[{"citationId":"k","citationPrefix":null,"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":1,"citationHash":0}],[]]}'),
    "cite-prefix-bad-inline": inl('{"t":"Cite","c":[[{"citationId":"k","citationPrefix":[5],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":1,"citationHash":0}],[]]}'),
    "cite-mode-unknown": inl('{"t":"Cite","c":[[{"citationId":"k","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"X"},"citationNoteNum":1,"citationHash":0}],[]]}'),
    "cite-mode-string": inl('{"t":"Cite","c":[[{"citationId":"k","citationPrefix":[],"citationSuffix":[],"citationMode":"NormalCitation","citationNoteNum":1,"citationHash":0}],[]]}'),
    "cite-notenum-fraction": inl('{"t":"Cite","c":[[{"citationId":"k","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":1.5,"citationHash":0}],[]]}'),
    "cite-hash-string": inl('{"t":"Cite","c":[[{"citationId":"k","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":1,"citationHash":"0"}],[]]}'),
    "meta-not-object": doc("[]", "[]"), "meta-null": doc("[]", "null"), "meta-value-number": meta('{"a":5}'),
    "meta-value-unknown": meta('{"a":{"t":"X"}}'), "meta-nested-unknown": meta('{"a":{"t":"MetaMap","c":{"b":{"t":"X"}}}}'),
    "meta-list-number": meta('{"a":{"t":"MetaList","c":[1]}}'), "meta-bool-number": meta('{"a":{"t":"MetaBool","c":1}}'),
    "meta-bool-string": meta('{"a":{"t":"MetaBool","c":"true"}}'), "meta-string-number": meta('{"a":{"t":"MetaString","c":1}}'),
    "meta-map-array": meta('{"a":{"t":"MetaMap","c":[]}}'), "meta-inlines-bad": meta('{"a":{"t":"MetaInlines","c":[{"t":"Para","c":[]}]}}'),
    "meta-missing-c": meta('{"a":{"t":"MetaString"}}'),
    "meta-error-sorted-first": meta('{"b":5,"a":5}'), "meta-before-blocks": doc("[5]", '{"a":5}'),
    "blocks-not-array": doc("{}"), "blocks-null": doc("null"), "blocks-string": doc('"x"'),
    "top-array": "[]", "top-number": "5", "top-null": "null", "top-string": '"x"', "top-true": "true", "top-empty-object": "{}",
    "version-missing": '{"meta":{},"blocks":[]}', "version-null": '{"pandoc-api-version":null,"meta":{},"blocks":[]}',
    "version-old": '{"pandoc-api-version":[1,22],"meta":{},"blocks":[]}', "version-new": '{"pandoc-api-version":[1,24,0],"meta":{},"blocks":[]}',
    "version-major": '{"pandoc-api-version":[2,23],"meta":{},"blocks":[]}', "version-one-part": '{"pandoc-api-version":[1],"meta":{},"blocks":[]}',
    "version-empty": '{"pandoc-api-version":[],"meta":{},"blocks":[]}', "version-string": '{"pandoc-api-version":"1.23","meta":{},"blocks":[]}',
    "version-object": '{"pandoc-api-version":{},"meta":{},"blocks":[]}', "version-item-string": '{"pandoc-api-version":[1,23,"x"],"meta":{},"blocks":[]}',
    "version-item-fraction": '{"pandoc-api-version":[1,23.5],"meta":{},"blocks":[]}', "version-first-fraction": '{"pandoc-api-version":[1.5,23],"meta":{},"blocks":[]}',
    "version-old-bad-item": '{"pandoc-api-version":[1,22,"x"],"meta":{},"blocks":[]}', "version-negative": '{"pandoc-api-version":[-1,23],"meta":{},"blocks":[]}',
    "version-before-content": '{"blocks":[{"t":"Nonsense"}],"meta":{},"pandoc-api-version":[2,0]}',
    "missing-meta": '{%s,"blocks":[]}' % V, "missing-blocks": '{%s,"meta":{}}' % V,
    "missing-meta-bad-blocks": '{%s,"blocks":5}' % V,
}.items():
    errs.append({"id": name, "input": name and v})
total += write("errors.toml", "Each class of error of the JSON reader: wrong type, wrong length, missing key, unknown tag, number out of range, version errors.", errs)

# Numbers.
nums = []
for i, v in enumerate(["1.0", "1e2", "1E0", "10e-1", "-0", "-0.0", "1000.0", "1000e-3", "0.5e1", "1.23e2", "100000000000000000000e-2",
                       "9223372036854775807", "-9223372036854775808", "9223372036854775808", "-9223372036854775809", "1e18", "1e19",
                       "1e1000", "1e-1000", "1.5", "-1.50", "0.1", "0.09", "0.001", "12345678.9", "1234567.5", "12345670.5",
                       "99999999999999999999", "1e99999999999999999999", "1e-99999999999999999999", "0e999", "0.000", "-0e-5",
                       "1e18446744073709551615", "5e18446744073709551615", "1e9223372036854775807", "1e-9223372036854775808",
                       "123456789012345678901234567890e-11", "1.0000000000000000000001", "2e0", "7E+0", "3e-0",
                       "1e9223372036854775808", "10e-9223372036854775808"]):
    nums.append({"id": "int-%02d" % i, "input": blk('{"t":"Header","c":[%s,%s,[]]}' % (v, A))})
dvals = ["null", '"+inf"', '"-inf"', "1e400", "-1e400", "1e309", "-1e309", "1e-400", "-1e-400", "-0.0", "-0", "0", "0e5", "1e23",
         "5e-324", "-5e-324", "2.5e-324", "2.4703282292062328e-324", "1.7976931348623157e308", "1.7976931348623158e308", "1.7976931348623159e308",
         "2.2250738585072014e-308", "2.2250738585072011e-308", "0.1", "0.09999999999999999", "9999999.999999998", "1e7", "9999999",
         "1e18446744073709551615", "123456789012345678901234567890", "0.30000000000000004", "1e22", "1e21", "4.35", "100", "1e-7",
         "12345678.9", "0.5", "0.25e1", "1E-2",
         "10e9223372036854775807", "-100e9223372036854775807", "1.50e9223372036854775807", "100e-9223372036854775808",
         "10.0e-9223372036854775808", "-10.0e-9223372036854775808", "1e-9223372036854775808", "0e9223372036854775807",
         "1.0e-9223372036854775808", "-1.0e-9223372036854775808", "2.50e-9223372036854775807", "1.000e-9223372036854775806",
         "7.0e-9223372036854775808", "0.10e-9223372036854775806"]
for i, v in enumerate(dvals):
    nums.append({"id": "double-%02d" % i, "input": widths([v])})
rng = random.Random(1309)
def rand_double():
    kind = rng.random()
    if kind < 0.4:
        bits = rng.getrandbits(64)
        d = struct.unpack("<d", struct.pack("<Q", bits))[0]
        if d != d or d in (float("inf"), float("-inf")):
            d = 1.0
        return repr(d)
    if kind < 0.7:
        return repr(rng.uniform(0, 1))
    if kind < 0.85:
        return "%de%d" % (rng.randint(1, 10**rng.randint(1, 20)), rng.randint(-330, 310))
    return "%d.%0*de%d" % (rng.randint(0, 9), rng.randint(1, 25), rng.randint(0, 10**20), rng.randint(-30, 30))
for b in range(10):
    nums.append({"id": "double-batch-%d" % b, "input": widths([rand_double() for _ in range(200)])})
nums.append({"id": "int-fields-all", "input": blk(table(bodies='[[%s,-0,[],[[%s,[[%s,{"t":"AlignDefault"},2e0,30e-1,[]]]]]]]' % (A, A, A)))})
total += write("numbers.toml", "Integer and double fields: integral forms, limits, exponent wrap, special values and the digits of show.", nums)

# Strings.
strs = []
svals = {
    "basic-escapes": r"a\b\f\n\r\t\/\\\"", "unicode-escape": r"\u00e9\u00E9\u4e2d", "surrogate-pair": r"\ud83d\ude00", "surrogate-upper": r"\uD83D\uDE00",
    "nul": r"\u0000", "controls": r"\u0001\u001f\u007f\u0080\u009f\u00a0", "separators": r"\u2028\u2029", "noncharacters": r"\uFFFF\uFFFE\uFEFF",
    "raw-unicode": "é中😀", "raw-del": "\x7f", "raw-c1": "\u0085", "html-chars": "<>&'", "lone-high": r"\ud83d", "lone-low": r"\ude00",
    "high-then-text": r"\ud83dx", "high-then-escape": r"\ud83d\u0041", "low-then-high": r"\ude00\ud83d", "two-high": r"\ud83d\ud83d",
    "high-then-n": r"\ud83d\n", "invalid-escape-q": r"\q", "invalid-escape-x": r"\x41", "invalid-escape-U": r"\U0041", "invalid-escape-quote": r"\'",
    "short-u": r"\u12", "bad-hex": r"\u12g4", "escape-space": "\\ ", "escape-a": r"\a", "long-text": "x" * 500, "empty": "",
    "backslash-end": "a\\\\", "slash": "/", "escaped-slash-only": r"\/",
}
for k, v in svals.items():
    strs.append({"id": "str-" + k, "input": inl('{"t":"Str","c":"%s"}' % v)})
strs.append({"id": "raw-control-in-string", "input": inl('{"t":"Str","c":"a\x01b"}')})
strs.append({"id": "raw-newline-in-string", "input": inl('{"t":"Str","c":"a\nb"}')})
strs.append({"id": "escaped-raw-newline", "input": inl('{"t":"Str","c":"a\\\nb"}')})
strs.append({"id": "key-escapes", "input": meta('{"\\u0061\\n":{"t":"MetaBool","c":true},"k\\"q":{"t":"MetaString","c":"\\u00e9"}}')})
strs.append({"id": "key-invalid-escape", "input": meta('{"\\q":{"t":"MetaBool","c":true}}')})
strs.append({"id": "key-lone-surrogate", "input": meta('{"\\ud800":{"t":"MetaBool","c":true}}')})
strs.append({"id": "tag-escaped", "input": inl('{"\\u0074":"\\u0053tr","c":"x"}')})
strs.append({"id": "attr-escapes", "input": inl('{"t":"Code","c":[["i\\"d",["c\\\\"],[["k\\n","v\\u0001"]]],"\\u001b[0m"]}')})
strs.append({"id": "invalid-escape-before-syntax-error", "input": '["\\q" x'})
strs.append({"id": "invalid-escape-in-ignored-key", "input": '{"pandoc-api-version":[1,22],"x":"\\q"}'})
total += write("strings.toml", "String literals: escapes, surrogate pairs, lone surrogates, control characters and invalid escapes.", strs)

# Syntax errors.
syn = []
svals = ["", " ", "\n", "hello", "hel", "hello\n", "hello\n\n", "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz", "{", '{"a"', '{"a":', '{"a":1',
         '{"a":1,', '{"a":1 "b"', '{"a" 1}', "{5:1}", '{"a":1,}', '{"a":1,5}', "[1,]", "[1 2]", "[", "[x", "[1,2", "[true", '{"a":tru}',
         doc("[}"), '"abc', "tru", "nul", "fals", "true x", "-", "-a", "01", "-01", "1.", "1.e5", "1e", "1e+", "1e+a", "1ea", "1e-", "1.a", "1a",
         "-0", "0x", ".5", "+1", "[-]", "[-x]", "[0.]", "[1.5e+]", "[1.5E-x]", "[1e\u00e9]", "[1.\u00e9]", "[1e\x7f]", "[1e\\]", "[1e']",
         '"é', '{"é":é}', "NaN", "Infinity", "-Infinity", '{"a":[1,2,3]]', '{"a":{"b":1}}}', "x\x01\x7f\x05é1", "x\x0eH", "x\\\"'", "\x1b[31m",
         "[1]  \n ", "[1] \x0c", doc() + " x", doc() + "\x0c", doc() + "]", doc() + "}", doc() + doc(), doc() + "\n\n\n", "\x0c" + doc(),
         "  {\"a\" : 1 , }", "[1e5x]", "1E5x", doc("[" * 3 + "]" * 2), "/* c */ " + doc(), "// c\n" + doc(), "'a'", "{'a':1}", '{"a":1;"b":2}',
         "[1,,2]", "[,1]", "{,}", '{"a"::1}', "[TRUE]", "[True]", "[null1]", "[nulll]", "[truex]", "[1 ]x", "\u00a0[]", "\u3000[]",
         "x" * 29, "x" * 31, "é" * 20, "\x00", "[\"\\u0000\"]", "{\"a\":1}\x00"]
for i, v in enumerate(svals):
    syn.append({"id": "syntax-%03d" % i, "input": v})
total += write("syntax.toml", "Syntax errors and text after the value: the messages of the aeson token parser.", syn)

# Keys: duplicates, order, path forms.
keys = []
kvals = {
    "dup-meta-key": '{%s,"meta":{"a":{"t":"MetaBool","c":true},"a":{"t":"MetaBool","c":false}},"blocks":[]}' % V,
    "dup-blocks": '{"pandoc-api-version":[1,23],"meta":{},"blocks":[],"blocks":[{"t":"HorizontalRule"}]}',
    "dup-blocks-bad-second": '{%s,"meta":{},"blocks":[],"blocks":5}' % V,
    "dup-blocks-bad-first": '{%s,"meta":{},"blocks":5,"blocks":[]}' % V,
    "dup-version": '{"pandoc-api-version":[1,23,1,2],"pandoc-api-version":[1,22],"meta":{},"blocks":[]}',
    "dup-version-old-first": '{"pandoc-api-version":[1,22],"pandoc-api-version":[1,23],"meta":{},"blocks":[]}',
    "dup-meta": '{%s,"meta":{"a":{"t":"MetaBool","c":true}},"meta":{},"blocks":[]}' % V,
    "dup-t": blk('{"t":"Para","t":"Plain","c":[]}'), "dup-c": blk('{"t":"Para","c":[],"c":[{"t":"Space"}]}'),
    "dup-c-bad-second": blk('{"t":"Para","c":[],"c":5}'), "dup-t-unknown-second": blk('{"t":"Para","t":"Nope","c":[]}'),
    "dup-citation-id": inl('{"t":"Cite","c":[[{"citationId":"k","citationId":"j","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":1,"citationHash":0,"extra":5}],[]]}'),
    "dup-citation-mode": inl('{"t":"Cite","c":[[{"citationId":"k","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationMode":{"t":"X"},"citationNoteNum":1,"citationHash":0}],[]]}'),
    "dup-metamap-key": meta('{"m":{"t":"MetaMap","c":{"x":{"t":"MetaString","c":"1"},"x":{"t":"MetaString","c":"2"}}}}'),
    "dup-meta-key-bad-second": meta('{"a":{"t":"MetaBool","c":true},"a":5}'),
    "key-order-reversed": '{"blocks":[{"c":[{"c":"x","t":"Str"}],"t":"Para"}],"meta":{},"pandoc-api-version":[1,23,1,2]}',
    "key-order-c-first": blk('{"c":[["",[],[]],"code"],"t":"CodeBlock"}'),
    "path-dot-key": meta('{"a.b":{"t":"MetaBool","c":1}}'), "path-space-key": meta('{"a b":{"t":"MetaBool","c":1}}'),
    "path-quote-key": meta('{"a\'b":{"t":"MetaBool","c":1}}'), "path-backslash-key": meta('{"a\\\\b":{"t":"MetaBool","c":1}}'),
    "path-empty-key": meta('{"":{"t":"MetaBool","c":1}}'), "path-underscore-key": meta('{"_a1":{"t":"MetaBool","c":1}}'),
    "path-digit-key": meta('{"1a":{"t":"MetaBool","c":1}}'), "path-unicode-key": meta('{"é":{"t":"MetaBool","c":1}}'),
    "path-alnum-key": meta('{"abc123":{"t":"MetaBool","c":1}}'), "path-dash-key": meta('{"a-b":{"t":"MetaBool","c":1}}'),
    "path-cjk-key": meta('{"中文":{"t":"MetaBool","c":1}}'),
    "path-letter-number-key": meta('{"\u2163":{"t":"MetaBool","c":1}}'), "path-letter-then-number-key": meta('{"a\u2163":{"t":"MetaBool","c":1}}'),
    "path-fraction-key": meta('{"a\u2150":{"t":"MetaBool","c":1}}'), "path-unicode16-key": meta('{"a\u1c89":{"t":"MetaBool","c":1}}'),
    "path-vowel-sign-key": meta('{"a\u0905\u093e":{"t":"MetaBool","c":1}}'), "path-circled-key": meta('{"\u24d0":{"t":"MetaBool","c":1}}'),
    "path-arabic-digit-key": meta('{"x\u0661":{"t":"MetaBool","c":1}}'), "path-greek-digit-key": meta('{"αβ٣":{"t":"MetaBool","c":1}}'),
    "path-nested-list": meta('{"a":{"t":"MetaList","c":[{"t":"MetaMap","c":{"b c":{"t":"MetaList","c":[{"t":"MetaBool","c":true},5]}}}]}}'),
    "path-deep-blocks": blk('{"t":"BlockQuote","c":[{"t":"Div","c":[%s,[{"t":"BulletList","c":[[{"t":"Plain","c":[{"t":"Note","c":[5]}]}]]}]]}]}' % A),
    "path-table-cell": blk(table(bodies='[[%s,0,[],[[%s,[[%s,{"t":"AlignDefault"},1,1,[5]]]]]]]' % (A, A, A))),
    "path-caption-block": blk(table(caption='[null,[%s,5]]' % para(S))),
    "path-figure-short": blk('{"t":"Figure","c":[%s,[[%s,5],[]],[]]}' % (A, S)),
}
for k, v in kvals.items():
    keys.append({"id": k, "input": v})
total += write("keys.toml", "Equal keys (the first value wins), key order and the forms of the error path.", keys)

# Several input files.
files = []
def fdoc(m, b):
    return '{%s,"meta":{%s},"blocks":[%s]}' % (V, m, b)
F1 = fdoc('"a":{"t":"MetaString","c":"1"},"k":{"t":"MetaString","c":"1"},"m":{"t":"MetaMap","c":{"x":{"t":"MetaBool","c":true}}}', '{"t":"HorizontalRule"}')
F2 = fdoc('"b":{"t":"MetaString","c":"2"},"k":{"t":"MetaString","c":"2"},"m":{"t":"MetaMap","c":{"y":{"t":"MetaBool","c":true}}}', '{"t":"Plain","c":[]}') + "\n"
F3 = fdoc('"k":{"t":"MetaString","c":"3"}', para(S))
BAD = "hello"
OLD = '{"pandoc-api-version":[1,22],"meta":{},"blocks":[]}'
B5 = fdoc("", "5")
TG = fdoc("", "") + " x"
combos = {
    "two": ["one.json", "two.json"], "two-reversed": ["two.json", "one.json"], "three": ["one.json", "two.json", "three.json"],
    "same-twice": ["one.json", "one.json"], "bad-second": ["one.json", "bad.json"], "bad-first": ["bad.json", "one.json"],
    "empty-second": ["one.json", "empty.json"], "empty-only": ["empty.json"], "old-second": ["one.json", "old.json"],
    "block-error-second": ["one.json", "b5.json"], "trailing-second": ["one.json", "tg.json"], "two-bad": ["bad.json", "old.json"],
    "two-bad-reversed": ["old.json", "bad.json"], "concatenated": ["cat.json"], "whitespace-only": ["ws.json", "one.json"],
}
fset = {"one.json": F1, "two.json": F2, "three.json": F3, "bad.json": BAD, "empty.json": "", "old.json": OLD, "b5.json": B5, "tg.json": TG,
        "cat.json": F1 + F3, "ws.json": "  \n\n"}
for k, names in combos.items():
    files.append({"id": "files-" + k, "cmd": ["-f", "json", "-t", "json"] + names, "files": {n: fset[n] for n in sorted(set(names))}})
total += write("files.toml", "Several input files: each file is one document, and the reader joins the documents.", files)

print("cases", total)
