#!/usr/bin/env python3
"""Prints the table of `listings` language names for `blocks.rs`.

Usage: RANDOC_ORACLE_PANDOC=... python3 tools/listings.py

The script asks the oracle (`--listings`) about each language name of the
`listings` package and each name of `pandoc --list-highlight-languages`, in
lower case, as the class of a code block. It prints each class that gets a
`language=` option, with that option.
"""
import json
import os
import re
import subprocess

ORACLE = os.environ["RANDOC_ORACLE_PANDOC"]

# The language names of the `listings` package (its documentation, section
# "Predefined languages"), and some common short names.
LISTINGS = """ABAP ACM ACMscript ACSL Ada Algol Ant Assembler Awk bash Basic C C++ Caml CIL
Clean Cobol Comal command.com Comsol csh Delphi Eiffel Elan elisp erlang Euphoria
Fortran GAP GCL Gnuplot Go hansl Haskell HTML IDL inform Java JVMIS ksh Lingo Lisp
LLVM Logo Lua make Mathematica Matlab Mercury MetaPost Miranda Mizar ML Modula-2
MuPAD NASTRAN Oberon-2 OCL Octave OORexx Oz Pascal Perl PHP PL/I Plasm PostScript
POV Prolog Promela PSTricks Python R Reduce Rexx RSL Ruby S SAS Scala Scilab sh
SHELXL Simula SPARQL SQL Swift tcl TeX VBScript Verilog VHDL VRML XML XSLT
fortran90 fortran95 fortran77 lisp delphi assembler objectivec cs cpp c++ tex sh
""".split()


def main():
    highlight = subprocess.run([ORACLE, "--list-highlight-languages"], capture_output=True,
                               text=True, check=True).stdout.split()
    names = sorted({name.lower() for name in LISTINGS + highlight})
    blocks = [{"t": "CodeBlock", "c": [["", [name], []], "x"]} for name in names]
    doc = json.dumps({"pandoc-api-version": [1, 23, 1, 2], "meta": {}, "blocks": blocks})
    out = subprocess.run([ORACLE, "-f", "json", "-t", "latex", "--listings"], input=doc,
                         capture_output=True, text=True, check=True).stdout
    found = re.findall(r"\\begin\{lstlisting\}(\[language=(.*?)\])?", out)
    assert len(found) == len(names), (len(found), len(names))
    for name, (_, language) in zip(names, found):
        if language:
            print(f'    ("{name}", "{language}"),')


main()
