"""Writes tests/replay/citation-{cases.txt,refs.json,locale.xml}: references
x citation layouts x citation forms. Each case line has the Markdown citation
for the oracle, the citation items for the library, the layout attributes and
the layout content. Items are separated by `|`; the fields of an item by `^`:
TYPE (N normal, A author only, S suppress author), ID, LABEL, LOCATOR,
PREFIX, SUFFIX. An empty field is no value.
Usage (from the crate folder): python3 tools/citation_cases.py"""
import json

REFS = [
    {"id": "c1", "type": "book", "title": "Rivers of Glass", "issued": {"date-parts": [[1998]]},
     "author": [{"family": "Ampleforth", "given": "Ada"}], "publisher": "Northwind"},
    {"id": "c2", "type": "article-journal", "title": "On Small Stones", "issued": {"date-parts": [[2004, 5]]},
     "author": [{"family": "Brook", "given": "Bella"}, {"family": "Cole", "given": "Cyrus"}],
     "container-title": "Pebble Review", "volume": "12", "page": "101-119"},
    {"id": "c3", "type": "book", "title": "Three Hands", "issued": {"date-parts": [[2011]]},
     "author": [{"family": "Dunmore", "given": "Della"}, {"family": "Easton", "given": "Eli"},
                {"family": "Fairley", "given": "Fay"}]},
    {"id": "c4", "type": "report", "title": "Annual Count", "issued": {"date-parts": [[2020]]},
     "author": [{"literal": "Harbour Survey Office"}]},
    {"id": "c5", "type": "book", "title": "Collected Letters", "issued": {"date-parts": [[1975]]},
     "editor": [{"family": "Grange", "given": "Gil"}]},
    {"id": "c6", "type": "webpage", "title": "Tide Tables Online"},
    {"id": "c7", "type": "chapter", "title": "A Short Chapter", "issued": {"date-parts": [[2015, 3, 9]]},
     "author": [{"family": "Gaulle", "given": "Charles", "non-dropping-particle": "de"}],
     "container-title": "Big Book", "page": "33"},
    {"id": "c8", "type": "book", "title": "Many Voices", "issued": {"date-parts": [[2001]]},
     "author": [{"family": "Ives", "given": "Ian"}, {"family": "Jones", "given": "Jo"},
                {"family": "Kemp", "given": "Kit"}, {"family": "Lowe", "given": "Lu"},
                {"family": "Marsh", "given": "Max"}]},
    {"id": "c9", "type": "book", "title": "Lower Case", "issued": {"date-parts": [[1990]]},
     "author": [{"family": "hooks", "given": "bell"}]},
    {"id": "c10", "type": "book", "title": "Der Fluss", "issued": {"date-parts": [[1930]]},
     "author": [{"family": "Neumann", "given": "Nina"}], "language": "de-DE"},
    {"id": "c11", "type": "article-newspaper", "title": "Storm Warning", "issued": {"date-parts": [[2022, 11, 2]]},
     "author": [{"family": "Oakes", "given": "Olive"}], "container-title": "Coast Daily"},
    {"id": "c12", "type": "thesis", "title": "Sand Models", "issued": {"date-parts": [[2009]]},
     "author": [{"family": "Pryce", "given": "Pat"}], "publisher": "Dune University", "number-of-pages": "240"},
    {"id": "c13", "type": "book", "title": "No Date Here",
     "author": [{"family": "Quill", "given": "Quinn"}], "translator": [{"family": "Rowe", "given": "Rex"}]},
    {"id": "c14", "type": "book", "title": "Same Again", "issued": {"date-parts": [[1998]]},
     "author": [{"family": "Ampleforth", "given": "Ada"}, {"family": "Stone", "given": "Sam"}]},
    {"id": "c15", "type": "legal_case", "title": "Field v. Meadow", "issued": {"date-parts": [[1961]]},
     "authority": "High Court", "section": "4"},
    {"id": "z1", "type": "book", "title": "Other Item", "issued": {"date-parts": [[1950]]},
     "author": [{"family": "Zeller", "given": "Zoe"}]},
]

LOCALE = ('<locale><terms>'
          '<term name="and">and</term><term name="and" form="symbol">&amp;</term>'
          '<term name="et-al">et al.</term><term name="anonymous">anon.</term>'
          '<term name="no date">n.d.</term><term name="in">in</term>'
          '<term name="page"><single>page</single><multiple>pages</multiple></term>'
          '<term name="page" form="short"><single>p.</single><multiple>pp.</multiple></term>'
          '<term name="chapter"><single>chapter</single><multiple>chapters</multiple></term>'
          '<term name="chapter" form="short"><single>chap.</single><multiple>chaps.</multiple></term>'
          '<term name="section"><single>section</single><multiple>sections</multiple></term>'
          '<term name="section" form="symbol"><single>§</single><multiple>§§</multiple></term>'
          '<term name="editor" form="short"><single>ed.</single><multiple>eds.</multiple></term>'
          '<term name="ibid">ibid.</term>'
          '<term name="chapter" form="symbol"><single>ch§</single><multiple>ch§§</multiple></term>'
          '<term name="translator" form="short"><single>tr.</single><multiple>trs.</multiple></term>'
          '<term name="ordinal">º</term>'
          '<term name="page-range-delimiter">–</term>'
          '</terms></locale>')

AUTHOR = '<names variable="author"><name form="short" and="symbol"/><substitute><names variable="editor"/><text variable="title" font-style="italic"/></substitute></names>'
YEAR = '<date variable="issued"><date-part name="year"/></date>'
NODATE = '<choose><if variable="issued">' + YEAR + '</if><else><text term="no date"/></else></choose>'
LOC = '<group delimiter=" "><label variable="locator" form="short"/><text variable="locator"/></group>'

LAYOUTS = [
    ('prefix="(" suffix=")" delimiter="; "', f'<group delimiter=", ">{AUTHOR}{YEAR}{LOC}</group>'),
    ('prefix="(" suffix=")" delimiter="; "', f'<group delimiter=" ">{AUTHOR}{NODATE}</group><group prefix=", " delimiter=" "><label variable="locator" form="short"/><text variable="locator"/></group>'),
    ('prefix="[" suffix="]" delimiter=", "', f'<group delimiter=": ">{AUTHOR}<text variable="locator"/></group>'),
    ('delimiter="; "', f'{AUTHOR}<text variable="title" prefix=" " font-style="italic"/>'),
    ('prefix="(" suffix=")"', f'<group delimiter=", ">{AUTHOR}{YEAR}<group><label variable="locator" form="long" suffix=" "/><text variable="locator"/></group></group>'),
    ('prefix="(" suffix=")" delimiter="; "', f'<group delimiter=", ">{AUTHOR}{YEAR}<group><label variable="locator" form="symbol" suffix=" "/><text variable="locator"/></group></group>'),
    ('prefix="(" suffix=")" delimiter="; "', f'{AUTHOR}<choose><if variable="locator"><text variable="locator" prefix=" at "/></if><else><text value=" whole"/></else></choose>'),
    ('prefix="(" suffix=")" delimiter="; "', f'{AUTHOR}<choose><if locator="page"><text variable="locator" prefix=" p "/></if><else-if locator="chapter"><text variable="locator" prefix=" ch "/></else-if><else><text variable="locator" prefix=" ?"/></else></choose>'),
    ('prefix="(" suffix=")" delimiter="; "', f'{AUTHOR}<choose><if is-numeric="locator"><text value=" numeric"/></if><else><text value=" other"/></else></choose><text variable="locator" prefix=" "/>'),
    ('prefix="(" suffix=")" delimiter="; "', f'{AUTHOR}<choose><if position="first"><text value=" first"/></if><else><text value=" later"/></else></choose>'),
    ('prefix="(" suffix=")" delimiter="; "', f'<group delimiter=" "><text variable="title" quotes="true"/>{YEAR}</group>'),
    ('prefix="(" suffix=")" delimiter="; "', f'<text variable="citation-number"/>'),
    ('prefix="[" suffix="]" delimiter="," vertical-align="sup"', '<text variable="citation-number"/><text variable="locator" prefix=":"/>'),
    ('suffix="." delimiter="; "', f'{AUTHOR}<group display="block"><text variable="title"/></group>'),
    ('prefix="(" suffix=")." delimiter="; "', f'{AUTHOR}<text variable="title" display="right-inline" prefix=" "/>'),
    ('prefix="(" suffix=")" delimiter="; "', '<names variable="author"><name and="text" et-al-min="3" et-al-use-first="1"/></names><date variable="issued" prefix=" "><date-part name="year"/></date>'),
    ('prefix="(" suffix=")" delimiter="; "', '<names variable="author" font-variant="small-caps"><name form="short"/><label form="short" prefix=" "/></names><text variable="page" prefix=", "/>'),
    ('prefix="(" suffix=")" delimiter="; "', f'<group delimiter=", "><text variable="title" text-case="uppercase"/>{LOC}</group>'),
    ('prefix="(" suffix=")" delimiter="; "', '<group delimiter=", "><names variable="editor translator"><name form="short"/><label form="short" prefix=" (" suffix=")"/></names><text variable="title"/></group>'),
    ('delimiter=" and "', f'<group delimiter=" "><text term="in" text-case="capitalize-first"/>{AUTHOR}</group><text variable="locator" prefix=", "/>'),
    ('prefix="(" suffix=")" delimiter="; "', '<group delimiter=", "><text variable="container-title" font-style="italic"/><text variable="volume"/><text variable="page"/></group>'),
    ('prefix="(" suffix=")" delimiter="; "', f'{AUTHOR}<group prefix=" " delimiter=" "><label variable="locator" form="short" plural="always"/><text variable="locator"/></group>'),
    ('prefix="(" suffix=")" delimiter="; "', f'{AUTHOR}<group prefix=" " delimiter=" "><label variable="locator" form="short" plural="never"/><text variable="locator"/></group>'),
    ('prefix="(" suffix=")" delimiter="; "', '<text variable="id"/><text variable="type" prefix=" "/>'),
    ('prefix="(" suffix=")" delimiter="; "', f'<group delimiter=" "><names variable="author"><name initialize-with=". " name-as-sort-order="first"/></names>{YEAR}</group>'),
    ('prefix="(" suffix=")" delimiter="; "', f'<group delimiter=", " font-weight="bold">{AUTHOR}{YEAR}</group><text variable="locator" prefix=", " font-style="italic"/>'),
    ('prefix="(" suffix=")" delimiter="; "', '<text variable="title" form="short" strip-periods="true"/><text variable="section" prefix=" §"/>'),
    ('prefix="(" suffix=")" delimiter="; "', f'<group delimiter=", ">{AUTHOR}<number variable="number-of-pages" form="ordinal"/></group>'),
    ('prefix="« " suffix=" »" delimiter=" ; "', f'<group delimiter=" ">{AUTHOR}{NODATE}</group>'),
    ('prefix="(" suffix=")" delimiter="; "', f'<group delimiter=", "><text macro="m"/>{YEAR}</group><group prefix=": " delimiter=" "><label variable="locator" form="short"/><text variable="locator"/></group>'),
]

FORMS = [
    ('[@ID]', 'N^ID^^^^'),
    ('@ID', 'A^ID^^^^|S^ID^^^^'),
    ('[-@ID]', 'S^ID^^^^'),
    ('[@ID, p. 33]', 'N^ID^page^33^^'),
    ('[@ID, chap. 2-4]', 'N^ID^chapter^2-4^^'),
    ('[see @ID]', 'N^ID^^^see ^'),
    ('[@ID, with notes]', 'N^ID^^^^, with notes'),
    ('[@ID; @z1]', 'N^ID^^^^|N^z1^^^^'),
    ('@ID [p. 7]', 'A^ID^^^^|S^ID^page^7^^'),
]


def main():
    with open('tests/replay/citation-refs.json', 'w') as f:
        f.write('[\n' + ',\n'.join(json.dumps(r, ensure_ascii=False) for r in REFS) + '\n]\n')
    with open('tests/replay/citation-locale.xml', 'w') as f:
        f.write(LOCALE + '\n')
    with open('tests/replay/citation-cases.txt', 'w') as f:
        for ref in REFS[:15]:
            for attrs, layout in LAYOUTS:
                for form, items in FORMS:
                    i = ref['id']
                    f.write(f"{form.replace('ID', i)}\t{items.replace('ID', i)}\t{attrs}\t{layout}\n")


main()
