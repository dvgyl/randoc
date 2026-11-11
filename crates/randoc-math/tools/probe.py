#!/usr/bin/env python3
"""Record the oracle results for TeX math formulas.

Usage: probe.py generate OUT.tsv      generate the formula set and record it
       probe.py record IN.txt OUT.tsv record the formulas of IN.txt (KIND<TAB>TEX,
                                      with TEX escaped as in the output)
       probe.py table IN.tsv OUT.rs   write the tables of src/symbols.rs from the results
       probe.py check IN.tsv N        compare N batch results with single runs

The formula set holds the oracle matrices (one rule in each formula), each
candidate command name of katex-names.txt (KaTeX, MIT), document formulas,
malformed formulas, random formulas, and the inputs of `findings.txt` (the
review findings and the matrices that found their rules). A second pass records each character
that a command stands for. The tables are: the symbol commands, the characters
of each mathvariant value, the class of each character, the negations of
`\\not`, and the commands that are delimiters.

The oracle is $RANDOC_ORACLE_PANDOC. Source env.sh first: it sets the heap limit.
The tool runs one oracle process at a time, with a time limit of 60 seconds.

Each output line has six fields, separated by tabs:
  kind  tex  mathml  inline  mathml_warning  inline_warning
kind is I (inline math) or D (display math).
mathml is the HTML output of the formula with the MathML method.
inline is the HTML output with the plain method, at 1 column, so that each
Space of the inline list is a line break.
A field escapes the backslash, the tab, the newline and the carriage return
as \\\\, \\t, \\n and \\r.
"""

import json
import os
import random
import re
import subprocess
import sys

BATCH = 800

GREEK = ("alpha beta gamma delta epsilon varepsilon zeta eta theta vartheta iota kappa "
         "lambda mu nu xi pi varpi rho varrho sigma varsigma tau upsilon phi varphi chi psi "
         "omega Gamma Delta Theta Lambda Xi Pi Sigma Upsilon Phi Psi Omega digamma varkappa").split()
SYMBOLS = ("pm mp times div cdot ast star circ bullet cap cup uplus sqcap sqcup vee wedge "
           "setminus wr diamond bigtriangleup bigtriangledown triangleleft triangleright oplus "
           "ominus otimes oslash odot bigcirc dagger ddagger amalg leq le geq ge equiv models prec "
           "succ sim perp preceq succeq simeq mid ll gg asymp parallel subset supset approx bowtie "
           "subseteq supseteq cong sqsubset sqsupset neq ne smile sqsubseteq sqsupseteq doteq frown "
           "in ni notin propto vdash dashv leftarrow gets rightarrow to uparrow downarrow "
           "leftrightarrow updownarrow Leftarrow Rightarrow Uparrow Downarrow Leftrightarrow "
           "Updownarrow mapsto longmapsto nearrow searrow swarrow nwarrow hookleftarrow "
           "hookrightarrow longleftarrow longrightarrow Longleftarrow Longrightarrow "
           "longleftrightarrow Longleftrightarrow iff implies impliedby leftharpoonup "
           "rightharpoonup leftharpoondown rightharpoondown rightleftharpoons aleph hbar imath "
           "jmath ell wp Re Im partial infty prime emptyset varnothing nabla surd top bot angle "
           "triangle backslash forall exists nexists neg lnot flat natural sharp clubsuit "
           "diamondsuit heartsuit spadesuit ldots cdots vdots ddots dots dotsc dotsb colon "
           "lbrace rbrace langle rangle lfloor rfloor lceil rceil vert Vert lvert rvert lVert "
           "rVert land lor coloneqq lessgtr leqslant geqslant nleq ngeq subsetneq supsetneq "
           "therefore because square blacksquare checkmark circledast complement mho eth "
           "lll ggg ldotp cdotp vartriangle intercal").split()
BIGOPS = ("sum prod coprod int iint iiint oint bigcap bigcup bigsqcup bigvee bigwedge bigodot "
          "bigotimes bigoplus biguplus").split()
FUNCS = ("arccos arcsin arctan arg cos cosh cot coth csc deg det dim exp gcd hom inf ker lg lim "
         "liminf limsup ln log max min Pr sec sin sinh sup tan tanh").split()
ACCENTS = ("hat widehat tilde widetilde bar overline underline vec overrightarrow overleftarrow "
           "dot ddot acute grave breve check mathring overbrace underbrace").split()
ONEARG = ("sqrt mathrm mathbf mathit mathsf mathtt mathcal mathbb mathfrak mathscr boldsymbol "
          "bm mathbfit text textrm textbf textit texttt textsf mbox operatorname mathop mathbin "
          "mathrel mathord mathpunct boxed phantom hphantom vphantom displaystyle textstyle "
          "pmb emph mathopen mathclose").split()
TWOARG = ("frac dfrac tfrac binom dbinom tbinom cfrac stackrel overset underset genfrac "
          "xrightarrow xleftarrow color textcolor").split()
SPACES = "! , : ; > quad qquad enspace thinspace medspace thickspace negthinspace".split()
SIZES = "big Big bigg Bigg bigl bigr Bigl Bigr left right middle".split()
ENVS = ("matrix pmatrix bmatrix Bmatrix vmatrix Vmatrix smallmatrix cases aligned align "
        "align* gathered gather array subarray split eqnarray multline equation alignat").split()


ALPHABETS = ("mathrm mathbf mathit mathsf mathtt mathcal mathbb mathfrak mathscr boldsymbol bm "
             "mathbfit pmb mathnormal mathup mathbfsf mathsfit mathbfcal mathbffrak mathsfbf "
             "bold Bbb frak cal rm bf it sf tt").split()
TEXTS = "text textrm textbf textit texttt textsf mbox hbox textnormal textup textsc emph textmd".split()
LOWER_GREEK = ("alpha beta gamma delta epsilon varepsilon zeta eta theta vartheta iota kappa lambda mu "
               "nu xi pi varpi rho varrho sigma varsigma tau upsilon phi varphi chi psi omega").split()
UPPER_GREEK = "Gamma Delta Theta Lambda Xi Pi Sigma Upsilon Phi Psi Omega".split()
MORE_ENVS = ("matrix pmatrix bmatrix Bmatrix vmatrix Vmatrix smallmatrix cases aligned align align* "
             "gathered gather gather* array split eqnarray equation alignedat alignat dcases rcases "
             "subarray CD darray matrix* pmatrix* equation* multline flalign").split()
DELIMS = ["(", ")", "[", "]", "|", "\\|", "\\{", "\\}", "\\langle", "\\rangle", "/", "\\backslash",
          "\\lfloor", "\\rfloor", "\\lceil", "\\rceil", "\\uparrow", "\\downarrow", "\\Uparrow",
          "\\updownarrow", ".", "x", "+", "<", ">", "\\vert", "\\Vert", "\\lvert", "\\rVert",
          "\\lbrace", "\\rbrace", "\\lbrack", "\\rbrack", "\\ulcorner", "\\lgroup", "\\lmoustache"]


def matrices(katex_names):
    """The formulas that show one rule each (the "oracle matrix" of a construct)."""
    out = []

    def add(t, kind="I"):
        out.append((kind, t))

    known = set(GREEK + SYMBOLS + BIGOPS + FUNCS)
    for c in katex_names:
        if c not in known:
            add("\\" + c)
            add("a \\" + c + " b")
            add("\\" + c + "{a}{b}")
        add("\\left\\" + c + " x \\right.")
        add("\\big\\" + c)
    upper = "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
    for a in ALPHABETS:
        for arg in [upper, upper.lower(), "0123456789", "".join("\\" + g + " " for g in LOWER_GREEK),
                    "".join("\\" + g + " " for g in UPPER_GREEK), "x_1^2", "\\sin x", "(a)", "a=b,c",
                    "\\frac{a}{b}", "\\text{a}", "\\hat{x}", "\\sqrt{x}", "\\left(x\\right)",
                    "\\partial\\infty\\nabla\\hbar\\ell\\imath\\sum", "a\\,b", "3.5x", "\\mathrm{x}",
                    "\\mathbf{x}", "\\mathit{x}", "\\mathcal{A}", "a-b", "\\sum_i x", "x'", "a b",
                    "\\begin{matrix}a&b\\end{matrix}", "{a}{b}", "\\operatorname{a}", "a!", "A\\Gamma 1"]:
            add("\\%s{%s}" % (a, arg))
        add("\\%s x y" % a)
        add("\\%s 12" % a)
        add("\\%s \\alpha" % a)
        add("{\\%s x} y" % a)
        add("\\mathrm{\\%s{x}}" % a)
        add("\\mathbf{\\%s{x}}" % a)
        add("x_{\\%s{a}}" % a)
        add("\\%s{x}_1" % a)
        add("\\%s{}" % a)
    for t in TEXTS:
        for arg in ["a", "ab c", "a $x^2$ b", " a ", "a\\ b", "\\textbf{a}", "a_b", "<&>", "a{b}c", "\\$",
                    "\u00e9", "", "a  b", "1", "a-b", "a~b", "``a''", "a--b", "\\alpha", "\\%", "\\{\\}",
                    "\\&", "\\_", "\\#", "$x$", "a $x$", "$x$ b", "$x$$y$", "a\\(x\\)b", "$$",
                    "$\\frac{a}{b}$", "a\\textit{b}c", "\\textbf{a \\textit{b}}", "a'b", "a\"b", "a.b,c",
                    "A B C", "a\nb", "{a}", "\\text{a}", "a\\\\b", "a\\,b", "a^b"]:
            add("\\%s{%s}" % (t, arg))
        for arg in [upper, upper.lower(), "0123456789"]:
            add("\\%s{%s}" % (t, arg))
        add("x \\%s{ if } y" % t)
        add("\\%s a b" % t)
        add("\\%s{a}_1" % t)
        add("x_{\\%s{a b}}" % t)
        add("\\mathbf{\\%s{a}}" % t)
    for e in MORE_ENVS:
        for body in ["a", "a&b", "a\\\\b", "a&b\\\\c&d\\\\", "a&\\\\&d", "", "a+b&=c\\\\d&=e-f",
                     "1&x>0\\\\0&\\text{otherwise}", "\\frac{a}{b}&c", "a \\\\[2pt] b",
                     "a&b\\\\ \\hline c&d", "a&b&c\\\\d", " a & b \\\\ c & d ", "x_1&=1\\\\", "\\\\", "&",
                     "a\\cr b", "-1&+2", "a,&b"]:
            add("\\begin{%s}%s\\end{%s}" % (e, body, e))
        add("x = \\begin{%s}a\\\\b\\end{%s} y" % (e, e))
        add("x = \\begin{%s}a\\\\b\\end{%s} y" % (e, e), "D")
        add("\\left(\\begin{%s}a\\end{%s}\\right)" % (e, e))
        add("\\begin{%s}\\begin{%s}a&b\\end{%s}\\\\c\\end{%s}" % (e, e, e, e))
        add("\\begin{%s}{a}&b\\end{%s}" % (e, e))
        add("\\begin{%s}{2}a&b&c&d\\end{%s}" % (e, e))
        add("\\begin{ %s }a\\end{ %s }" % (e, e))
    for spec in ["c", "lcr", "l|c|r", "|c|", "cc", "@{}l@{}", "p{2cm}", "rl", "", "ll", "r", "c c", "*{2}{c}",
                 "||c||", "x", "lcrlcr", "c|c"]:
        add("\\begin{array}{%s}a&b&c\\\\d&e&f\\end{array}" % spec)
        add("\\begin{array}{%s}a\\end{array}" % spec)
    add("\\begin{array}[t]{c}a\\end{array}")
    add("\\begin{array} {c} a\\end{array}")
    add("\\begin{array}c a\\end{array}")
    for f in "frac dfrac tfrac cfrac binom dbinom tbinom".split():
        for arg in ["{a}{b}", "{a+1}{b}", "12", "{a}{b}c", "{}{}", "{\\frac{a}{b}}{c}", "ab", "{a}b^2", " {a} {b}"]:
            add("\\%s%s" % (f, arg))
            add("\\%s%s" % (f, arg), "D")
        add("x^{\\%s{a}{b}}" % f)
        add("\\%s{a}{b}_1" % f)
    for g in ["{}{}{}{}", "{(}{)}{0pt}{}", "{[}{]}{}{0}", "{}{}{0pt}{1}", "{}{}{1pt}{2}", "{}{}{}{3}",
              "{\\{}{\\}}{0pt}{}", "{|}{|}{2pt}{0}", "{.}{.}{}{}", "{(}{}{}{}", "{}{}{0.4pt}{}", "{}{}{0em}{}"]:
        add("\\genfrac%s{a}{b}" % g)
    for t in ["\\stackrel{a}{b}", "\\stackrel{?}{=}", "\\overset{a}{=}", "\\underset{a}{=}",
              "x \\overset{a}{=} y", "\\overset{a+b}{\\to}", "\\xrightarrow{a}", "\\xrightarrow[b]{a}",
              "\\xleftarrow{a+b}", "\\overset{\\text{def}}{=}", "\\underset{x}{\\operatorname{argmax}}",
              "\\underset{x}{\\max}", "\\overset{a}{b}^c", "\\underset{a}{b}_c", "x \\stackrel{a}{\\to} y",
              "\\overset{a}{+}", "a \\overset{a}{+} b", "\\underset{a}{x} y", "\\stackrel a b",
              "\\xrightarrow{}", "\\xleftarrow[b]{}", "\\xleftrightarrow{a}", "\\xRightarrow{a}",
              "\\xmapsto{a}", "\\xhookrightarrow{a}", "\\overset{a}{\\underset{b}{c}}",
              "\\overline{x+1}", "\\underline{x}", "\\widehat{xy}", "\\overbrace{a+b}^{n} c",
              "\\underbrace{a+b}_{n} c", "\\overbrace{a}", "\\underbrace{a}", "\\overbrace{a}_b",
              "\\overleftrightarrow{ab}", "\\underleftarrow{ab}", "\\underrightarrow{ab}",
              "\\overparen{ab}", "\\underparen{ab}", "\\overbracket{ab}", "\\underbracket{ab}",
              "\\hat{x}_1", "\\hat x^2", "\\hat{x+1}", "\\tilde{\\alpha}", "\\vec{AB}", "\\dot{x}",
              "\\ddot{x}", "\\dddot{x}", "\\ddddot{x}", "\\bar{x}^2", "\\hat{\\mathbf{x}}",
              "\\mathbf{\\hat{x}}", "\\hat{}", "\\hat 1", "\\hat{12}", "\\hat\\alpha", "\\hat{\\hat{x}}",
              "\\widecheck{x}", "\\utilde{x}", "\\Hat{x}", "\\Bar{x}", "\\Vec{x}", "\\Dot{x}", "\\Tilde{x}"]:
        add(t)
        add(t, "D")
    for r in ["=", "\\in", "<", ">", "\\subset", "\\equiv", "\\leq", "\\sim", "\\approx", "\\exists", "a",
              "{=}", "\\to", "\\mid", "\\parallel", "\\cong", "\\simeq", "\\supset", "\\subseteq",
              "\\supseteq", "\\geq", "\\ni", "\\vdash", "\\models", "\\prec", "\\succ", "\\rightarrow",
              "\\leftarrow", "\\Rightarrow", "\\Leftrightarrow", "\\le", "\\ge", "+", "1", "\\alpha",
              "\\asymp", "\\sqsubseteq", "\\preceq", "\\succeq", "\\perp", "|", "\\|", "(", "\\neq", "\\ll"]:
        add("\\not%s" % (r if not r[0].isalpha() else " " + r))
        add("a \\not%s b" % (r if not r[0].isalpha() else " " + r))
    for s in ("big Big bigg Bigg bigl Bigl biggl Biggl bigr Bigr biggr Biggr bigm Bigm biggm Biggm "
              "left right middle").split():
        for d in DELIMS:
            add("\\%s%s" % (s, d))
            add("a \\%s%s b" % (s, d))
        add("\\%s( x \\%s)" % (s, s))
        add("\\%s" % s)
        add("\\%s{(}" % s)
        add("\\%s (" % s)
    for d in DELIMS:
        add("\\left%s x \\right%s" % (d, d))
        add("\\left( a \\middle%s b \\right)" % d)
    for t in ["\\left( a \\middle| b \\middle| c \\right)", "\\left< x \\right>", "\\left( x \\right) ^2",
              "\\left( x \\right)_1^2", "\\left(\\frac{a}{b}\\right)", "\\left(\\right)", "\\left.\\right.",
              "a \\left( b \\right) c", "\\left( a + \\left[ b \\right] \\right)", "\\left( -a \\right)",
              "\\left( a, b \\right)", "\\left( a = b \\right)", "\\left( + \\right)", "\\left\\{ a \\right.",
              "\\middle| x", "\\left( x \\right] y", "\\left(x\\right)\\left(y\\right)", "-\\left(x\\right)",
              "\\sin\\left(x\\right)", "\\left( x", "\\left( x \\right", "f\\left(x\\right)=y"]:
        add(t)
    for c in "mathop mathbin mathrel mathord mathpunct mathopen mathclose mathinner".split():
        for t in ["\\%s{x}", "a \\%s{x} b", "\\%s{+}", "a\\%s{+}b", "\\%s{ab}", "a \\%s{ab} b", "\\%s{x}_a^b",
                  "\\%s{\\mathrm{sup}}_x y", "\\%s{}", "\\%s x", "a \\%s{=} b", "a \\%s{\\alpha} b",
                  "a \\%s{1} b", "a \\%s{\\text{if}} b", "\\%s{x}\\limits_a^b", "a \\%s{(} b",
                  "a \\%s{\\sum} b", "a \\%s{\\mathrm{d}} b", "a \\%s{\\frac{a}{b}} b", "a \\%s{,} b"]:
            add(t % c)
        add("\\%s{x}_a^b" % c, "D")
        add("\\%s{\\mathrm{sup}}_x y" % c, "D")
    for t in ["\\operatorname{a}", "\\operatorname*{a}", "\\operatorname*{argmax}_x f", "\\operatorname{arg\\,max}",
              "\\operatorname{a b}", "\\operatorname{a-b}", "\\operatorname{a}_x^y", "\\operatorname{\\alpha}",
              "\\operatorname{A1}", "\\operatorname x", "\\operatorname{a} x", "\\operatorname*{a} x",
              "\\operatorname{a}(x)", "\\operatorname{}", "\\operatorname{a_b}", "\\operatorname{\\mathrm{a}}",
              "\\operatorname{a+b}", "\\operatorname{a'}", "\\operatorname{sin}", "\\operatorname{a}\\limits_x",
              "\\operatorname*{a}\\nolimits_x", "\\operatorname{a\\ b}", "\\operatorname{a.b}",
              "\\operatorname{\\text{a}}", "\\operatorname{a,b}", "x\\operatorname{a}", "\\operatorname{a}^2 x",
              "\\operatorname{ab}\\operatorname{cd}", "\\operatorname{a*b}", "\\operatorname{a/b}",
              "\\operatorname{a|b}", "\\operatorname{a:b}", "\\operatorname{a(b)}", "\\operatorname{1}",
              "\\sin x", "\\sin(x)", "\\sin", "x \\sin", "\\sin\\cos x", "\\sin^2", "\\sin_a x", "\\sin{x}",
              "2\\sin x", "\\lim x", "\\lim_a", "\\lim_a x", "\\max_a^b x", "\\sup x", "(\\sin)", "\\sin +",
              "\\sin = x", "\\sin, x", "\\sin\\, x", "\\sin\\alpha", "\\sin 2", "\\sin\\frac{a}{b}", "\\sin |x|",
              "\\sin\\left(x\\right)", "\\log_2 n", "\\log_2", "\\arg\\max_x f", "\\lim\\limits_a x",
              "\\int\\limits_a^b f", "\\int_a^b f", "\\sum_a^b f", "\\sum\\nolimits_a^b f", "\\sum_a", "\\sum^b",
              "\\int^b", "\\oint_a", "\\sum x", "\\sum", "x\\sum", "\\sum\\sum_a x", "\\bigcup_i A_i",
              "\\sum' x", "\\sum_a' x", "\\prod\\limits^b", "\\lim^b x", "\\lim\\nolimits_a x",
              "x\\limits_a", "x\\nolimits_a", "+\\limits_a", "\\alpha\\limits_a^b", "\\sum\\limits x",
              "\\mathop{x}\\nolimits_a^b", "\\bigoplus_a^b", "\\iint_a^b", "\\iiint_a^b", "\\oint_a^b",
              "\\coprod_a^b", "\\bigotimes_a^b", "\\bigodot_a^b", "\\biguplus_a^b", "\\bigsqcup_a^b",
              "\\bigvee_a^b", "\\bigwedge_a^b", "\\bigcap_a^b", "\\injlim_a", "\\projlim_a", "\\varlimsup_a",
              "\\varliminf_a", "\\varinjlim_a", "\\varprojlim_a", "\\liminf_a x", "\\limsup_a x"]:
        add(t)
        add(t, "D")
    for st in "displaystyle textstyle scriptstyle scriptscriptstyle".split():
        for t in ["\\%s x", "\\%s{x}", "a \\%s b c", "{\\%s a b} c", "\\%s\\sum_i x", "\\frac{\\%s a}{b}",
                  "\\%s\\frac{a}{b} + c", "\\%s", "x^{\\%s a}", "\\%s a \\%s b", "a + \\%s", "\\left(\\%s a\\right)",
                  "\\%s\\lim_a x", "\\%s\\int_a^b"]:
            add(t.replace("%s", st))
            add(t.replace("%s", st), "D")
    for t in ["\\boxed{x}", "\\boxed{x+1}", "\\boxed{x} y", "\\boxed{}", "\\phantom{x}", "\\phantom{x+1}",
              "a\\phantom{b}c", "\\hphantom{x}", "\\vphantom{x}", "\\color{red} x", "\\color{red}{x} y",
              "{\\color{red} x} y", "\\color{#ff0000} x", "\\color[rgb]{1,0,0} x", "a \\color{blue} b + c",
              "\\color{red}", "x \\color{red}", "\\color{red} x \\color{blue} y", "\\color{Red} x",
              "\\color{ red } x", "\\color{nocolor} x", "\\color{#F00} x", "\\color{ff0000} x",
              "\\frac{\\color{red} a}{b}", "\\color{red}{x+1}", "\\color{red} \\frac{a}{b}",
              "\\left(\\color{red} x\\right)", "x^{\\color{red} a}", "\\color{red} x_1", "\\color{red} x + y = z",
              "\\textcolor{red}{x}", "\\textcolor{red}{x+1} y", "\\textcolor{#00ff00}{x}", "\\textcolor{red} x",
              "a \\textcolor{red}{+} b", "\\textcolor{red}{\\text{a}}", "\\textcolor{red}{}", "\\textcolor{red}{1}",
              "\\textcolor{red}{\\alpha}", "\\textcolor{red}{x}_1", "\\textcolor[rgb]{1,0,0}{x}",
              "\\colorbox{red}{x}", "\\mathcolor{red}{x}", "\\fcolorbox{red}{blue}{x}",
              "\\cancel{x}", "\\bcancel{x}", "\\xcancel{x}", "\\sout{x}", "\\cancel{x+1}", "\\smash{x}",
              "\\mathstrut", "\\strut", "\\fbox{x}", "\\hspace{1em}", "a\\hspace{2pt}b", "a\\hspace{1cm}b",
              "a\\hspace{-1em}b", "a\\hspace{1.5em}b", "a\\hspace{10mu}b", "a\\hspace{1ex}b", "a\\hspace{1in}b",
              "a\\hspace{12pt}b", "a\\hspace{0.5em}b", "a\\hspace*{1em}b", "a\\hspace{ 1em }b", "a\\hspace{x}b",
              "a\\hskip 1em b", "a\\kern 1em b", "a\\mkern 3mu b", "a\\mskip 3mu b", "a\\kern1emb", "a\\mkern18mu b",
              "a\\mkern-3mu b", "\\rule{1em}{1em}", "\\substack{a\\\\b}", "\\sum_{\\substack{a\\\\b}} x",
              "\\substack{a}", "\\substack{a\\\\b\\\\c}", "\\mathclap{x}", "\\mathllap{x}", "\\mathrlap{x}",
              "\\llap{x}", "\\rlap{x}", "\\tag{1} x", "x \\tag{1}", "\\label{a} x", "x \\label{a}",
              "\\nonumber x", "x \\nonumber", "\\notag x", "\\allowbreak x", "x \\allowbreak y", "\\nobreak x",
              "a \\bmod b", "a \\pmod{b}", "a \\pmod b", "a \\mod b", "a \\pod{b}", "\\pmod{a+b}", "a \\bmod", "\\bmod b",
              "a \\quad b", "a \\qquad b", "a \\enspace b", "a \\, b", "a \\: b", "a \\; b", "a \\! b", "a \\  b",
              "a \\> b", "a ~ b", "a \\medspace b", "a \\thinspace b", "a \\thickspace b", "a \\negmedspace b",
              "a \\negthickspace b", "a \\negthinspace b", "a \\enskip b", "a \\nobreakspace b", "a \\space b",
              "a\\\\b", "a \\\\ b \\\\ c", "a \\newline b", "a \\cr b", "a \\\\[1em] b", "\\\\", "a\\\\",
              "\\displaylines{a\\\\b}", "\\begin{aligned}a\\end{aligned}\\\\b", "x\\\\\\begin{matrix}a\\end{matrix}"]:
        add(t)
    for t in ["\\newcommand{\\R}{\\mathbb{R}}\\R", "\\newcommand{\\a}{b} \\a + \\a", "\\newcommand{\\f}[1]{#1^2} \\f{x}",
              "\\newcommand{\\f}[2]{#1+#2} \\f{x}{y}", "\\newcommand{\\f}[2][z]{#1+#2} \\f{y} \\f[w]{y}",
              "\\newcommand\\a{b}\\a", "\\renewcommand{\\alpha}{b}\\alpha", "\\providecommand{\\a}{b}\\a",
              "\\def\\a{b}\\a", "\\def\\a#1{#1+#1}\\a x", "\\def\\a#1#2{#1-#2}\\a{x}{y}", "\\newcommand{\\a}{b}",
              "\\newcommand{\\a}{\\b}\\newcommand{\\b}{c}\\a", "\\let\\a\\alpha \\a",
              "\\DeclareMathOperator{\\foo}{foo}\\foo x", "\\DeclareMathOperator*{\\foo}{foo}\\foo_a x",
              "\\newcommand{\\f}[1]{#1^2} \\f x", "\\newcommand{\\f}[1]{#1^2} \\f", "\\newcommand{\\f}[1]{#1^2} \\f{x}{y}",
              "\\newcommand{\\a}{b}\\newcommand{\\a}{c}\\a", "\\newcommand{\\a}{b}\\renewcommand{\\a}{c}\\a",
              "\\renewcommand{\\a}{c}\\a", "\\providecommand{\\alpha}{b}\\alpha", "\\newcommand{\\sin}{b}\\sin",
              "\\newcommand{\\a}{x^}\\a 2", "\\newcommand{\\a}[1]{\\frac{#1}{2}}\\a{\\a{x}}",
              "\\newcommand{\\a}[1]{#1#1}\\a{\\a{\\a{x}}}", "\\newcommand{\\a}{b}x\\a y", "x \\newcommand{\\a}{b} \\a",
              "{\\newcommand{\\a}{b}} \\a", "\\newcommand{\\a}[0]{b}\\a", "\\newcommand{\\a}[10]{b}\\a",
              "\\newcommand{\\ab}{b}\\ab\\abc", "\\newcommand{\\a}{b}\\a1", "\\newcommand{\\a}{b}\\a x",
              "\\newcommand{\\a}{ b }\\a\\a", "\\newcommand{\\a}{+}x\\a y", "\\newcommand{\\a}{^2}x\\a",
              "\\newcommand{\\a}[1]{\\text{#1}}\\a{b c}", "\\newcommand{\\a}[2]{#2#1}\\a x y",
              "\\newcommand{\\a}[1]{##1}\\a{x}", "\\newcommand*{\\a}{b}\\a", "\\newcommand{\\a }{b}\\a",
              "\\newcommand{a}{b}a", "\\def\\a{\\b}\\def\\b{c}\\a", "\\def\\a #1{#1}\\a x", "\\def\\a(#1){#1}\\a(x)",
              "\\gdef\\a{b}\\a", "\\edef\\a{b}\\a", "\\newcommand{\\1}{b}\\1",
              "\\newcommand{\\be}{\\begin{matrix}}\\newcommand{\\ee}{\\end{matrix}}\\be x\\ee",
              "\\newenvironment{foo}{a}{b}\\begin{foo}x\\end{foo}", "\\newcommand{\\a}[1]{\\mathbf{#1}}\\a{x}_1",
              "x^\\a", "\\newcommand{\\a}{b}x^\\a", "\\newcommand{\\a}{bc}x^\\a", "\\newcommand{\\a}{bc}\\frac\\a\\a",
              "\\newcommand{\\a}{b}\\text{\\a}", "\\newcommand{\\a}{b}\\mathrm{\\a}", "\\newcommand{\\a}{b}\\operatorname{\\a}",
              "\\newcommand{\\a}[1]{#1}\\a{\\frac{x}{y}}", "\\newcommand{\\a}[1]{#1}\\a\\frac{x}{y}",
              "\\newcommand{\\a}[2]{#1#2}\\a{x}", "\\newcommand{\\a}[1][c]{#1}\\a", "\\newcommand{\\a}[1][c]{#1}\\a[d]",
              "\\newcommand{\\a}[1][c]{#1}\\a x", "\\newcommand{\\a}[2][c]{#1#2}\\a", "\\newcommand{\\a}{#1}\\a x",
              "\\newcommand{\\a}{\\a}\\a", "\\newcommand{\\a}{x\\a}\\a", "\\newcommand{\\a}{\\b}\\newcommand{\\b}{\\a}\\a",
              "\\newcommand{\\a}{\\b}\\newcommand{\\b}{\\c}\\newcommand{\\c}{d}\\a",
              "\\newcommand{\\a}{\\b}\\newcommand{\\b}{\\c}\\newcommand{\\c}{\\d}\\newcommand{\\d}{e}\\a",
              "\\newcommand{\\a}[1]{#1}\\a{\\a{\\a{x}}}", "\\newcommand{\\a}[1]{#1#1}\\a{\\a{x}}",
              "\\newcommand{\\a}{b}\\newcommand{\\c}{\\a\\a}\\c\\c", "\\newcommand{\\a}{b}\\providecommand{\\a}{c}\\a",
              "\\newcommand{\\a}{x}\\a\\foo", "\\newcommand{\\a}{x} \\a \\foo y"]:
        add(t)
    for t in ["x^2^3", "x_1_2", "a^{b}^{c}", "x^2_3^4", "x_1^2_3", "x'^2", "x^2'", "x''_1", "x'_1'", "x^'", "x_1'",
              "x^{a}'", "{x'}'", "x'{}'", "\\sum^a^b", "x^a_b^c_d", "'", "x^\\prime", "x'''''", "f''''", "f'''",
              "x^2^3^4", "x_1_2_3", "x^{2}_{3}^{4}_{5}", "x^1'", "x'^1'", "x_a'^b", "x^a{}^b", "x ^ 2", "x _ 1 ^ 2",
              "x^\\alpha\\beta", "x^ab", "x^12", "x^{12}", "x^1.5", "x^\\frac12", "x^\\frac{1}{2}", "x^\\sqrt{2}",
              "x^\\mathrm{T}", "x^\\text{a}", "x^\\hat{a}", "x^\\left(a\\right)", "x^+", "x^-", "x^*", "x^=", "x^,",
              "x^(", "x^)", "x^|", "x^\\{", "x^\\sin", "x^\\sum", "x^\\leq", "x^\\to", "x^\\infty", "x^\\,", "x^\\quad",
              "x^~", "x_+", "x_-", "x_=", "x_{+}", "x_{-}", "x_{=}", "x^{+}", "x^{-}", "x^{-1}", "x^{+1}", "x^{a+}",
              "x^{=a}", "x^{a=}", "x^{,}", "x^{a,}", "x^{(}", "x^{)}", "x^{\\sin}", "x^{\\sin a}", "x^{\\leq}", "x^{*}",
              "x^{\\times}", "x^{\\cdot}", "x^{\\pm}", "x^{\\circ}", "x^{\\dagger}", "x^{\\prime}", "x^{\\prime\\prime}",
              "x^{/}", "x^{!}", "x^{.}", "x^{:}", "x^{;}", "x^{<}", "x^{>}", "x^{|}", "x^{[}", "x^{]}", "{+}", "{-}", "{=}",
              "{,}", "{(}", "{\\sin}", "{+}x", "x{+}y", "x{=}y", "x{,}y", "x+{}", "{}+x", "x={}", "{}=x", "x,{}", "+{}+",
              "{a+}", "{+a}", "a+{+a}", "\\frac{+}{-}", "\\frac{=}{,}", "\\sqrt{+}", "\\sqrt{-a}", "\\frac{-a}{b-}",
              "(+)", "(-a)", "[-a]", "\\{-a\\}", "a,-b", "a;-b", "a=-b", "a+-b", "a--b", "a\\times-b", "a\\leq-b",
              "\\sin-a", "\\sum-a", "-", "--", "-a", "+a", "a+", "a-", "a+)", "a+=", "a+,", "a=+", "=a", "a=", "==",
              "a==b", "a=<b", ",a", "a,", ",,", "a,,b", "a:=b", "a::b", "a.b", "a..b", "1.2", ".5", "1.", "1.2.3",
              "12abc", "1 2", "1e5", "1,000", "1{,}000", "0.5x", "x1", "x 1", "1x1", "a!b", "a!", "!a", "a?b", "a/b",
              "a/", "/a", "a|b", "a*b", "a*", "*a", "a**b", "a@b", "a\"b", "a`b", "a'b", "a<b", "a>b", "a<=b", "a>=b",
              "a<<b", "a>>b", "a->b", "a<-b", "a=>b", "a\\{b", "a\\}b", "a(b", "a)b", "a[b", "a]b", ")a(", "]a[",
              "(a]", "[a)", "((a))", "(a)(b)", "f(a)", "f(a,b)", "f[a]", "(a+b)c", "a(-b)", "(a)-b", "-(a)", "(a)+", "+(a)"]:
        add(t)
    for t in ["|a|", "|a|+|b|", "a|b", "\\vert a \\vert", "\\lvert a \\rvert", "\\|a\\|", "\\Vert a\\Vert",
              "\\lVert a \\rVert", "x|", "|", "||a||", "|a|^2", "\\mid", "a \\mid b", "(a|b)", "\\{x | x>0\\}",
              "|x-y|<\\epsilon", "\\langle a | b \\rangle", "a\\|b", "\\left|a\\right|", "|-a|", "|a|-|b|", "-|a|",
              "|a+b|", "a=|b|", "|a|=b", "\\|", "\\| -a \\|", "\\vert", "\\Vert", "\\lvert", "\\rvert", "\\lVert",
              "\\rVert", "a \\vert b", "a \\Vert b", "a \\lvert b", "a \\rvert b", "|a|_1", "\\|a\\|_2^2", "|_a",
              "|^2", "(a)^2", ")^2", "]_1", "\\}_1^2", "\\rangle^2", "\\langle_1", "(^2", "a \\parallel b",
              "a \\backslash b", "a \\setminus b", "a \\smallsetminus b", "\\langle a,b\\rangle", "\\lfloor a \\rfloor",
              "\\lceil -a \\rceil", "\\lbrace a \\rbrace", "\\lbrack a \\rbrack", "< a >", "\\lt a \\gt",
              "\\ulcorner a \\urcorner", "\\llcorner a \\lrcorner", "\\lgroup a \\rgroup", "\\lmoustache a \\rmoustache",
              "\\llbracket a \\rrbracket", "\\lBrace a \\rBrace", "\\lparen a \\rparen", "\\uparrow a \\downarrow"]:
        add(t)
    for t in ["x \u2264 y", "\u2211_i x", "a \u00d7 b", "\u00e9", "\u4e2d", "x\u2192y", "\u2200x", "\u00bd", "\u00b0",
              "\u2032", "\u03b1\u03b2", "\u0393", "x\u00b2", "\u221e", "\u2202x", "a\u00a0b", "a\u2009b", "\u2115",
              "\u210f", "\u2260", "\u00b1", "\u00b7", "\u2026", "\u2208", "\u2229", "\u222b_a^b", "\u221a2", "\u00ac p",
              "\u2227", "\u2228", "\u2205", "\u2248", "\u2261", "\u2282", "\u2286", "\u00f7", "\u2217", "\u2218",
              "\u2295", "\u2297", "\u22c5", "\u27e8a\u27e9", "\u2308a\u2309", "\u230aa\u230b", "\u2016a\u2016",
              "\u03a9", "\u0394x", "\U0001d400", "\u0661", "\u00e9^2", "a\u0301", "\u00c5", "\u00df", "\u0416",
              "\u05d0", "\u3042", "a\tb", "a\nb", "a\n\nb", "a % c\nb", "a \\% b", "% c", "a%", "a\\\nb",
              "\\\tb", "a\\ b", "\\ ", "\\\n", "a\\\\\n", "\\alpha1", "\\alpha b", "\\alphab", "\\alpha_1",
              "\\alpha{}b", "\\ alpha", "\\{", "\\}", "\\[", "\\]", "\\(", "\\)", "\\<", "\\>", "\\-", "\\+", "\\=",
              "\\'", "\\`", "\\^", "\\~", "\\\"", "\\.", "\\/", "\\@", "\\*", "\\?", "\\1", "\\ae", "\\AA", "\\o",
              "\\ss", "\\i", "\\j", "\\l", "\\L", "\\S", "\\P", "\\dag", "\\ddag", "\\pounds", "\\copyright",
              "\\textbackslash", "\\textasciitilde", "\\textdollar", "\\euro", "\\yen", "\\cent", "\\degree",
              "\\TeX", "\\LaTeX", "\\dots", "\\dotsc", "\\dotsb", "\\dotsm", "\\dotsi", "\\dotso", "\\ldots",
              "\\cdots", "\\vdots", "\\ddots", "a,\\ldots,b", "a+\\cdots+b", "a \\ldots b", "\\ldots b", "a \\cdots",
              "1,2,\\dots,n", "x_1+\\dots+x_n", "\\prime", "\\backprime", "x\\prime", "\\infty", "-\\infty",
              "\\emptyset", "\\varnothing", "\\hbar", "\\hslash", "\\ell", "\\wp", "\\Re", "\\Im", "\\aleph",
              "\\beth", "\\gimel", "\\daleth", "\\eth", "\\partial x", "\\nabla f", "\\Box", "\\Diamond",
              "\\triangle", "\\angle ABC", "\\measuredangle", "\\sphericalangle", "\\top", "\\bot", "\\forall x",
              "\\exists x", "\\nexists x", "\\neg p", "\\lnot p", "\\flat", "\\natural", "\\sharp", "\\surd",
              "\\surd x", "\\checkmark", "\\maltese", "\\circledR", "\\diagup", "\\diagdown", "\\imath", "\\jmath",
              "\\Bbbk", "\\Finv", "\\Game", "\\mho", "\\complement", "\\varkappa", "\\digamma", "\\varGamma",
              "\\varDelta", "\\varTheta", "\\varLambda", "\\varXi", "\\varPi", "\\varSigma", "\\varUpsilon",
              "\\varPhi", "\\varPsi", "\\varOmega", "\\Alpha", "\\Beta", "\\omicron", "\\Epsilon"]:
        add(t)
    return out


def formulas():
    out = []

    def add(t, kind="I"):
        out.append((kind, t))

    for c in GREEK + SYMBOLS + BIGOPS + FUNCS:
        add("\\" + c)
        add("a \\" + c + " b")
    for c in BIGOPS + FUNCS:
        add("\\%s_{i=1}^{n} x" % c)
        add("\\%s_{i=1}^{n} x" % c, "D")
        add("\\%s\\limits_a^b x" % c)
        add("\\%s\\nolimits_a^b x" % c)
    for c in ACCENTS:
        add("\\%s{x}" % c)
        add("\\%s{xy}" % c)
        add("\\%s x + 1" % c)
    for c in ONEARG:
        add("\\%s{x}" % c)
        add("\\%s{ab c}" % c)
        add("\\%s{x+1} y" % c)
        add("\\%s{AZ09}" % c)
    for c in TWOARG:
        add("\\%s{a}{b}" % c)
        add("\\%s{a+1}{b} c" % c)
    add("\\sqrt[3]{x}")
    add("\\sqrt[n]{x+1}")
    add("\\genfrac{(}{)}{0pt}{}{a}{b}")
    add("\\color{red}{x}")
    add("\\textcolor{blue}{x}")
    for c in SPACES:
        add("a\\%s b" % c)
    for c in SIZES:
        add("\\%s( x \\%s)" % (c, c))
    for d in ["(", ")", "[", "]", "\\{", "\\}", "|", "\\|", ".", "\\langle", "\\rangle",
              "\\lfloor", "\\rfloor", "/", "\\uparrow"]:
        add("\\left%s x \\right." % d)
        add("\\left. x \\right%s" % d)
    for e in ENVS:
        add("\\begin{%s}a&b\\\\c&d\\end{%s}" % (e, e))
        add("\\begin{%s}a&b\\\\c&d\\end{%s}" % (e, e), "D")
    add("\\begin{array}{lcr}a&b&c\\\\d&e&f\\end{array}")
    add("\\begin{array}{|c|}a\\\\b\\end{array}")
    for t in ["x", "x_1", "x^2", "x_i^2", "x'", "x''", "x^{\\prime}", "f'(x)", "x_{ij}", "x^{n+1}",
              "{}_a^b", "x^2_3", "a^{b^c}", "a_{b_c}", "10", "3.14", "1,000", "-1", "x-y", "a=b",
              "a+-b", "(a)", "[a]", "\\{a\\}", "|a|", "a/b", "a!", "a?", "a;b", "a:b", "a,b",
              "a.b", "ab", "abc", "2x", "\\$", "\\%", "\\#", "\\_", "\\&", "x \\over y",
              "a \\choose b", "\\mathrm{d}x", "e^{i\\pi}", "\\not=", "\\not\\in", "''",
              "\\{ x \\mid x > 0 \\}", "x\\,y", "x\\ y", "x~y", "\\newcommand{\\R}{\\mathbb{R}}\\R",
              "\\def\\a{b}\\a", "\\text{a b}", "\\text{$x$}", "\\mathrm{a b}", "A^T", "x^*",
              "x^\\dagger", "x^\\circ", "\\hat{\\imath}", "a \\bmod b", "a \\pmod{b}",
              "\\lim_{x\\to 0} f", "\\int_0^1 f(x)\\,dx", "\\displaystyle\\sum_i x",
              "\\operatorname*{argmax}_x f", "\\tag{1} x", "\\label{a} x", "\\nonumber x",
              "\\overbrace{a+b}^{n}", "\\underbrace{a+b}_{n}", "\\mathbb{1}", "\\mathcal{L}",
              "\\mathfrak{g}", "\\mathbf{0}", "\\infty", "a \\cdot\\cdot b", "\\ldots",
              "x \\in \\mathbb{R}^n", "\\| x \\|", "\\vert x \\vert", "f \\circ g",
              "\\hbox{a}", "\\varGamma", "\\mathbf{\\alpha}", "\\boldsymbol{\\alpha}",
              "\\exp(x)", "\\sin^2 x", "\\log_2 n", "\\det A", "\\sqrt{}", "\\frac12",
              "\\frac 1 2", "x^-", "x^+", "+", "-", "=", "<", ">", "", " ", "a  b"]:
        add(t)
    out += matrices(katex_names())
    out += [("I", t) for t in DOCUMENT]
    out += [("D", t) for t in DOCUMENT[::3]]
    out += [("I", t) for t in MALFORMED]
    pieces = ["x", "y", "2", "\\alpha", "+", "-", "=", "\\leq", "(", ")", "\\sin", "a", "\\infty"]
    wraps = ["{%s}", "\\frac{%s}{%s}", "\\sqrt{%s}", "%s^{%s}", "%s_{%s}", "\\left(%s\\right)",
             "\\mathbf{%s}", "\\text{%s}", "\\hat{%s}", "\\begin{matrix}%s&%s\\end{matrix}",
             "\\operatorname{%s}", "\\overline{%s}", "\\binom{%s}{%s}", "\\mathrm{%s}"]
    for w1 in wraps:
        for w2 in wraps:
            inner = w2.replace("%s", "x")
            add(w1.replace("%s", inner))
    rng = random.Random(13)

    def gen(depth):
        if depth == 0 or rng.random() < 0.35:
            return " ".join(rng.choice(pieces) for _ in range(rng.randint(1, 3)))
        return _fill(rng.choice(wraps), depth)

    def _fill(w, depth):
        parts = w.split("%s")
        res = parts[0]
        for p in parts[1:]:
            res += gen(depth - 1) + p
        return res

    for _ in range(3200):
        add(gen(3))
    # The inputs of the review findings and the matrices that found their rules.
    with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "findings.txt"), encoding="utf-8") as f:
        for line in f:
            if line.strip():
                k, t = line.rstrip("\n").split("\t", 1)
                add(unesc(t), k)
    seen = set()
    uniq = []
    for k, t in out:
        if (k, t) not in seen:
            seen.add((k, t))
            uniq.append((k, t))
    return uniq


DOCUMENT = [
    "E = mc^2", "F = ma", "\\nabla \\cdot \\mathbf{E} = \\frac{\\rho}{\\epsilon_0}",
    "\\nabla \\times \\mathbf{B} = \\mu_0 \\mathbf{J}", "p = \\hbar k", "\\Delta x \\Delta p \\geq \\frac{\\hbar}{2}",
    "i\\hbar \\frac{\\partial}{\\partial t} \\psi = H \\psi", "\\mu = \\frac{1}{n}\\sum_{i=1}^n x_i",
    "\\sigma^2 = E[(X-\\mu)^2]", "P(A \\mid B) = \\frac{P(B \\mid A) P(A)}{P(B)}",
    "X \\sim N(\\mu, \\sigma^2)", "\\bar{x} \\pm 1.96 \\frac{s}{\\sqrt{n}}",
    "Ax = b", "\\det(A - \\lambda I) = 0", "A^{-1} A = I", "\\langle u, v \\rangle = u^T v",
    "\\|x\\|_2 = \\sqrt{x_1^2 + x_2^2}", "\\forall x \\exists y \\, P(x,y)", "p \\land q \\to r",
    "\\neg (p \\lor q) \\iff \\neg p \\land \\neg q", "H_2O", "\\mathrm{H_2SO_4}",
    "2H_2 + O_2 \\rightarrow 2H_2O", "\\int_a^b f(x)\\,dx = F(b) - F(a)", "e^{i\\pi} + 1 = 0",
    "a^2 + b^2 = c^2", "x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}", "\\lim_{n \\to \\infty} (1 + 1/n)^n = e",
    "f'(x) = \\lim_{h\\to 0} \\frac{f(x+h)-f(x)}{h}", "\\sum_{k=0}^\\infty \\frac{x^k}{k!}",
    "\\binom{n}{k} = \\frac{n!}{k!(n-k)!}", "O(n \\log n)", "\\mathbb{E}[X] = \\sum_x x p(x)",
    "\\Pr[X \\ge t] \\le \\frac{E[X]}{t}", "S = k_B \\ln \\Omega", "PV = nRT", "\\Delta G = \\Delta H - T\\Delta S",
    "\\vec{F} = q(\\vec{E} + \\vec{v} \\times \\vec{B})", "\\oint_C \\mathbf{F} \\cdot d\\mathbf{r}",
    "\\{x \\in \\mathbb{Z} : x > 0\\}", "A \\cup B = B \\cup A", "A \\subseteq B", "f: X \\to Y",
    "g \\circ f", "\\sin^2\\theta + \\cos^2\\theta = 1", "\\tan\\theta = \\frac{\\sin\\theta}{\\cos\\theta}",
    "\\hat{\\beta} = (X^T X)^{-1} X^T y", "R^2 = 1 - \\frac{SS_{res}}{SS_{tot}}", "\\chi^2", "t_{n-1}",
    "\\rho_{XY} = \\frac{\\mathrm{Cov}(X,Y)}{\\sigma_X \\sigma_Y}", "\\mathcal{O}(1)", "n \\equiv 1 \\pmod 4",
    "\\gcd(a,b) = 1", "\\lfloor x \\rfloor", "\\lceil n/2 \\rceil", "|x - y| < \\epsilon",
    "\\alpha \\beta \\gamma", "v = v_0 + at", "x(t) = x_0 + v_0 t + \\frac{1}{2} a t^2",
    "\\omega = 2\\pi f", "E_k = \\frac{1}{2}mv^2", "\\lambda = \\frac{h}{p}", "c = \\lambda \\nu",
    "\\mathrm{pH} = -\\log_{10}[\\mathrm{H}^+]", "K_a = \\frac{[H^+][A^-]}{[HA]}",
    "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}", "\\begin{cases} 1 & x > 0 \\\\ 0 & x \\le 0 \\end{cases}",
    "\\operatorname{tr}(A)", "\\operatorname{rank} A", "\\ker T", "\\dim V = n", "u \\perp v",
    "\\partial_x u + \\partial_y v = 0", "\\frac{\\partial^2 u}{\\partial t^2} = c^2 \\nabla^2 u",
    "\\iint_D f\\,dA", "\\prod_{p} \\frac{1}{1-p^{-s}}", "\\zeta(s) = \\sum_{n=1}^\\infty n^{-s}",
    "\\Gamma(n) = (n-1)!", "\\aleph_0", "2^{\\aleph_0}", "\\mathbb{N} \\subset \\mathbb{Z} \\subset \\mathbb{Q}",
    "\\vdash \\phi", "\\Gamma \\vdash A \\to B", "\\models \\phi", "\\top \\land \\bot", "\\Box p \\to p",
]

MALFORMED = [
    "\\frac{a", "x^", "x_", "{", "}", "a}", "{a", "\\left( x", "x \\right)", "\\begin{matrix} a",
    "\\end{matrix}", "\\sqrt", "\\sqrt[", "x^^2", "x__2", "x^2^3", "x_1_2", "\\foo", "\\", "a\\", "&",
    "a & b", "a \\\\ b", "\\begin{foo}x\\end{foo}", "\\begin{matrix}a\\end{pmatrix}", "\\frac", "\\frac{a}",
    "\\text", "\\mathbf", "\\left", "\\right", "\\left(", "\\right)", "\\left x \\right)", "#", "a # b",
    "\\hat", "^2", "_2", "x'^", "\\color{red}", "\\textcolor{x}", "\\binom{a}", "\\over", "a \\over b \\over c",
    "\\begin{array}a\\end{array}", "\\newcommand", "\\newcommand{\\a}", "\\operatorname", "\\mathrm{",
    "\\text{a", "x^{", "%", "a % b", "\\left[ a \\middle", "\\limits", "x\\limits^2", "\\big", "\\big x",
    "\\sum\\limits\\limits", "\\overbrace", "\\underbrace{x}^", "\\sqrt[3", "\\stackrel{a}",
    "~", "\\,", "\\;", "\\!", "\\quad", "\\ ", "a^{b}^{c}", "\\mathbb", "\\not", "\\not a",
] + ["\\" + c + "{" for c in ("frac sqrt mathbf hat text begin end left operatorname").split()] + \
    ["x" + "^" * n for n in range(1, 4)] + ["{" * n + "x" + "}" * (n - 1) for n in range(2, 6)]


def katex_names():
    """The candidate command names of `tools/katex-names.txt` (KaTeX, MIT)."""
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "katex-names.txt")
    with open(path, encoding="utf-8") as f:
        return [line.strip() for line in f if line.strip() and not line.startswith("#")]


def esc(s):
    return s.replace("\\", "\\\\").replace("\t", "\\t").replace("\n", "\\n").replace("\r", "\\r")


def run_oracle(doc, args):
    pandoc = os.environ.get("RANDOC_ORACLE_PANDOC")
    if not pandoc or "GHCRTS" not in os.environ:
        sys.exit("probe.py: source env.sh first")
    p = subprocess.run([pandoc, "-f", "json", "-t", "html"] + args, input=doc.encode(),
                       capture_output=True, timeout=60)
    if p.returncode != 0:
        sys.exit("probe.py: the oracle failed with exit code %d: %s" % (p.returncode, p.stderr[:500]))
    return p.stdout.decode(), p.stderr.decode()


def split_paras(html):
    html = html.strip("\n")
    assert html.startswith("<p>") and html.endswith("</p>")
    return html[3:-4].split("</p>\n<p>")


def split_warnings(err):
    return [w for w in re.split(r"(?m)^\[WARNING\] ", err) if w.strip()]


def is_fallback(para):
    return para.startswith('<span class="math inline">$') or \
        para.startswith('<span class="math display">$$') or \
        re.match(r'<span\nclass="math (inline|display)">\$', para) is not None


def record(items):
    rows = []
    for start in range(0, len(items), BATCH):
        chunk = items[start:start + BATCH]
        blocks = [{"t": "Para", "c": [{"t": "Math", "c": [
            {"t": "DisplayMath" if k == "D" else "InlineMath"}, t]}]} for k, t in chunk]
        doc = json.dumps({"pandoc-api-version": [1, 23, 1], "meta": {}, "blocks": blocks})
        res = []
        for args in (["--wrap=none", "--math-method=mathml"],
                     ["--wrap=auto", "--columns=1", "--math-method=plain"]):
            out, err = run_oracle(doc, args)
            paras = split_paras(out)
            assert len(paras) == len(chunk), (len(paras), len(chunk))
            warns = [w for w in split_warnings(err) if w.startswith("Could not convert TeX math")]
            wmap = {}
            for i, p in enumerate(paras):
                # The oracle indents each continuation line of a warning by two spaces.
                head = "Could not convert TeX math %s, rendering as TeX" % chunk[i][1].replace("\n", "\n  ")
                if warns and is_fallback(p) and warns[0].startswith(head):
                    wmap[i] = warns.pop(0)
            assert not warns, (args, warns[:3])
            res.append((paras, wmap))
        for i, (k, t) in enumerate(chunk):
            rows.append("\t".join([k, esc(t), esc(res[0][0][i]), esc(res[1][0][i]),
                                   esc(res[0][1].get(i, "").rstrip("\n")),
                                   esc(res[1][1].get(i, "").rstrip("\n"))]))
        print("probe.py: %d of %d" % (start + len(chunk), len(items)), file=sys.stderr)
    return rows


def unesc(s):
    return re.sub(r"\\(.)", lambda m: {"t": "\t", "n": "\n", "r": "\r"}.get(m.group(1), m.group(1)), s)


def inner(mathml, inline):
    m = re.sub(r'^<math display="(inline|block)" xmlns="[^"]*"><semantics>', "", mathml)
    m = re.sub(r"<annotation.*", "", m, flags=re.S)
    n = re.sub(r'^<span\s+class="math (inline|display)">', "", inline)
    return m, re.sub(r"</span>$", "", n)


CLASSES = [
    (r"<mi>(.+)</mi>", r"<em>(.+)</em>", "Ident"),
    (r'<mi mathvariant="normal">(.+)</mi>', r"<em>(.+)</em>", "IdentUpright"),
    (r"<mi>(.+)</mi>", r"(.+)", "Ord"),
    (r"<mo>(.+)</mo>", " (.+) ", "Bin"),
    (r"<mo>(.+)</mo>", " (.+) ", "Rel"),
    (r"<mo>(.+)</mo>", "(.+) ", "Punct"),
    (r"<mo>(.+)</mo>", r"([^    ]+)", "Op"),
    (r'<mo stretchy="false" form="prefix">(.+)</mo>', r"(.+)", "Open"),
    (r'<mo stretchy="false" form="postfix">(.+)</mo>', r"(.+)", "Close"),
    (r'<mrow><mi mathvariant="normal">(.+)</mi><mo>&#8289;</mo></mrow>', "(.+) ", "Func"),
    (r'<mtext mathvariant="normal">(.+)</mtext>', r"(.+)", "Text"),
]


def rust_str(text):
    """A Rust string literal with each non-ASCII character as an escape."""
    out = []
    for c in text:
        if c in '\\"':
            out.append("\\" + c)
        elif " " <= c <= "~":
            out.append(c)
        else:
            out.append("\\u{%x}" % ord(c))
    return '"' + "".join(out) + '"'


def read_rows(tsv):
    rows = {}
    with open(tsv, encoding="utf-8", newline="\n") as f:
        for line in f:
            k, tex, mml, inl, _, _ = [unesc(x) for x in line.rstrip("\n").split("\t")]
            rows[(k, tex)] = (mml, inl)
    return rows


def leaves(mathml):
    """The (mathvariant, text) pairs of the token elements of a MathML text."""
    found = re.findall(r'<m(?:i|n|o|text)(?: mathvariant="([^"]*)")?>([^<]*)</m', mathml)
    return [(v, html_unescape(t)) for v, t in found]


def variants(rows):
    """The character pairs of each mathvariant value: the characters that the
    oracle writes for the letters, digits and Greek letters in each alphabet."""
    upper = "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
    charwise = [upper, upper.lower(), "0123456789", "".join("\\" + g + " " for g in LOWER_GREEK),
                "".join("\\" + g + " " for g in UPPER_GREEK), "\\partial\\infty\\nabla\\hbar\\ell\\imath\\sum"]
    pairs = {}

    def learn(tex, base):
        row = rows.get(("I", tex))
        if not row or not row[0].startswith("<math"):
            return
        found = [(v, c) for v, text in leaves(inner(row[0], row[1])[0]) for c in text]
        if len(found) != len(base):
            return
        for src, (variant, dst) in zip(base, found):
            if variant and src != dst:
                assert pairs.setdefault(variant, {}).setdefault(src, dst) == dst, (tex, src, dst)

    for arg in charwise:
        plain = rows.get(("I", "\\mathrm{%s}" % arg))
        base = "".join(text for _, text in leaves(inner(plain[0], plain[1])[0]))
        for a in ALPHABETS:
            learn("\\%s{%s}" % (a, arg), base)
    # Each symbol command alone in an alphabet (`\mathbf{\Alpha}`).
    for k, tex in list(rows):
        m = re.fullmatch(r"\\([a-z]+)\{(\\[A-Za-z]+)\}", tex)
        plain = m and rows.get(("I", "\\mathrm{%s}" % m.group(2)))
        if k == "I" and plain and plain[0].startswith("<math") and m.group(1) in ALPHABETS:
            learn(tex, "".join(text for _, text in leaves(inner(plain[0], plain[1])[0])))
    for arg in charwise[:3]:
        for t in TEXTS:
            learn("\\%s{%s}" % (t, arg), arg)
    return pairs


def classify(mml, inl):
    """The class, the MathML text and the inline text of the item between `a`
    and `b`, or None when the item has no known class."""
    m, n = inner(mml, inl)
    mm = re.fullmatch(r"<mrow><mi>a</mi>(.*)<mi>b</mi></mrow>", m)
    nn = re.fullmatch(r"<em>a</em>(.*)<em>b</em>", n, flags=re.S)
    if not mm or not nn:
        return None
    for mpat, npat, cls in CLASSES:
        a = re.fullmatch(mpat, mm.group(1))
        b = re.fullmatch(npat, nn.group(1), flags=re.S)
        if a and b and "<" not in a.group(1):
            text = html_unescape(a.group(1))
            if html_unescape(b.group(1)) in (text, text + "\ufe0e"):
                return cls, text, html_unescape(b.group(1))
    return None


def symbol_entries(rows):
    """The commands `\\X` that the oracle renders in `a \\X b` in a form of a
    known class."""
    entries = {}
    for (k, tex), (mml, inl) in rows.items():
        mt = re.fullmatch(r"a \\([A-Za-z]+) b", tex)
        found = classify(mml, inl) if k == "I" and mt else None
        if found:
            entries[mt.group(1)] = found
    return entries


def symbol_chars(entries):
    return sorted({text for cls, text, _ in entries.values() if len(text) == 1 and ord(text) > 127})


def table(tsv, out):
    """Write the tables of `src/symbols.rs` from the recorded results."""
    rows = read_rows(tsv)
    entries = symbol_entries(rows)
    pairs = variants(rows)
    chars = {}
    for c in symbol_chars(entries):
        row = rows.get(("I", "a %s b" % c))
        found = classify(*row) if row else None
        if found and found[1] == c:
            chars[c] = found[0]
    delimiters = sorted(name for name in entries
                        if rows.get(("I", "\\left\\%s x \\right." % name), ("",))[0].startswith("<math"))
    sized = sorted(name for name in entries
                   if rows.get(("I", "\\big\\%s" % name), ("",))[0].startswith("<math"))
    accents = set()
    for (k, tex), (mml, inl) in rows.items():
        mt = re.fullmatch(r"\\sum_\{(\\[A-Za-z]+|\\_|'+)\}", tex)
        mo = re.fullmatch(r'<munder><mo>∑</mo><mo accent="true">([^<]+)</mo></munder>', inner(mml, inl)[0])
        if k == "D" and mt and mo:
            accents.add(html_unescape(mo.group(1)))
    negations = {}
    for (k, tex), (mml, inl) in rows.items():
        mt = re.fullmatch(r"\\not(?:\\([A-Za-z]+)|([^\\A-Za-z0-9 {]))", tex)
        mo = re.fullmatch(r"<mo>([^<]+)</mo>", inner(mml, inl)[0])
        if k != "I" or not mt or not mo:
            continue
        base = entries.get(mt.group(1), (None, None))[1] if mt.group(1) else mt.group(2)
        if base:
            negations[base] = html_unescape(mo.group(1))
    with open(out, "w", encoding="utf-8") as f:
        f.write("//! The tables of observed oracle results. `tools/probe.py table` writes this\n"
                "//! file from `tests/oracle.tsv`.\n\n"
                "use crate::tree::Class;\n\n"
                "/// The commands that stand for one symbol, sorted by name: the name, the\n"
                "/// class, the MathML text and the text of the inline list.\n"
                "pub(crate) static SYMBOLS: &[(&str, Class, &str, &str)] = &[\n")
        for name in sorted(entries):
            cls, text, itext = entries[name]
            f.write("    (%s, Class::%s, %s, %s),\n" % (rust_str(name), cls, rust_str(text), rust_str(itext)))
        f.write("];\n\n"
                "/// The characters of each `mathvariant` value, sorted by the value: each\n"
                "/// pair is a plain character and its form in that alphabet, sorted by the\n"
                "/// plain character.\n"
                "pub(crate) static VARIANTS: &[(&str, &[(char, char)])] = &[\n")
        for variant in sorted(pairs):
            f.write("    (\n        %s,\n        &[\n" % rust_str(variant))
            for src in sorted(pairs[variant]):
                f.write("            ('\\u{%x}', '\\u{%x}'),\n" % (ord(src), ord(pairs[variant][src])))
            f.write("        ],\n    ),\n")
        f.write("];\n\n"
                "/// The class of each character that a command of `SYMBOLS` stands for,\n"
                "/// when the formula holds the character itself (the result of `a C b`),\n"
                "/// sorted by the character.\n"
                "pub(crate) static CHARS: &[(char, Class)] = &[\n")
        for c in sorted(chars):
            f.write("    ('\\u{%x}', Class::%s),\n" % (ord(c), chars[c]))
        f.write("];\n\n"
                "/// The symbol that `\\not` makes from a symbol, sorted by the first text.\n"
                "pub(crate) static NEGATIONS: &[(&str, &str)] = &[\n")
        for base in sorted(negations):
            f.write("    (%s, %s),\n" % (rust_str(base), rust_str(negations[base])))
        f.write("];\n\n"
                "/// The symbols that are an accent above or below a base (the result of\n"
                "/// `\\\\sum_{\\\\X}` in display math), sorted.\n"
                "pub(crate) static ACCENT_SYMBOLS: &[&str] = &[\n")
        for text in sorted(accents):
            f.write("    %s,\n" % rust_str(text))
        f.write("];\n")
        for doc, name, names in (("The commands that `\\\\left`, `\\\\right` and `\\\\middle` take", "DELIMITERS", delimiters),
                                 ("The commands that a size command takes", "SIZED", sized)):
            f.write("\n/// %s as a delimiter,\n/// sorted by name.\npub(crate) static %s: &[&str] = &[\n" % (doc, name))
            for item in names:
                f.write("    %s,\n" % rust_str(item))
            f.write("];\n")
    print("probe.py: %d commands, %d characters, %d variants, %d negations, %d delimiters" %
          (len(entries), len(chars), len(pairs), len(negations), len(delimiters)), file=sys.stderr)


def html_unescape(s):
    return s.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")


def check(tsv, count):
    """Record a sample of the rows one formula at a time and compare with the
    batch result: a failure of one formula must not change the others."""
    with open(tsv, encoding="utf-8", newline="\n") as f:
        lines = [line.rstrip("\n") for line in f if line.strip()]
    rng = random.Random(7)
    bad = 0
    for line in rng.sample(lines, count):
        fields = line.split("\t")
        single = record([(fields[0], unesc(fields[1]))])[0]
        if single != line:
            bad += 1
            print("probe.py: batch and single results differ: %s" % fields[1], file=sys.stderr)
    print("probe.py: %d of %d sample rows equal" % (count - bad, count), file=sys.stderr)


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "check":
        check(sys.argv[2], int(sys.argv[3]))
        return
    if len(sys.argv) == 4 and sys.argv[1] == "table":
        table(sys.argv[2], sys.argv[3])
        return
    if len(sys.argv) == 3 and sys.argv[1] == "generate":
        items = formulas()
        out = sys.argv[2]
    elif len(sys.argv) == 4 and sys.argv[1] == "record":
        items = []
        with open(sys.argv[2], encoding="utf-8") as f:
            for line in f:
                line = line.rstrip("\n")
                if line:
                    k, t = line.split("\t", 1)
                    items.append((k, unesc(t)))
        out = sys.argv[3]
    else:
        sys.exit(__doc__)
    rows = record(items)
    if sys.argv[1] == "generate":
        # The second pass: each character that a command stands for, as input.
        known = {}
        for line in rows:
            k, tex, mml, inl, _, _ = [unesc(x) for x in line.split("\t")]
            known[(k, tex)] = (mml, inl)
        extra = [("I", "a %s b" % c) for c in symbol_chars(symbol_entries(known))]
        rows += record([item for item in extra if item not in known])
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(rows) + "\n")


if __name__ == "__main__":
    main()
