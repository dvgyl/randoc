#!/usr/bin/env python3
"""Compares table heads, feet and bodies with the oracle: each combination of
empty or full cells, alignment, row span, column span, attributes, column
count, column widths, row count and missing bodies, in `html` and `html4`.

Usage: python3 tools/tables.py
"""
import itertools, json, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from differential import compare, doc_of, A, S
cases=[]
for part in ["head","foot","body"]:
    for content, align, rs, cs, attr, ncols, colw, nrows in itertools.product([[], [{"t":"Plain","c":[S("x")]}]], ["AlignDefault","AlignCenter"], [1,2], [1,2], [A, ["i",[],[]], ["",[],[["k","v"]]]], [1,2], [False, True], [1,2]):
        cell=[attr,{"t":align},rs,cs,content]
        rows=[[A,[cell]]] + ([[A,[]]] if nrows==2 else [])
        specs=[[{"t":"AlignDefault"},{"t":"ColWidth","c":0.5} if colw else {"t":"ColWidthDefault"}]]*ncols
        body=[[A,0,[],[[A,[[A,{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[S("b")]}]]]]]]]
        for nobody in [False, True]:
            head = rows if part=="head" else []
            foot = rows if part=="foot" else []
            bodies = [[A,0,[],rows]] if part=="body" else ([] if nobody else body)
            t={"t":"Table","c":[A,[None,[]],specs,[A,head],bodies,[A,foot]]}
            for fmt in ["html","html4"]:
                cases.append((f"{part}-{content!=[]}-{align}-{rs}-{cs}-{attr}-{ncols}-{colw}-{nrows}-{nobody}", doc_of([t]), fmt, {}))
diffs=compare(cases)
print(len(diffs), len(cases))
for (name,doc,fmt,opts),m,o in diffs[:4]:
    print(name, fmt); print("R",repr(m["out"])); print("O",repr(o["out"]))
