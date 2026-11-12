# Work list of the LaTeX reader

Made by `tools/latex_diff.py table` from the probe results. Do not edit.
The order is the estimated frequency in real documents. A state
counts the probe contexts that give the bytes of Pandoc 3.13:
equal (all), not started (not all, and no string of src/parse.rs
names it), partial (some), different (none).

Counts: equal 162, partial 68, different 4, not started 117, not probed 0

| rank | name | state | contexts equal |
| --- | --- | --- | --- |
| 1 | `\section` | partial | 6/8 |
| 2 | `\emph` | equal | 8/8 |
| 3 | `\label` | partial | 6/8 |
| 4 | `\ref` | partial | 6/8 |
| 5 | `\cite` | equal | 8/8 |
| 6 | `\textbf` | equal | 8/8 |
| 7 | `\item` | partial | 4/8 |
| 8 | `{itemize}` | partial | 5/8 |
| 9 | `\subsection` | partial | 6/8 |
| 10 | `\begin` | different | 0/8 |
| 11 | `\end` | different | 0/8 |
| 12 | `\usepackage` | not started | 5/8 |
| 13 | `\includegraphics` | equal | 8/8 |
| 14 | `\caption` | partial | 5/8 |
| 15 | `{figure}` | different | 0/8 |
| 16 | `\centering` | partial | 6/8 |
| 17 | `\textit` | equal | 8/8 |
| 18 | `{enumerate}` | partial | 5/8 |
| 19 | `\footnote` | equal | 8/8 |
| 20 | `{equation}` | equal | 8/8 |
| 21 | `\hline` | equal | 8/8 |
| 22 | `{tabular}` | different | 0/8 |
| 23 | `{table}` | partial | 6/8 |
| 24 | `\documentclass` | partial | 4/8 |
| 25 | `{document}` | partial | 5/8 |
| 26 | `\newcommand` | partial | 7/8 |
| 27 | `\url` | partial | 7/8 |
| 28 | `\href` | partial | 7/8 |
| 29 | `\texttt` | equal | 8/8 |
| 30 | `\subsubsection` | partial | 6/8 |
| 31 | `\title` | partial | 4/8 |
| 32 | `\author` | partial | 4/8 |
| 33 | `\date` | partial | 4/8 |
| 34 | `\maketitle` | partial | 4/8 |
| 35 | `{abstract}` | partial | 6/8 |
| 36 | `{align}` | equal | 8/8 |
| 37 | `{align*}` | equal | 8/8 |
| 38 | `{equation*}` | equal | 8/8 |
| 39 | `\eqref` | partial | 6/8 |
| 40 | `\textwidth` | equal | 8/8 |
| 41 | `\linewidth` | equal | 8/8 |
| 42 | `\vspace` | equal | 8/8 |
| 43 | `\hspace` | equal | 8/8 |
| 44 | `\noindent` | partial | 6/8 |
| 45 | `\paragraph` | partial | 6/8 |
| 46 | `\chapter` | partial | 6/8 |
| 47 | `{description}` | partial | 4/8 |
| 48 | `{quote}` | partial | 6/8 |
| 49 | `{center}` | partial | 5/8 |
| 50 | `{verbatim}` | partial | 6/8 |
| 51 | `\toprule` | equal | 8/8 |
| 52 | `\midrule` | equal | 8/8 |
| 53 | `\bottomrule` | equal | 8/8 |
| 54 | `\multicolumn` | partial | 7/8 |
| 55 | `\citep` | equal | 8/8 |
| 56 | `\citet` | equal | 8/8 |
| 57 | `\\` | partial | 7/8 |
| 58 | `\,` | equal | 8/8 |
| 59 | `\ldots` | equal | 8/8 |
| 60 | `\dots` | equal | 8/8 |
| 61 | `\quad` | equal | 8/8 |
| 62 | `\qquad` | equal | 8/8 |
| 63 | `\thanks` | not started | 0/8 |
| 64 | `\tableofcontents` | equal | 8/8 |
| 65 | `\newpage` | equal | 8/8 |
| 66 | `\clearpage` | equal | 8/8 |
| 67 | `\bibliography` | not started | 0/8 |
| 68 | `\bibliographystyle` | not started | 5/8 |
| 69 | `\bibitem` | equal | 8/8 |
| 70 | `{thebibliography}` | not started | 6/8 |
| 71 | `\newblock` | equal | 8/8 |
| 72 | `\input` | partial | 5/8 |
| 73 | `\include` | partial | 3/8 |
| 74 | `\textsc` | equal | 8/8 |
| 75 | `\underline` | equal | 8/8 |
| 76 | `\small` | not started | 0/8 |
| 77 | `\footnotesize` | not started | 0/8 |
| 78 | `\large` | not started | 0/8 |
| 79 | `\Large` | not started | 0/8 |
| 80 | `\tiny` | not started | 0/8 |
| 81 | `\scriptsize` | not started | 0/8 |
| 82 | `\normalsize` | not started | 0/8 |
| 83 | `\huge` | not started | 0/8 |
| 84 | `\LARGE` | not started | 0/8 |
| 85 | `\Huge` | not started | 0/8 |
| 86 | `\bf` | partial | 6/8 |
| 87 | `\it` | partial | 6/8 |
| 88 | `\em` | partial | 6/8 |
| 89 | `\tt` | not started | 0/8 |
| 90 | `\rm` | not started | 0/8 |
| 91 | `\sc` | equal | 8/8 |
| 92 | `\sf` | equal | 8/8 |
| 93 | `\sl` | partial | 6/8 |
| 94 | `\bfseries` | partial | 6/8 |
| 95 | `\itshape` | partial | 6/8 |
| 96 | `\ttfamily` | not started | 0/8 |
| 97 | `\scshape` | not started | 0/8 |
| 98 | `\sffamily` | equal | 8/8 |
| 99 | `\rmfamily` | equal | 8/8 |
| 100 | `\mdseries` | equal | 8/8 |
| 101 | `\upshape` | equal | 8/8 |
| 102 | `\normalfont` | equal | 8/8 |
| 103 | `\textsf` | equal | 8/8 |
| 104 | `\textrm` | equal | 8/8 |
| 105 | `\textsl` | equal | 8/8 |
| 106 | `\textup` | not started | 0/8 |
| 107 | `\textmd` | not started | 0/8 |
| 108 | `\textnormal` | not started | 0/8 |
| 109 | `\textsuperscript` | equal | 8/8 |
| 110 | `\textsubscript` | equal | 8/8 |
| 111 | `\mbox` | equal | 8/8 |
| 112 | `\hbox` | equal | 8/8 |
| 113 | `\makebox` | equal | 8/8 |
| 114 | `\fbox` | equal | 8/8 |
| 115 | `\framebox` | equal | 8/8 |
| 116 | `\parbox` | not started | 0/8 |
| 117 | `\raisebox` | equal | 8/8 |
| 118 | `\vbox` | not started | 1/8 |
| 119 | `\phantom` | equal | 8/8 |
| 120 | `\hfill` | not started | 0/8 |
| 121 | `\vfill` | not started | 0/8 |
| 122 | `\smallskip` | equal | 8/8 |
| 123 | `\medskip` | equal | 8/8 |
| 124 | `\bigskip` | equal | 8/8 |
| 125 | `\newline` | not started | 0/8 |
| 126 | `\linebreak` | equal | 8/8 |
| 127 | `\pagebreak` | equal | 8/8 |
| 128 | `\cleardoublepage` | equal | 8/8 |
| 129 | `\indent` | equal | 8/8 |
| 130 | `\par` | partial | 3/8 |
| 131 | `\raggedright` | not started | 0/8 |
| 132 | `\raggedleft` | equal | 8/8 |
| 133 | `\enspace` | equal | 8/8 |
| 134 | `\thinspace` | equal | 8/8 |
| 135 | `\space` | equal | 8/8 |
| 136 | `\;` | not started | 0/8 |
| 137 | `\:` | not started | 0/8 |
| 138 | `\!` | not started | 0/8 |
| 139 | `\ ` | equal | 8/8 |
| 140 | `\LaTeX` | equal | 8/8 |
| 141 | `\TeX` | equal | 8/8 |
| 142 | `\LaTeXe` | equal | 8/8 |
| 143 | `\today` | equal | 8/8 |
| 144 | `\%` | equal | 8/8 |
| 145 | `\&` | equal | 8/8 |
| 146 | `\$` | equal | 8/8 |
| 147 | `\_` | equal | 8/8 |
| 148 | `\{` | equal | 8/8 |
| 149 | `\}` | equal | 8/8 |
| 150 | `\-` | equal | 8/8 |
| 151 | `\'` | equal | 8/8 |
| 152 | `\`` | equal | 8/8 |
| 153 | `\^` | equal | 8/8 |
| 154 | `\"` | equal | 8/8 |
| 155 | `\~` | equal | 8/8 |
| 156 | `\=` | equal | 8/8 |
| 157 | `\.` | equal | 8/8 |
| 158 | `\c` | equal | 8/8 |
| 159 | `\v` | equal | 8/8 |
| 160 | `\u` | equal | 8/8 |
| 161 | `\H` | equal | 8/8 |
| 162 | `\r` | equal | 8/8 |
| 163 | `\k` | equal | 8/8 |
| 164 | `\d` | equal | 8/8 |
| 165 | `\b` | equal | 8/8 |
| 166 | `\t` | not started | 0/8 |
| 167 | `\ss` | equal | 8/8 |
| 168 | `\ae` | equal | 8/8 |
| 169 | `\oe` | equal | 8/8 |
| 170 | `\o` | equal | 8/8 |
| 171 | `\l` | equal | 8/8 |
| 172 | `\i` | equal | 8/8 |
| 173 | `\aa` | equal | 8/8 |
| 174 | `\AA` | equal | 8/8 |
| 175 | `\copyright` | not started | 0/8 |
| 176 | `\textcopyright` | not started | 0/8 |
| 177 | `\textregistered` | not started | 0/8 |
| 178 | `\texttrademark` | equal | 8/8 |
| 179 | `\textdegree` | not started | 0/8 |
| 180 | `\textbackslash` | equal | 8/8 |
| 181 | `\textasciitilde` | not started | 0/8 |
| 182 | `\textasciicircum` | not started | 0/8 |
| 183 | `\textbar` | equal | 8/8 |
| 184 | `\textless` | not started | 0/8 |
| 185 | `\textgreater` | not started | 0/8 |
| 186 | `\textendash` | equal | 8/8 |
| 187 | `\textemdash` | equal | 8/8 |
| 188 | `\textquoteleft` | not started | 0/8 |
| 189 | `\textquoteright` | not started | 0/8 |
| 190 | `\textquotedblleft` | not started | 0/8 |
| 191 | `\textquotedblright` | not started | 0/8 |
| 192 | `\guillemotleft` | not started | 0/8 |
| 193 | `\guillemotright` | not started | 0/8 |
| 194 | `\S` | not started | 0/8 |
| 195 | `\P` | not started | 0/8 |
| 196 | `\dag` | equal | 8/8 |
| 197 | `\ddag` | equal | 8/8 |
| 198 | `\pounds` | not started | 0/8 |
| 199 | `\euro` | not started | 0/8 |
| 200 | `\textbullet` | not started | 0/8 |
| 201 | `\textperiodcentered` | equal | 8/8 |
| 202 | `\slash` | not started | 0/8 |
| 203 | `\char` | partial | 7/8 |
| 204 | `\symbol` | equal | 8/8 |
| 205 | `\part` | partial | 6/8 |
| 206 | `\subparagraph` | partial | 6/8 |
| 207 | `\appendix` | equal | 8/8 |
| 208 | `\frontmatter` | equal | 8/8 |
| 209 | `\mainmatter` | equal | 8/8 |
| 210 | `\backmatter` | equal | 8/8 |
| 211 | `\listoffigures` | not started | 5/8 |
| 212 | `\listoftables` | not started | 5/8 |
| 213 | `\footnotemark` | not started | 0/8 |
| 214 | `\footnotetext` | not started | 7/8 |
| 215 | `\marginpar` | equal | 8/8 |
| 216 | `\pageref` | equal | 8/8 |
| 217 | `\autoref` | partial | 6/8 |
| 218 | `\cref` | partial | 6/8 |
| 219 | `\Cref` | partial | 6/8 |
| 220 | `\nameref` | equal | 8/8 |
| 221 | `\hyperref` | not started | 0/8 |
| 222 | `\hypertarget` | not started | 0/8 |
| 223 | `\hyperlink` | not started | 0/8 |
| 224 | `\nolinkurl` | not started | 0/8 |
| 225 | `\captionof` | equal | 8/8 |
| 226 | `\printbibliography` | equal | 8/8 |
| 227 | `\addbibresource` | not started | 0/8 |
| 228 | `\nocite` | equal | 8/8 |
| 229 | `\citealp` | equal | 8/8 |
| 230 | `\citeauthor` | equal | 8/8 |
| 231 | `\citeyear` | equal | 8/8 |
| 232 | `\parencite` | equal | 8/8 |
| 233 | `\textcite` | equal | 8/8 |
| 234 | `\autocite` | equal | 8/8 |
| 235 | `\footcite` | equal | 8/8 |
| 236 | `\cites` | partial | 7/8 |
| 237 | `\renewcommand` | partial | 7/8 |
| 238 | `\providecommand` | partial | 7/8 |
| 239 | `\def` | partial | 6/8 |
| 240 | `\let` | partial | 6/8 |
| 241 | `\newenvironment` | partial | 7/8 |
| 242 | `\DeclareMathOperator` | not started | 6/8 |
| 243 | `\newtheorem` | not started | 4/8 |
| 244 | `\theoremstyle` | not started | 4/8 |
| 245 | `\newcounter` | equal | 8/8 |
| 246 | `\setcounter` | equal | 8/8 |
| 247 | `\addtocounter` | not started | 5/8 |
| 248 | `\stepcounter` | equal | 8/8 |
| 249 | `\setlength` | equal | 8/8 |
| 250 | `\addtolength` | equal | 8/8 |
| 251 | `\newlength` | equal | 8/8 |
| 252 | `\pagestyle` | equal | 8/8 |
| 253 | `\thispagestyle` | equal | 8/8 |
| 254 | `\graphicspath` | not started | 4/8 |
| 255 | `\hypersetup` | equal | 8/8 |
| 256 | `\geometry` | equal | 8/8 |
| 257 | `\makeatletter` | equal | 8/8 |
| 258 | `\makeatother` | equal | 8/8 |
| 259 | `\protect` | equal | 8/8 |
| 260 | `\relax` | equal | 8/8 |
| 261 | `\ifx` | equal | 8/8 |
| 262 | `\iftrue` | equal | 8/8 |
| 263 | `\iffalse` | equal | 8/8 |
| 264 | `\else` | equal | 8/8 |
| 265 | `\fi` | equal | 8/8 |
| 266 | `\newif` | equal | 8/8 |
| 267 | `\the` | equal | 8/8 |
| 268 | `\arabic` | equal | 8/8 |
| 269 | `\roman` | equal | 8/8 |
| 270 | `\number` | equal | 8/8 |
| 271 | `\vskip` | not started | 0/8 |
| 272 | `\hskip` | not started | 0/8 |
| 273 | `\kern` | not started | 0/8 |
| 274 | `\expandafter` | not started | 0/8 |
| 275 | `\csname` | equal | 8/8 |
| 276 | `\cline` | partial | 7/8 |
| 277 | `\cmidrule` | partial | 7/8 |
| 278 | `\addlinespace` | equal | 8/8 |
| 279 | `\multirow` | not started | 7/8 |
| 280 | `\tabularnewline` | not started | 7/8 |
| 281 | `\textcolor` | not started | 0/8 |
| 282 | `\color` | equal | 8/8 |
| 283 | `\colorbox` | not started | 0/8 |
| 284 | `\definecolor` | equal | 8/8 |
| 285 | `\enquote` | partial | 7/8 |
| 286 | `\foreignquote` | not started | 0/8 |
| 287 | `\texorpdfstring` | equal | 8/8 |
| 288 | `\SI` | not started | 0/8 |
| 289 | `\si` | not started | 0/8 |
| 290 | `\num` | not started | 0/8 |
| 291 | `\qty` | not started | 0/8 |
| 292 | `\ang` | not started | 0/8 |
| 293 | `\lstinline` | not started | 0/8 |
| 294 | `\verb` | partial | 7/8 |
| 295 | `\mintinline` | not started | 0/8 |
| 296 | `\MakeUppercase` | not started | 0/8 |
| 297 | `\MakeLowercase` | not started | 0/8 |
| 298 | `\uppercase` | not started | 0/8 |
| 299 | `\lowercase` | not started | 0/8 |
| 300 | `\ensuremath` | not started | 0/8 |
| 301 | `\sout` | equal | 8/8 |
| 302 | `\uline` | equal | 8/8 |
| 303 | `\todo` | equal | 8/8 |
| 304 | `\xspace` | not started | 0/8 |
| 305 | `\gls` | not started | 0/8 |
| 306 | `\ac` | not started | 0/8 |
| 307 | `{flushleft}` | not started | 6/8 |
| 308 | `{flushright}` | not started | 6/8 |
| 309 | `{minipage}` | partial | 6/8 |
| 310 | `{quotation}` | partial | 6/8 |
| 311 | `{verse}` | partial | 6/8 |
| 312 | `{Verbatim}` | partial | 5/8 |
| 313 | `{lstlisting}` | partial | 5/8 |
| 314 | `{minted}` | partial | 5/8 |
| 315 | `{comment}` | partial | 7/8 |
| 316 | `{figure*}` | partial | 1/8 |
| 317 | `{table*}` | partial | 1/8 |
| 318 | `{tabular*}` | partial | 1/8 |
| 319 | `{tabularx}` | partial | 1/8 |
| 320 | `{longtable}` | partial | 1/8 |
| 321 | `{array}` | not started | 6/8 |
| 322 | `{gather}` | equal | 8/8 |
| 323 | `{gather*}` | equal | 8/8 |
| 324 | `{multline}` | equal | 8/8 |
| 325 | `{eqnarray}` | equal | 8/8 |
| 326 | `{split}` | not started | 6/8 |
| 327 | `{cases}` | not started | 6/8 |
| 328 | `{pmatrix}` | not started | 6/8 |
| 329 | `{bmatrix}` | not started | 6/8 |
| 330 | `{subequations}` | not started | 1/8 |
| 331 | `{theorem}` | not started | 6/8 |
| 332 | `{lemma}` | not started | 6/8 |
| 333 | `{proof}` | not started | 1/8 |
| 334 | `{definition}` | not started | 6/8 |
| 335 | `{corollary}` | not started | 6/8 |
| 336 | `{proposition}` | not started | 6/8 |
| 337 | `{remark}` | not started | 6/8 |
| 338 | `{example}` | not started | 6/8 |
| 339 | `{subfigure}` | not started | 1/8 |
| 340 | `{subtable}` | not started | 6/8 |
| 341 | `{wrapfigure}` | not started | 6/8 |
| 342 | `{sidewaystable}` | not started | 6/8 |
| 343 | `{landscape}` | not started | 6/8 |
| 344 | `{titlepage}` | not started | 6/8 |
| 345 | `{appendices}` | not started | 6/8 |
| 346 | `{multicols}` | not started | 6/8 |
| 347 | `{alltt}` | not started | 1/8 |
| 348 | `{displaymath}` | equal | 8/8 |
| 349 | `{math}` | equal | 8/8 |
| 350 | `{tikzpicture}` | not started | 3/8 |
| 351 | `{otherlanguage}` | not started | 1/8 |
