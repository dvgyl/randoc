"""The snippet bank of the case generator.

Each template is original text for this generator. Nothing here comes from
the Pandoc test suite or the Pandoc manual.

Tokens in a template (`casegen.fill` replaces them):

    @w@  a lowercase word         @W@  a capitalized word
    @n@  a number 1 to 99          @d@  a digit 1 to 9
    @s@  a sentence                @p@  a paragraph of 2 to 4 sentences
    @v@  an ASCII word, the same at each place in one snippet
    @V@  the capitalized @v@
    @id@ @id2@  identifiers, the same at each place in one snippet
    @ref@ an identifier, the same at each place in one document
    @url@ an https URL on example.org
    @lang@ a code language name
    @table:STYLE@  a random table (grid, pipe, simple, rst-simple, org)
    @u:C@  on its own line: a line of C as long as the next text line
           above (an underline) or below (an overline)
"""

WORDS = (
    "apple", "river", "stone", "cloud", "garden", "silver", "window", "harbor",
    "lantern", "meadow", "copper", "signal", "forest", "planet", "engine",
    "island", "marble", "orbit", "pencil", "rocket", "saddle", "tunnel",
    "violet", "walnut", "yellow", "zephyr", "anchor", "basket", "candle",
    "desert", "ember", "falcon", "glacier", "hollow", "ink", "jungle", "kettle",
    "ladder", "mirror", "needle", "oyster", "pepper", "quartz", "ribbon",
    "summit", "timber", "umbrella", "valley", "willow", "xylem", "yarn",
    "zinc", "atlas", "beacon", "cedar", "dune", "echo", "fable", "grove",
    "helix", "iris", "jasper", "koala", "lotus", "mango", "nectar", "onyx",
    "prism", "quill", "raven", "sable", "thistle", "union", "vector", "wave",
    "branch", "bridge", "canyon", "delta", "field", "gate", "hill", "lake",
    "market", "north", "ocean", "path", "quiet", "road", "south", "tower",
    "upper", "village", "west", "data", "table", "list", "note", "item",
    "value", "format", "reader", "writer", "parser", "token", "block",
    "inline", "header", "footer", "section", "chapter", "figure", "caption",
    "café", "naïve", "über", "straße", "façade", "jalapeño", "smørrebrød",
    "Ωmega", "日本語", "数据", "привет", "αβγ",
)

LANGS = ("python", "haskell", "rust", "c", "javascript", "bash", "json", "html", "xml", "lua", "sql", "yaml")

# Markdown, used for markdown, commonmark, commonmark_x and gfm.
MARKDOWN = (
    r"""# @W@ @w@ @w@

@p@""",
    r"""## @W@ @w@ {#@id@ .@w@ data-@w@="@n@"}

@s@""",
    r"""@W@ @w@
========

@s@

@W@ @w@ @w@
-----------

@s@""",
    r"""### @W@ *@w@* `@w@` ###

#### @W@ @w@ ####

@s@""",
    r"""@W@ *@w@* and **@w@ @w@** with `@w@()` code, ~~@w@~~, H~2~O, x^@d@^, ***@w@*** and @w@_@w@_@w@ text, ==@w@== @s@""",
    r"""@W@ [@w@ @w@](@url@ "@W@ @w@") and [@w@][@id@] and <@url@> and @url@ and [@V@] in text.

[@id@]: @url@ "@W@"
[@V@]: @url@""",
    r"""![@W@ @w@](@w@.png "@W@")""",
    r"""@W@ ![@w@](@w@.jpg){width=@n@% .@w@} @s@""",
    r"""- @s@
- @W@ @w@
    - @w@ @w@
    - @w@ **@w@**
        1. @w@
        2. @w@
- @s@""",
    r"""1. @s@

2. @s@

   @s@

3. @W@ @w@""",
    r"""@d@. @W@ @w@
#. @W@ @w@
#. @W@ @w@""",
    r"""a) @w@ @w@
b) @w@
c) @w@

(ii) @w@
(iii) @w@

A. @W@
B. @W@""",
    r"""- [ ] @W@ @w@
- [x] @W@ @w@
- [ ] @s@""",
    r"""@W@
:   @s@

@W@ @w@
:   @s@

:   @s@""",
    r"""> @s@
>
> > @W@ @w@ @w@
>
> - @w@
> - @w@
@s@""",
    r"""```@lang@
def @w@(@w@):
    return @w@ + @n@
```""",
    r"""```{.@lang@ .numberLines startFrom="@n@"}
@w@ = @n@
print(@w@)
```""",
    r"""~~~~ {#@id@ .@lang@}
let @w@ = "@w@";
~~~~""",
    "    @w@ = @n@\n    if @w@ > @d@:\n    \tprint(\"@w@\")",
    r"""@table:pipe@

: @W@ @w@ table""",
    r"""@table:simple@

Table: @W@ @w@""",
    r"""@table:grid@""",
    r"""-------------------------------------
@W@            @W@
-------------- ----------------------
@w@ @w@        @s@
               @w@ @w@

@n@            @w@ @w@ @w@
-------------------------------------

: @W@ multiline""",
    r"""@W@ @w@[^@id@] and @w@.^[@s@]

[^@id@]: @s@

    @w@ @w@ in a second paragraph of the note.""",
    r"""@W@ $x^@d@ + y_{@n@}$ and $\alpha \leq \frac{@d@}{@n@}$.

$$\sum_{i=1}^{@n@} i^2 = \int_0^@d@ f(x)\,dx$$""",
    r"""<div class="@w@">
<p>@s@</p>
<span title="@w@">@w@</span>
</div>""",
    r"""@W@ <span class="@w@">@w@</span> <b>@w@</b> &copy; &amp; &#955; \*@w@\* 5 < @n@ <!-- @s@ -->""",
    r"""```{=latex}
\newpage
\textbf{@w@}
```

@W@ `<b>@w@</b>`{=html} @w@ `\emph{@w@}`{=latex}.""",
    r"""::: {.@w@ #@id@ @w@="@w@"}
@p@

::: @w@
@s@
:::
:::""",
    r"""@W@ [@w@ @w@]{.@w@ key="@w@"} and [@W@]{.smallcaps} and [@w@]{.underline} text.""",
    r"""| @s@
|     @W@ @w@
| @w@ @w@ @w@""",
    r"""(@) @s@
(@@id@) @s@

See (@@id@) for @w@.""",
    r"""@W@ [@@id@, p. @n@] and @@id@ [sec. @d@] and [-@@id@; @@w@@n@].""",
    r"""@s@

* * *

@s@

---

@s@""",
    r""""@W@ @w@," she said -- '@w@' --- @w@... It's @w@'s 1990s."""
    + "\n\n@W@ @w@\\\n@w@ @w@  \n@w@.",
    r""":smile: @W@ :+1: @w@ :heart: :nosuchemoji:""",
    r"""> [!NOTE]
> @s@

> [!WARNING]
> @W@ @w@.""",
    "@W@\t@w@\t@n@\n\n```\n\t@w@\n\t\t@w@\n```",
    r"""@p@ @p@ @p@""",
    r"""# @W@

@s@

# @W@ again

## @W@

@s@""",
    r"""## @W@ {#@id@}

@s@

## @W@ @w@ {#@id@}

@s@""",
    r"""*[HTML]: Hyper Text Markup Language
*[@V@]: @W@ @w@

@V@ and HTML in @w@.""",
    r"""@W@ [[@W@ @w@]] and [[@W@|@w@]] links.""",
    r"""@W@ \(x^@d@\) and \\(y_@d@\\) and $`z^@d@`$ math.""",
    r"""@W@ \emph{@w@} and \LaTeX{} @w@.

\begin{equation}
x = @n@
\end{equation}""",
    r"""日本語の@w@
文章です。
@W@ 中文
句子。""",
    r"""> main :: IO ()
> main = print @n@

@s@""",
    r"""@W@<br>@w@ <sup>@d@</sup> <sub>@d@</sub> <kbd>Ctrl</kbd>""",
    r"""#

##### @W@

###### @W@ @w@ #####""",
    r"""<details>
<summary>@W@</summary>

@s@

</details>""",
)

MARKDOWN_HEADERS = (
    r"""---
title: "@W@ @w@"
author:
  - @W@ @W@
  - name: @W@
    affiliation: @W@ @w@
date: 2024-0@d@-1@d@
keywords: [@w@, @w@]
abstract: |
  @s@
lang: en-US
---""",
    r"""% @W@ @w@
% @W@ @W@; @W@ @W@
% 2023-1@d@-0@d@""",
    r"""---
title: @W@
subtitle: @W@ @w@
toc-title: @W@
---""",
)

HTML = (
    r"""<h1 id="@ref@">@W@ @w@</h1>
<p>@p@</p>""",
    r"""<h2 class="@w@">@W@ <em>@w@</em></h2>
<h3>@W@ @w@</h3>
<p>@s@</p>""",
    r"""<p>@W@ <em>@w@</em> <strong>@w@</strong> <code>@w@</code> <sub>@d@</sub> <sup>@d@</sup> <s>@w@</s> <del>@w@</del> <ins>@w@</ins> <mark>@w@</mark> <small>@w@</small> <u>@w@</u> <q>@w@</q> <abbr title="@W@">@W@</abbr> <kbd>@w@</kbd> <var>@w@</var> <samp>@w@</samp>.</p>""",
    r"""<p>@W@ <a href="@url@" title="@W@">@w@ @w@</a> and <a href="#@ref@">@w@</a> and <a href="mailto:@w@@example.org">@w@</a>.</p>""",
    r"""<p><img src="@w@.png" alt="@W@ @w@" width="@n@" /></p>
<figure>
<img src="@w@.jpg" alt="@w@" />
<figcaption>@W@ <em>@w@</em></figcaption>
</figure>""",
    r"""<ul>
<li>@s@</li>
<li>@W@
<ul>
<li>@w@</li>
<li><p>@w@</p><p>@w@</p></li>
</ul>
</li>
</ul>""",
    r"""<ol start="@d@" type="a">
<li>@w@</li>
<li>@w@</li>
</ol>
<ol reversed>
<li>@W@</li>
<li>@W@</li>
</ol>""",
    r"""<dl>
<dt>@W@</dt>
<dd>@s@</dd>
<dt>@W@ @w@</dt>
<dd><p>@s@</p></dd>
</dl>""",
    r"""<blockquote>
<p>@s@</p>
<blockquote><p>@W@</p></blockquote>
</blockquote>""",
    r"""<pre><code class="language-@lang@">def @w@():
    return &quot;@w@&quot; &lt; @n@
</code></pre>""",
    r"""<pre>  @w@
	@w@   @w@</pre>""",
    r"""<table>
<caption>@W@ @w@</caption>
<thead>
<tr><th>@W@</th><th style="text-align: right">@W@</th><th>@W@</th></tr>
</thead>
<tbody>
<tr><td>@w@</td><td>@n@</td><td>@w@</td></tr>
<tr><td colspan="2">@w@ @w@</td><td rowspan="2">@w@</td></tr>
<tr><td>@n@</td><td>@w@</td></tr>
</tbody>
<tfoot><tr><td>@W@</td><td>@n@</td><td></td></tr></tfoot>
</table>""",
    r"""<table>
<tr><td>@w@</td><td>@w@</td></tr>
<tr><td><p>@s@</p><ul><li>@w@</li></ul></td><td>@n@</td></tr>
</table>""",
    r"""<div class="@w@" id="@id@" data-@w@="@w@">
<p>@s@</p>
<div><p>@w@</p></div>
</div>""",
    r"""<p>@W@<br />@w@<br>@w@</p>
<hr />
<p>&copy; &mdash; &hellip; &nbsp;@w@ &#x3bb; &lt;@w@&gt;</p>""",
    r"""<!-- @s@ -->
<script>var @w@ = @n@;</script>
<style>p { color: red; }</style>
<p>@s@</p>""",
    r"""<section id="@id@">
<h2>@W@</h2>
<p>@s@</p>
</section>
<article><p>@w@</p></article>
<aside><p>@w@</p></aside>
<nav><a href="#@id@">@w@</a></nav>""",
    r"""<details><summary>@W@</summary><p>@s@</p></details>""",
    r"""<ul class="task-list">
<li><input type="checkbox" checked="" />@w@</li>
<li><input type="checkbox" />@w@</li>
</ul>""",
    r"""<p>@W@ <span class="math inline">\(x^@d@\)</span> and $y_@d@$ and \[z = @n@\]</p>""",
    r"""<p>@W@<a href="#fn1" class="footnote-ref" id="fnref1" role="doc-noteref"><sup>1</sup></a></p>
<section class="footnotes" role="doc-endnotes">
<ol>
<li id="fn1"><p>@s@<a href="#fnref1" class="footnote-back">↩︎</a></p></li>
</ol>
</section>""",
    r"""<div class="line-block">@W@ @w@<br />
    @w@ @w@</div>""",
    r"""<p><span class="@w@" lang="fr">@w@</span> <font color="red">@w@</font> <center>@w@</center></p>""",
    r"""<p>@p@ @p@</p>""",
    r"""<h4>@W@</h4><h5>@W@</h5><h6>@W@</h6><p>@w@</p>""",
    r"""<p>“@W@” ‘@w@’ -- --- ...</p>
<p>@w@<wbr>@w@ <iframe src="@url@"></iframe> <video src="@w@.mp4"></video></p>""",
    r"""<p></p>
<p>   </p>
<p>@s@</p>""",
)

HTML_HEADERS = (
    r"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8" />
<title>@W@ @w@</title>
<meta name="author" content="@W@ @W@" />
<meta name="description" content="@s@" />
</head>
<body>""",
    r"""<html><head><title>@W@</title><meta name="keywords" content="@w@, @w@"></head><body>""",
)

LATEX = (
    r"""\section{@W@ @w@}\label{sec:@ref@}

@p@""",
    r"""\subsection*{@W@ \emph{@w@}}

@s@ See Section~\ref{sec:@ref@} and page \pageref{sec:@id@}.""",
    r"""\paragraph{@W@} @s@

\subsubsection{@W@ @w@}
@s@""",
    r"""@W@ \emph{@w@} \textbf{@w@} \texttt{@w@} \textsc{@w@} \underline{@w@} \textit{@w@} \textsf{@w@} {\bf @w@} {\em @w@} \sout{@w@}.""",
    r"""``@W@,'' she said -- `@w@' --- @w@~@w@\ldots{} \& 50\% \$@n@ \#@d@ a\_b \{x\} @w@\\
@w@\newline @w@.""",
    r"""@W@\footnote{@s@} and @w@.\footnote{@W@ \emph{@w@}.}""",
    r"""\href{@url@}{@W@ @w@} and \url{@url@} and \cite{@id@} and \citep[p.~@n@]{@id@}.""",
    r"""\begin{itemize}
\item @s@
\item @W@
  \begin{enumerate}
  \item @w@
  \item[(x)] @w@
  \end{enumerate}
\item @w@ @w@
\end{itemize}""",
    r"""\begin{enumerate}
\item @W@ @w@
\item @s@
\end{enumerate}

\begin{description}
\item[@W@] @s@
\item[@W@ @w@] @w@
\end{description}""",
    r"""\begin{quote}
@s@
\end{quote}

\begin{quotation}
@p@
\end{quotation}""",
    r"""\begin{verbatim}
@w@ = @n@
  if @w@: print("@w@")
\end{verbatim}""",
    r"""\begin{lstlisting}[language=Python]
def @w@():
    return @n@
\end{lstlisting}

Inline \verb|@w@_@w@| and \verb+@n@+.""",
    r"""The value $x^@d@ + \sqrt{@n@}$ and \(y_@d@\) and
\[ \sum_{i=1}^{@n@} \frac{i}{@d@} \]""",
    r"""\begin{equation}\label{eq:@id@}
E = m c^@d@
\end{equation}

\begin{align}
a &= b + @n@ \\
c &= d
\end{align}""",
    r"""\begin{tabular}{|l|c|r|}
\hline
@W@ & @W@ & @W@ \\
\hline
@w@ & @n@ & @w@ \\
@w@ & \textbf{@n@} & @w@ \\
\hline
\end{tabular}""",
    r"""\begin{table}
\centering
\caption{@W@ @w@}\label{tab:@id@}
\begin{tabular}{ll}
@W@ & @W@ \\
@w@ & @n@ \\
\multicolumn{2}{c}{@w@} \\
\end{tabular}
\end{table}""",
    r"""\begin{figure}
\centering
\includegraphics[width=0.@d@\textwidth]{@w@.png}
\caption{@W@ @w@.}\label{fig:@id@}
\end{figure}""",
    r"""\newcommand{\@v@}{@W@ @w@}
\newcommand{\twice}[1]{#1 #1}
\@v@ and \twice{@w@}.""",
    r"""\begin{center}
@s@
\end{center}

\begin{flushright}
@w@
\end{flushright}""",
    r"""% @s@
@W@ @w@ % @w@
\textcolor{red}{@w@} \mbox{@w@} \hspace{1em} \vspace{2pt} @w@""",
    r"""\'e \"o \ss{} \`a \^u \~n \c{c} \v{s} \o{} \aa{} -- @W@ caf\'e.""",
    r"""\begin{abstract}
@s@
\end{abstract}""",
    r"""\chapter{@W@ @w@}
@s@

\part{@W@}""",
    r"""\@v@unknown{@w@} \begin{@v@env}@s@\end{@v@env}""",
    r"""\input{@w@-missing}
@s@""",
    r"""\begin{minipage}{0.5\textwidth}
@s@
\end{minipage}
\noindent @w@ \hfill @w@ \medskip

\bigskip
@p@ @p@""",
)

LATEX_HEADERS = (
    r"""\documentclass{article}
\usepackage{amsmath}
\title{@W@ @w@}
\author{@W@ @W@ \and @W@ @W@}
\date{@d@ March 20@n@}
\begin{document}
\maketitle""",
    r"""\title{@W@}
\author{@W@}""",
)

RST = (
    r"""@W@ @w@
@u:=@

@p@""",
    r"""@u:#@
@W@ @w@
@u:#@

@W@
@u:-@

@s@

@W@ @w@
@u:~@

@s@""",
    r"""@W@ *@w@* **@w@** ``@w@`` `@w@` :sub:`@d@` :sup:`@d@` :math:`x^@d@` :emphasis:`@w@` :strong:`@w@` :literal:`@w@` :title-reference:`@W@`.""",
    r"""@W@ `@w@ @w@ <@url@>`_ and `@v@`_ and `@W@ <@url@>`__ and @url@.

.. _@v@: @url@""",
    r"""@W@ [#]_ and [#@id@]_ and [3]_ and [@V@]_.

.. [#] @s@
.. [#@id@] @s@
.. [3] @s@
.. [@V@] @W@, @w@.""",
    r"""- @s@
- @W@

  - @w@
  - @w@

- @w@ @w@

#. @w@
#. @w@

a. @w@
b. @w@

(i) @w@
(ii) @w@""",
    r"""@W@
   @s@

@W@ : @w@
   @s@""",
    r""":@W@: @w@
:@W@ @w@: @s@
:@w@: @n@""",
    r"""-a         @W@ @w@
--@w@=FILE  @s@
-v, --verbose  @w@""",
    r"""@W@::

    @w@ = @n@
    print(@w@)

@s@""",
    r""".. code-block:: @lang@
   :linenos:

   @w@ = @n@
   @w@()

.. code:: @lang@

   return @w@""",
    r""".. math::

   \sum_{i=1}^{@n@} i = \frac{@n@}{@d@}""",
    r"""    @s@

    -- @W@ @W@""",
    r"""| @s@
| @W@ @w@
|     @w@ @w@""",
    r"""@table:grid@""",
    r"""@table:rst-simple@""",
    r""".. list-table:: @W@ @w@
   :header-rows: 1
   :widths: 20 30

   * - @W@
     - @W@
   * - @w@
     - @n@
   * - @w@ @w@
     - @w@""",
    r""".. csv-table:: @W@
   :header: "@W@", "@W@"

   "@w@", @n@
   "@w@ @w@", @d@""",
    r""".. note::

   @s@

.. warning:: @s@

.. admonition:: @W@ @w@

   @s@""",
    r""".. image:: @w@.png
   :alt: @W@
   :width: @n@px

.. figure:: @w@.jpg
   :align: center

   @W@ @w@ caption.

   @s@""",
    r""".. topic:: @W@

   @s@

.. sidebar:: @W@

   @s@

.. container:: @w@

   @s@""",
    r""".. raw:: html

   <b>@w@</b>

.. raw:: latex

   \newpage""",
    r"""@W@ |@v@| and |@v@-img|.

.. |@v@| replace:: @W@ @w@
.. |@v@-img| image:: @w@.png""",
    r""".. @s@

..
   @s@
   @s@""",
    r"""@s@

----------

@s@""",
    r""".. rubric:: @W@ @w@

.. epigraph::

   @s@

.. highlights::

   @s@""",
    r""">>> @w@ = @n@
>>> print(@w@)
@n@""",
    r""".. role:: @v@

@W@ :@v@:`@w@` and :unknownrole:`@w@`.""",
    r""".. @v@unknown:: @w@

   @s@""",
    r""".. include:: @w@-missing.rst""",
    r""".. contents:: @W@
   :depth: 2""",
    r"""@p@ @p@ @p@""",
    r"""@W@
@u:=@

@s@

@W@
@u:=@

@s@""",
)

RST_HEADERS = (
    r"""@u:=@
@W@ @w@
@u:=@

@u:-@
@W@
@u:-@

:Author: @W@ @W@
:Date: 2024-0@d@-1@d@
:Version: @d@.@n@""",
    r""":Author: @W@
:Organization: @W@ @w@
:Abstract: @s@""",
)

ORG = (
    r"""* @W@ @w@

@p@""",
    r"""** TODO [#A] @W@ @w@ :@w@:@w@:
SCHEDULED: <2024-0@d@-1@d@ Mon>

@s@

*** DONE @W@
CLOSED: [2024-01-0@d@ Tue 10:00]""",
    r"""@W@ *@w@* /@w@/ _@w@_ =@w@= ~@w@~ +@w@+ @w@^{@d@} @w@_{@d@} @w@.""",
    r"""@W@ [[@url@][@w@ @w@]] and [[@url@]] and [[#@ref@][@w@]] and <<@v@>> and [[@v@]].""",
    r"""[[file:@w@.png]]

#+CAPTION: @W@ @w@
#+NAME: fig:@id@
[[./@w@.jpg]]""",
    r"""@W@[fn:@id@] and @w@[fn::@s@].

[fn:@id@] @s@""",
    r"""- @s@
- @W@
  - @w@
  - @w@
- @w@ @w@

1. @w@
2. @w@

1) @w@
2) @w@""",
    r"""- [X] @W@
- [ ] @w@
- [-] @w@""",
    r"""- @W@ :: @s@
- @W@ @w@ :: @w@""",
    r"""#+BEGIN_SRC @lang@ :exports code
@w@ = @n@
print(@w@)
#+END_SRC

#+begin_src @lang@ -n
return @w@
#+end_src""",
    r"""#+BEGIN_EXAMPLE
@w@  @w@
   @n@
#+END_EXAMPLE

: @w@ fixed width
: @w@""",
    r"""#+BEGIN_QUOTE
@s@
#+END_QUOTE

#+BEGIN_VERSE
@W@ @w@
   @w@ @w@
#+END_VERSE""",
    r"""#+BEGIN_CENTER
@s@
#+END_CENTER

#+BEGIN_EXPORT html
<b>@w@</b>
#+END_EXPORT

#+HTML: <i>@w@</i>
#+LATEX: \newpage""",
    r"""@table:org@""",
    r"""#+CAPTION: @W@ @w@
#+NAME: tab:@id@
| <l> | <r> |
| @W@ | @W@ |
|-----+-----|
| @w@ | @n@ |
| @w@ | @d@ |""",
    r"""@W@ \alpha \beta and $x^@d@$ and \(y_@d@\) and \[ z = @n@ \]

\begin{equation}
a = b + @n@
\end{equation}""",
    r"""* @W@
:PROPERTIES:
:CUSTOM_ID: @ref@
:@W@: @w@
:END:

@s@""",
    r"""@s@

-----

@s@""",
    r"""#+INCLUDE: "@w@-missing.org"

@s@""",
    r"""# @s@

#+BEGIN_COMMENT
@s@
#+END_COMMENT

@w@""",
    r"""#+MACRO: @v@ @W@ $1
{{{@v@(@w@)}}} @s@""",
    r"""@W@ \\- @w@ -- @w@ --- @w@... "@w@" '@w@'""",
    r"""@W@ @@html:<b>@@@w@@@html:</b>@@ and src_python{@n@ + @d@} and call_@w@().""",
    r"""* COMMENT @W@
@s@

* @W@ :noexport:
@s@

* @W@ @w@
@s@""",
    r"""@p@ @p@ @p@""",
    r"""* @W@
** @W@
*** @W@
**** @W@
@s@""",
    r"""@W@ <<<@v@>>> and @v@.

#+ATTR_HTML: :width @n@%
[[file:@w@.svg]]""",
)

ORG_HEADERS = (
    r"""#+TITLE: @W@ @w@
#+AUTHOR: @W@ @W@
#+DATE: 2024-0@d@-1@d@
#+EMAIL: @w@@example.org
#+LANGUAGE: en
#+OPTIONS: toc:nil num:t""",
    r"""#+title: @W@
#+subtitle: @W@ @w@
#+keywords: @w@ @w@""",
)

# The snippets and the optional header of each input format of the generator.
BANKS = {
    "markdown": (MARKDOWN, MARKDOWN_HEADERS),
    "html": (HTML, HTML_HEADERS),
    "latex": (LATEX, LATEX_HEADERS),
    "rst": (RST, RST_HEADERS),
    "org": (ORG, ORG_HEADERS),
}
