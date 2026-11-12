//! Writer options, checked against the oracle.
//!
//! Each case is a document tree from `pandoc -t json`, the options and the
//! output of `pandoc -f json -t FORMAT` with the same options.

use randoc_ast::json::from_json;
use randoc_core::{
    CaptionPosition, CiteMethod, Context, TopLevelDivision, WrapOption, WriterOptions,
    default_extensions,
};

fn options() -> WriterOptions {
    let mut options = WriterOptions::default();
    options.extensions = default_extensions("latex");
    options
}

#[test]
fn natbib() {
    let doc = from_json(r###"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Cite","c":[[{"citationId":"doe99","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"AuthorInText"},"citationNoteNum":1,"citationHash":0}],[{"t":"Str","c":"@doe99"}]]},{"t":"Space"},{"t":"Str","c":"says"},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"a","citationPrefix":[],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"p. 3"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":2,"citationHash":0},{"citationId":"b","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"SuppressAuthor"},"citationNoteNum":2,"citationHash":0}],[{"t":"Str","c":"[@a,"},{"t":"Space"},{"t":"Str","c":"p."},{"t":"Space"},{"t":"Str","c":"3;"},{"t":"Space"},{"t":"Str","c":"-@b]"}]]},{"t":"Space"},{"t":"Str","c":"and"},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"c","citationPrefix":[{"t":"Str","c":"see"}],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"ch. 2"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":3,"citationHash":0}],[{"t":"Str","c":"[see"},{"t":"Space"},{"t":"Str","c":"@c,"},{"t":"Space"},{"t":"Str","c":"ch."},{"t":"Space"},{"t":"Str","c":"2]"}]]},{"t":"Space"},{"t":"Str","c":"and"},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"d","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"SuppressAuthor"},"citationNoteNum":4,"citationHash":0}],[{"t":"Str","c":"[-@d]"}]]},{"t":"Space"},{"t":"Str","c":"and"},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"e","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":5,"citationHash":0}],[{"t":"Str","c":"[@e]"}]]},{"t":"Space"},{"t":"Str","c":"and"},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"f","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":6,"citationHash":0},{"citationId":"g","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":6,"citationHash":0}],[{"t":"Str","c":"[@f;"},{"t":"Space"},{"t":"Str","c":"@g]"}]]}]},{"t":"Para","c":[{"t":"Cite","c":[[{"citationId":"a","citationPrefix":[],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"pp. 3-4"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":7,"citationHash":0}],[{"t":"Str","c":"[@a,"},{"t":"Space"},{"t":"Str","c":"pp."},{"t":"Space"},{"t":"Str","c":"3-4]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"b","citationPrefix":[],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"page"},{"t":"Space"},{"t":"Str","c":"3"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":8,"citationHash":0}],[{"t":"Str","c":"[@b,"},{"t":"Space"},{"t":"Str","c":"page"},{"t":"Space"},{"t":"Str","c":"3]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"c","citationPrefix":[],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"p. 5,"},{"t":"Space"},{"t":"Str","c":"note"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":9,"citationHash":0}],[{"t":"Str","c":"[@c,"},{"t":"Space"},{"t":"Str","c":"p."},{"t":"Space"},{"t":"Str","c":"5,"},{"t":"Space"},{"t":"Str","c":"note]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"d","citationPrefix":[],"citationSuffix":[{"t":"Space"},{"t":"Str","c":"p. 6"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":10,"citationHash":0}],[{"t":"Str","c":"[@d"},{"t":"Space"},{"t":"Str","c":"p."},{"t":"Space"},{"t":"Str","c":"6]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"e","citationPrefix":[],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"{p. 7}"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":11,"citationHash":0}],[{"t":"Str","c":"[@e,"},{"t":"Space"},{"t":"Str","c":"{p."},{"t":"Space"},{"t":"Str","c":"7}]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"f","citationPrefix":[],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"8"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":12,"citationHash":0}],[{"t":"Str","c":"[@f,"},{"t":"Space"},{"t":"Str","c":"8]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"g","citationPrefix":[],"citationSuffix":[{"t":"Str","c":"p. 9"}],"citationMode":{"t":"AuthorInText"},"citationNoteNum":13,"citationHash":0}],[{"t":"Str","c":"@g"},{"t":"Space"},{"t":"Str","c":"[p."},{"t":"Space"},{"t":"Str","c":"9]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"h","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":14,"citationHash":0},{"citationId":"i","citationPrefix":[],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"p. 2"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":14,"citationHash":0}],[{"t":"Str","c":"[@h;"},{"t":"Space"},{"t":"Str","c":"@i,"},{"t":"Space"},{"t":"Str","c":"p."},{"t":"Space"},{"t":"Str","c":"2]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"j","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"AuthorInText"},"citationNoteNum":15,"citationHash":0},{"citationId":"k","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":15,"citationHash":0}],[{"t":"Str","c":"@j"},{"t":"Space"},{"t":"Str","c":"[@k]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"l","citationPrefix":[],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"p. 3"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":16,"citationHash":0}],[{"t":"Str","c":"[@l,"},{"t":"Space"},{"t":"Str","c":"p.\\"},{"t":"Space"},{"t":"Str","c":"3]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"m","citationPrefix":[{"t":"Emph","c":[{"t":"Str","c":"see"}]}],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"p. 4"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":17,"citationHash":0}],[{"t":"Str","c":"[*see*"},{"t":"Space"},{"t":"Str","c":"@m,"},{"t":"Space"},{"t":"Str","c":"p."},{"t":"Space"},{"t":"Str","c":"4]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"n","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":18,"citationHash":0},{"citationId":"o","citationPrefix":[],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"2"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":18,"citationHash":0},{"citationId":"p","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":18,"citationHash":0}],[{"t":"Str","c":"[@n;"},{"t":"Space"},{"t":"Str","c":"@o,"},{"t":"Space"},{"t":"Str","c":"2;"},{"t":"Space"},{"t":"Str","c":"@p]"}]]}]}]}"###).unwrap();
    let mut options = options();
    options.cite_method = CiteMethod::Natbib;
    let text = crate::write_latex(&doc, &options, &mut Context::pure()).unwrap();
    assert_eq!(
        text,
        r###"\citet{doe99} says \citetext{\citealp[p.~3]{a}; \citeyear{b}} and
\citep[see][ch.~2]{c} and \citeyearpar{d} and \citep{e} and \citep{f, g}

\citep[pp.~3-4]{a} \citep[page 3]{b} \citep[p.~5, note]{c} \citep[
p.~6]{d} \citep[p.~7]{e} \citep[8]{f} \citet[p.~9]{g} \citep[p.~2]{h, i}
\citeauthor{j} \citetext{\citeyear{j}; \citealp{k}} \citep[p.~3]{l}
\citep[\emph{see}][p.~4]{m}
\citetext{\citealp{n}; \citealp[2]{o}; \citealp{p}}"###
    );
}

#[test]
fn biblatex() {
    let doc = from_json(r###"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Cite","c":[[{"citationId":"doe99","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"AuthorInText"},"citationNoteNum":1,"citationHash":0}],[{"t":"Str","c":"@doe99"}]]},{"t":"Space"},{"t":"Str","c":"says"},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"a","citationPrefix":[],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"p. 3"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":2,"citationHash":0},{"citationId":"b","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"SuppressAuthor"},"citationNoteNum":2,"citationHash":0}],[{"t":"Str","c":"[@a,"},{"t":"Space"},{"t":"Str","c":"p."},{"t":"Space"},{"t":"Str","c":"3;"},{"t":"Space"},{"t":"Str","c":"-@b]"}]]},{"t":"Space"},{"t":"Str","c":"and"},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"c","citationPrefix":[{"t":"Str","c":"see"}],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"ch. 2"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":3,"citationHash":0}],[{"t":"Str","c":"[see"},{"t":"Space"},{"t":"Str","c":"@c,"},{"t":"Space"},{"t":"Str","c":"ch."},{"t":"Space"},{"t":"Str","c":"2]"}]]},{"t":"Space"},{"t":"Str","c":"and"},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"d","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"SuppressAuthor"},"citationNoteNum":4,"citationHash":0}],[{"t":"Str","c":"[-@d]"}]]},{"t":"Space"},{"t":"Str","c":"and"},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"e","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":5,"citationHash":0}],[{"t":"Str","c":"[@e]"}]]},{"t":"Space"},{"t":"Str","c":"and"},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"f","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":6,"citationHash":0},{"citationId":"g","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":6,"citationHash":0}],[{"t":"Str","c":"[@f;"},{"t":"Space"},{"t":"Str","c":"@g]"}]]}]},{"t":"Para","c":[{"t":"Cite","c":[[{"citationId":"a","citationPrefix":[],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"pp. 3-4"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":7,"citationHash":0}],[{"t":"Str","c":"[@a,"},{"t":"Space"},{"t":"Str","c":"pp."},{"t":"Space"},{"t":"Str","c":"3-4]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"b","citationPrefix":[],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"page"},{"t":"Space"},{"t":"Str","c":"3"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":8,"citationHash":0}],[{"t":"Str","c":"[@b,"},{"t":"Space"},{"t":"Str","c":"page"},{"t":"Space"},{"t":"Str","c":"3]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"c","citationPrefix":[],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"p. 5,"},{"t":"Space"},{"t":"Str","c":"note"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":9,"citationHash":0}],[{"t":"Str","c":"[@c,"},{"t":"Space"},{"t":"Str","c":"p."},{"t":"Space"},{"t":"Str","c":"5,"},{"t":"Space"},{"t":"Str","c":"note]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"d","citationPrefix":[],"citationSuffix":[{"t":"Space"},{"t":"Str","c":"p. 6"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":10,"citationHash":0}],[{"t":"Str","c":"[@d"},{"t":"Space"},{"t":"Str","c":"p."},{"t":"Space"},{"t":"Str","c":"6]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"e","citationPrefix":[],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"{p. 7}"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":11,"citationHash":0}],[{"t":"Str","c":"[@e,"},{"t":"Space"},{"t":"Str","c":"{p."},{"t":"Space"},{"t":"Str","c":"7}]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"f","citationPrefix":[],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"8"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":12,"citationHash":0}],[{"t":"Str","c":"[@f,"},{"t":"Space"},{"t":"Str","c":"8]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"g","citationPrefix":[],"citationSuffix":[{"t":"Str","c":"p. 9"}],"citationMode":{"t":"AuthorInText"},"citationNoteNum":13,"citationHash":0}],[{"t":"Str","c":"@g"},{"t":"Space"},{"t":"Str","c":"[p."},{"t":"Space"},{"t":"Str","c":"9]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"h","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":14,"citationHash":0},{"citationId":"i","citationPrefix":[],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"p. 2"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":14,"citationHash":0}],[{"t":"Str","c":"[@h;"},{"t":"Space"},{"t":"Str","c":"@i,"},{"t":"Space"},{"t":"Str","c":"p."},{"t":"Space"},{"t":"Str","c":"2]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"j","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"AuthorInText"},"citationNoteNum":15,"citationHash":0},{"citationId":"k","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":15,"citationHash":0}],[{"t":"Str","c":"@j"},{"t":"Space"},{"t":"Str","c":"[@k]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"l","citationPrefix":[],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"p. 3"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":16,"citationHash":0}],[{"t":"Str","c":"[@l,"},{"t":"Space"},{"t":"Str","c":"p.\\"},{"t":"Space"},{"t":"Str","c":"3]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"m","citationPrefix":[{"t":"Emph","c":[{"t":"Str","c":"see"}]}],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"p. 4"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":17,"citationHash":0}],[{"t":"Str","c":"[*see*"},{"t":"Space"},{"t":"Str","c":"@m,"},{"t":"Space"},{"t":"Str","c":"p."},{"t":"Space"},{"t":"Str","c":"4]"}]]},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"n","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":18,"citationHash":0},{"citationId":"o","citationPrefix":[],"citationSuffix":[{"t":"Str","c":","},{"t":"Space"},{"t":"Str","c":"2"}],"citationMode":{"t":"NormalCitation"},"citationNoteNum":18,"citationHash":0},{"citationId":"p","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":18,"citationHash":0}],[{"t":"Str","c":"[@n;"},{"t":"Space"},{"t":"Str","c":"@o,"},{"t":"Space"},{"t":"Str","c":"2;"},{"t":"Space"},{"t":"Str","c":"@p]"}]]}]}]}"###).unwrap();
    let mut options = options();
    options.cite_method = CiteMethod::Biblatex;
    let text = crate::write_latex(&doc, &options, &mut Context::pure()).unwrap();
    assert_eq!(
        text,
        r###"\textcite{doe99} says \autocites[3]{a}{b} and \autocite[see][ch.~2]{c}
and \autocite*{d} and \autocite{e} and \autocite{f,g}

\autocite[3-4]{a} \autocite[3]{b} \autocite[5, note]{c} \autocite[6]{d}
\autocite[7]{e} \autocite[8]{f} \textcite[9]{g} \autocites{h}[2]{i}
\textcite{j,k} \autocite[3]{l} \autocite[\emph{see}][4]{m}
\autocites{n}[2]{o}{p}"###
    );
}

#[test]
fn listings() {
    let doc = from_json(r###"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Code","c":[["",["python"],[]],"x"]},{"t":"Space"},{"t":"Str","c":"and"},{"t":"Space"},{"t":"Code","c":[["",[],[]],"a!b"]},{"t":"Space"},{"t":"Str","c":"and"},{"t":"Space"},{"t":"Code","c":[["",[],[]],"{%}"]}]},{"t":"CodeBlock","c":[["",["python"],[]],"x = 1"]},{"t":"CodeBlock","c":[["",[],[]],"plain"]},{"t":"CodeBlock","c":[["id",["haskell","numberLines"],[["startFrom","3"]]],"y"]},{"t":"Para","c":[{"t":"Str","c":"Note."},{"t":"Note","c":[{"t":"Para","c":[{"t":"Code","c":[["",[],[]],"in note"]}]}]}]}]}"###).unwrap();
    let mut options = options();
    options.listings = true;
    let text = crate::write_latex(&doc, &options, &mut Context::pure()).unwrap();
    assert_eq!(
        text,
        r###"\passthrough{\lstinline[language=Python]!x!} and
\passthrough{\lstinline"a!b"} and \passthrough{\lstinline!\{\%\}!}

\begin{lstlisting}[language=Python]
x = 1
\end{lstlisting}

\begin{lstlisting}
plain
\end{lstlisting}

\begin{lstlisting}[language=Haskell, numbers=left, firstnumber=3, label=id]
y
\end{lstlisting}

Note.\footnote{\passthrough{\lstinline!in note!}}"###
    );
}

#[test]
fn no_highlight() {
    let doc = from_json(r###"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Code","c":[["",["python"],[]],"x"]},{"t":"Space"},{"t":"Str","c":"and"},{"t":"Space"},{"t":"Code","c":[["",[],[]],"a!b"]},{"t":"Space"},{"t":"Str","c":"and"},{"t":"Space"},{"t":"Code","c":[["",[],[]],"{%}"]}]},{"t":"CodeBlock","c":[["",["python"],[]],"x = 1"]},{"t":"CodeBlock","c":[["",[],[]],"plain"]},{"t":"CodeBlock","c":[["id",["haskell","numberLines"],[["startFrom","3"]]],"y"]},{"t":"Para","c":[{"t":"Str","c":"Note."},{"t":"Note","c":[{"t":"Para","c":[{"t":"Code","c":[["",[],[]],"in note"]}]}]}]}]}"###).unwrap();
    let mut options = options();
    options.highlight_style = None;
    let text = crate::write_latex(&doc, &options, &mut Context::pure()).unwrap();
    assert_eq!(
        text,
        r###"\texttt{x} and \texttt{a!b} and \texttt{\{\%\}}

\begin{verbatim}
x = 1
\end{verbatim}

\begin{verbatim}
plain
\end{verbatim}

\protect\phantomsection\label{id}%
\begin{verbatim}
y
\end{verbatim}

Note.\footnote{\texttt{in\ note}}"###
    );
}

#[test]
fn chapter() {
    let doc = from_json(r###"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Header","c":[1,["one",[],[]],[{"t":"Str","c":"One"}]]},{"t":"Header","c":[2,["two",[],[]],[{"t":"Str","c":"Two"}]]},{"t":"Header","c":[6,["six",[],[]],[{"t":"Str","c":"Six"}]]},{"t":"Header","c":[1,["un",["unnumbered"],[]],[{"t":"Str","c":"Un"}]]}]}"###).unwrap();
    let mut options = options();
    options.top_level_division = TopLevelDivision::TopLevelChapter;
    let text = crate::write_latex(&doc, &options, &mut Context::pure()).unwrap();
    assert_eq!(
        text,
        r###"\chapter{One}\label{one}

\section{Two}\label{two}

\subparagraph{Six}\label{six}

\chapter*{Un}\label{un}
\addcontentsline{toc}{chapter}{Un}"###
    );
}

#[test]
fn part() {
    let doc = from_json(r###"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Header","c":[1,["one",[],[]],[{"t":"Str","c":"One"}]]},{"t":"Header","c":[2,["two",[],[]],[{"t":"Str","c":"Two"}]]},{"t":"Header","c":[6,["six",[],[]],[{"t":"Str","c":"Six"}]]},{"t":"Header","c":[1,["un",["unnumbered"],[]],[{"t":"Str","c":"Un"}]]}]}"###).unwrap();
    let mut options = options();
    options.top_level_division = TopLevelDivision::TopLevelPart;
    let text = crate::write_latex(&doc, &options, &mut Context::pure()).unwrap();
    assert_eq!(
        text,
        r###"\part{One}\label{one}

\chapter{Two}\label{two}

\paragraph{Six}\label{six}

\part*{Un}\label{un}
\addcontentsline{toc}{part}{Un}"###
    );
}

#[test]
fn section() {
    let doc = from_json(r###"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Header","c":[1,["one",[],[]],[{"t":"Str","c":"One"}]]},{"t":"Header","c":[2,["two",[],[]],[{"t":"Str","c":"Two"}]]},{"t":"Header","c":[6,["six",[],[]],[{"t":"Str","c":"Six"}]]},{"t":"Header","c":[1,["un",["unnumbered"],[]],[{"t":"Str","c":"Un"}]]}]}"###).unwrap();
    let mut options = options();
    options.top_level_division = TopLevelDivision::TopLevelSection;
    let text = crate::write_latex(&doc, &options, &mut Context::pure()).unwrap();
    assert_eq!(
        text,
        r###"\section{One}\label{one}

\subsection{Two}\label{two}

Six

\section*{Un}\label{un}
\addcontentsline{toc}{section}{Un}"###
    );
}

#[test]
fn wrap_none() {
    let doc = from_json(r###"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Str","c":"A"},{"t":"Space"},{"t":"Str","c":"paragraph"},{"t":"Space"},{"t":"Str","c":"with"},{"t":"Space"},{"t":"Str","c":"enough"},{"t":"Space"},{"t":"Str","c":"words"},{"t":"Space"},{"t":"Str","c":"to"},{"t":"Space"},{"t":"Str","c":"wrap"},{"t":"Space"},{"t":"Str","c":"at"},{"t":"Space"},{"t":"Str","c":"a"},{"t":"Space"},{"t":"Str","c":"narrow"},{"t":"Space"},{"t":"Str","c":"width,"},{"t":"Space"},{"t":"Str","c":"and"},{"t":"Space"},{"t":"Str","c":"a"},{"t":"SoftBreak"},{"t":"Str","c":"soft"},{"t":"Space"},{"t":"Str","c":"break"},{"t":"Space"},{"t":"Str","c":"here."},{"t":"Space"},{"t":"Str","c":"Another"},{"t":"Space"},{"t":"Emph","c":[{"t":"Str","c":"sentence"}]},{"t":"Space"},{"t":"Str","c":"follows."}]},{"t":"BulletList","c":[[{"t":"Plain","c":[{"t":"Str","c":"item"},{"t":"Space"},{"t":"Str","c":"with"},{"t":"Space"},{"t":"Str","c":"words"},{"t":"Space"},{"t":"Str","c":"that"},{"t":"Space"},{"t":"Str","c":"wrap"}]}]]},{"t":"LineBlock","c":[[{"t":"Str","c":"line"}],[{"t":"Str","c":" block"}]]}]}"###).unwrap();
    let mut options = options();
    options.wrap_text = WrapOption::WrapNone;
    let text = crate::write_latex(&doc, &options, &mut Context::pure()).unwrap();
    assert_eq!(
        text,
        r###"A paragraph with enough words to wrap at a narrow width, and a soft break here. Another \emph{sentence} follows.

\begin{itemize}
\tightlist
\item
  item with words that wrap
\end{itemize}

line\\
\strut ~block"###
    );
}

#[test]
fn wrap_preserve() {
    let doc = from_json(r###"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Str","c":"A"},{"t":"Space"},{"t":"Str","c":"paragraph"},{"t":"Space"},{"t":"Str","c":"with"},{"t":"Space"},{"t":"Str","c":"enough"},{"t":"Space"},{"t":"Str","c":"words"},{"t":"Space"},{"t":"Str","c":"to"},{"t":"Space"},{"t":"Str","c":"wrap"},{"t":"Space"},{"t":"Str","c":"at"},{"t":"Space"},{"t":"Str","c":"a"},{"t":"Space"},{"t":"Str","c":"narrow"},{"t":"Space"},{"t":"Str","c":"width,"},{"t":"Space"},{"t":"Str","c":"and"},{"t":"Space"},{"t":"Str","c":"a"},{"t":"SoftBreak"},{"t":"Str","c":"soft"},{"t":"Space"},{"t":"Str","c":"break"},{"t":"Space"},{"t":"Str","c":"here."},{"t":"Space"},{"t":"Str","c":"Another"},{"t":"Space"},{"t":"Emph","c":[{"t":"Str","c":"sentence"}]},{"t":"Space"},{"t":"Str","c":"follows."}]},{"t":"BulletList","c":[[{"t":"Plain","c":[{"t":"Str","c":"item"},{"t":"Space"},{"t":"Str","c":"with"},{"t":"Space"},{"t":"Str","c":"words"},{"t":"Space"},{"t":"Str","c":"that"},{"t":"Space"},{"t":"Str","c":"wrap"}]}]]},{"t":"LineBlock","c":[[{"t":"Str","c":"line"}],[{"t":"Str","c":" block"}]]}]}"###).unwrap();
    let mut options = options();
    options.wrap_text = WrapOption::WrapPreserve;
    let text = crate::write_latex(&doc, &options, &mut Context::pure()).unwrap();
    assert_eq!(
        text,
        r###"A paragraph with enough words to wrap at a narrow width, and a
soft break here. Another \emph{sentence} follows.

\begin{itemize}
\tightlist
\item
  item with words that wrap
\end{itemize}

line\\
\strut ~block"###
    );
}

#[test]
fn columns_30() {
    let doc = from_json(r###"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Str","c":"A"},{"t":"Space"},{"t":"Str","c":"paragraph"},{"t":"Space"},{"t":"Str","c":"with"},{"t":"Space"},{"t":"Str","c":"enough"},{"t":"Space"},{"t":"Str","c":"words"},{"t":"Space"},{"t":"Str","c":"to"},{"t":"Space"},{"t":"Str","c":"wrap"},{"t":"Space"},{"t":"Str","c":"at"},{"t":"Space"},{"t":"Str","c":"a"},{"t":"Space"},{"t":"Str","c":"narrow"},{"t":"Space"},{"t":"Str","c":"width,"},{"t":"Space"},{"t":"Str","c":"and"},{"t":"Space"},{"t":"Str","c":"a"},{"t":"SoftBreak"},{"t":"Str","c":"soft"},{"t":"Space"},{"t":"Str","c":"break"},{"t":"Space"},{"t":"Str","c":"here."},{"t":"Space"},{"t":"Str","c":"Another"},{"t":"Space"},{"t":"Emph","c":[{"t":"Str","c":"sentence"}]},{"t":"Space"},{"t":"Str","c":"follows."}]},{"t":"BulletList","c":[[{"t":"Plain","c":[{"t":"Str","c":"item"},{"t":"Space"},{"t":"Str","c":"with"},{"t":"Space"},{"t":"Str","c":"words"},{"t":"Space"},{"t":"Str","c":"that"},{"t":"Space"},{"t":"Str","c":"wrap"}]}]]},{"t":"LineBlock","c":[[{"t":"Str","c":"line"}],[{"t":"Str","c":" block"}]]}]}"###).unwrap();
    let mut options = options();
    options.columns = 30;
    let text = crate::write_latex(&doc, &options, &mut Context::pure()).unwrap();
    assert_eq!(
        text,
        r###"A paragraph with enough words
to wrap at a narrow width, and
a soft break here. Another
\emph{sentence} follows.

\begin{itemize}
\tightlist
\item
  item with words that wrap
\end{itemize}

line\\
\strut ~block"###
    );
}

#[test]
fn number_sections() {
    let doc = from_json(r###"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Header","c":[1,["one",[],[]],[{"t":"Str","c":"One"}]]},{"t":"Header","c":[2,["two",[],[]],[{"t":"Str","c":"Two"}]]},{"t":"Header","c":[6,["six",[],[]],[{"t":"Str","c":"Six"}]]},{"t":"Header","c":[1,["un",["unnumbered"],[]],[{"t":"Str","c":"Un"}]]}]}"###).unwrap();
    let mut options = options();
    options.number_sections = true;
    let text = crate::write_latex(&doc, &options, &mut Context::pure()).unwrap();
    assert_eq!(
        text,
        r###"\section{One}\label{one}

\subsection{Two}\label{two}

Six

\section*{Un}\label{un}
\addcontentsline{toc}{section}{Un}"###
    );
}

#[test]
fn caption_below() {
    let doc = from_json(r###"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Table","c":[["",[],[]],[null,[{"t":"Plain","c":[{"t":"Str","c":"Caption"}]}]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}],[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"a"}]}]],[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"b"}]}]]]]]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"1"}]}]],[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"2"}]}]]]]]]],[["",[],[]],[]]]},{"t":"Figure","c":[["",[],[]],[null,[{"t":"Plain","c":[{"t":"Str","c":"Fig"},{"t":"Space"},{"t":"Str","c":"cap"}]}]],[{"t":"Plain","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"Fig"},{"t":"Space"},{"t":"Str","c":"cap"}],["f.png",""]]}]}]]}]}"###).unwrap();
    let mut options = options();
    options.table_caption_position = CaptionPosition::CaptionBelow;
    options.figure_caption_position = CaptionPosition::CaptionAbove;
    let text = crate::write_latex(&doc, &options, &mut Context::pure()).unwrap();
    assert_eq!(
        text,
        r###"\begin{longtable}[]{@{}ll@{}}
\toprule\noalign{}
a & b \\
\midrule\noalign{}
\endhead
\bottomrule\noalign{}
\tabularnewline
\caption{Caption}
\endlastfoot
1 & 2 \\
\end{longtable}

\begin{figure}
\centering
\caption{Fig cap}
\pandocbounded{\includegraphics[keepaspectratio,alt={Fig cap}]{f.png}}
\end{figure}"###
    );
}

#[test]
fn dpi_150() {
    let doc = from_json(r###"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Image","c":[["",[],[["width","300"]]],[{"t":"Str","c":"a"}],["a.png",""]]},{"t":"Space"},{"t":"Image","c":[["",[],[["height","150px"]]],[{"t":"Str","c":"b"}],["b.png",""]]}]}]}"###).unwrap();
    let mut options = options();
    options.dpi = 150;
    let text = crate::write_latex(&doc, &options, &mut Context::pure()).unwrap();
    assert_eq!(
        text,
        r###"\includegraphics[width=2in,height=\textheight,keepaspectratio,alt={a}]{a.png}
\includegraphics[width=\linewidth,height=1in,keepaspectratio,alt={b}]{b.png}"###
    );
}

#[test]
fn incremental() {
    let doc = from_json(r###"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Header","c":[1,["sec",[],[]],[{"t":"Str","c":"Sec"}]]},{"t":"Header","c":[2,["slide",[],[]],[{"t":"Str","c":"Slide"}]]},{"t":"BulletList","c":[[{"t":"Plain","c":[{"t":"Str","c":"a"}]}],[{"t":"Plain","c":[{"t":"Str","c":"b"}]}]]},{"t":"Header","c":[3,["sub",[],[]],[{"t":"Str","c":"Sub"}]]},{"t":"Para","c":[{"t":"Str","c":"text"}]},{"t":"Header","c":[2,["two",[],[]],[{"t":"Str","c":"Two"}]]},{"t":"OrderedList","c":[[1,{"t":"Decimal"},{"t":"Period"}],[[{"t":"Plain","c":[{"t":"Str","c":"x"}]}],[{"t":"Plain","c":[{"t":"Str","c":"y"}]}]]]}]}"###).unwrap();
    let mut options = options();
    options.incremental = true;
    let text = crate::write_beamer(&doc, &options, &mut Context::pure()).unwrap();
    assert_eq!(
        text,
        r###"\section{Sec}\label{sec}

\begin{frame}{Slide}
\protect\phantomsection\label{slide}
\begin{itemize}[<+->]
\tightlist
\item
  a
\item
  b
\end{itemize}

\begin{block}{Sub}
\protect\phantomsection\label{sub}
text
\end{block}
\end{frame}

\begin{frame}{Two}
\protect\phantomsection\label{two}
\begin{enumerate}[<+->]
\tightlist
\item
  x
\item
  y
\end{enumerate}
\end{frame}"###
    );
}

#[test]
fn slide_level_1() {
    let doc = from_json(r###"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Header","c":[1,["sec",[],[]],[{"t":"Str","c":"Sec"}]]},{"t":"Header","c":[2,["slide",[],[]],[{"t":"Str","c":"Slide"}]]},{"t":"BulletList","c":[[{"t":"Plain","c":[{"t":"Str","c":"a"}]}],[{"t":"Plain","c":[{"t":"Str","c":"b"}]}]]},{"t":"Header","c":[3,["sub",[],[]],[{"t":"Str","c":"Sub"}]]},{"t":"Para","c":[{"t":"Str","c":"text"}]},{"t":"Header","c":[2,["two",[],[]],[{"t":"Str","c":"Two"}]]},{"t":"OrderedList","c":[[1,{"t":"Decimal"},{"t":"Period"}],[[{"t":"Plain","c":[{"t":"Str","c":"x"}]}],[{"t":"Plain","c":[{"t":"Str","c":"y"}]}]]]}]}"###).unwrap();
    let mut options = options();
    options.slide_level = Some(1);
    let text = crate::write_beamer(&doc, &options, &mut Context::pure()).unwrap();
    assert_eq!(
        text,
        r###"\begin{frame}{Sec}
\protect\phantomsection\label{sec}
\begin{block}{Slide}
\protect\phantomsection\label{slide}
\begin{itemize}
\tightlist
\item
  a
\item
  b
\end{itemize}

\begin{block}{Sub}
\protect\phantomsection\label{sub}
text
\end{block}
\end{block}

\begin{block}{Two}
\protect\phantomsection\label{two}
\begin{enumerate}
\tightlist
\item
  x
\item
  y
\end{enumerate}
\end{block}
\end{frame}"###
    );
}

#[test]
fn slide_level_3() {
    let doc = from_json(r###"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Header","c":[1,["sec",[],[]],[{"t":"Str","c":"Sec"}]]},{"t":"Header","c":[2,["slide",[],[]],[{"t":"Str","c":"Slide"}]]},{"t":"BulletList","c":[[{"t":"Plain","c":[{"t":"Str","c":"a"}]}],[{"t":"Plain","c":[{"t":"Str","c":"b"}]}]]},{"t":"Header","c":[3,["sub",[],[]],[{"t":"Str","c":"Sub"}]]},{"t":"Para","c":[{"t":"Str","c":"text"}]},{"t":"Header","c":[2,["two",[],[]],[{"t":"Str","c":"Two"}]]},{"t":"OrderedList","c":[[1,{"t":"Decimal"},{"t":"Period"}],[[{"t":"Plain","c":[{"t":"Str","c":"x"}]}],[{"t":"Plain","c":[{"t":"Str","c":"y"}]}]]]}]}"###).unwrap();
    let mut options = options();
    options.slide_level = Some(3);
    let text = crate::write_beamer(&doc, &options, &mut Context::pure()).unwrap();
    assert_eq!(
        text,
        r###"\section{Sec}\label{sec}

\subsection{Slide}\label{slide}

\begin{frame}{Slide}
\begin{itemize}
\tightlist
\item
  a
\item
  b
\end{itemize}
\end{frame}

\begin{frame}{Sub}
\protect\phantomsection\label{sub}
text
\end{frame}

\subsection{Two}\label{two}

\begin{frame}{Two}
\begin{enumerate}
\tightlist
\item
  x
\item
  y
\end{enumerate}
\end{frame}"###
    );
}

#[test]
fn beamer_chapter() {
    let doc = from_json(r###"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Header","c":[1,["sec",[],[]],[{"t":"Str","c":"Sec"}]]},{"t":"Header","c":[2,["slide",[],[]],[{"t":"Str","c":"Slide"}]]},{"t":"BulletList","c":[[{"t":"Plain","c":[{"t":"Str","c":"a"}]}],[{"t":"Plain","c":[{"t":"Str","c":"b"}]}]]},{"t":"Header","c":[3,["sub",[],[]],[{"t":"Str","c":"Sub"}]]},{"t":"Para","c":[{"t":"Str","c":"text"}]},{"t":"Header","c":[2,["two",[],[]],[{"t":"Str","c":"Two"}]]},{"t":"OrderedList","c":[[1,{"t":"Decimal"},{"t":"Period"}],[[{"t":"Plain","c":[{"t":"Str","c":"x"}]}],[{"t":"Plain","c":[{"t":"Str","c":"y"}]}]]]}]}"###).unwrap();
    let mut options = options();
    options.top_level_division = TopLevelDivision::TopLevelChapter;
    let text = crate::write_beamer(&doc, &options, &mut Context::pure()).unwrap();
    assert_eq!(
        text,
        r###"\part{Sec}\label{sec}

\begin{frame}{Slide}
\protect\phantomsection\label{slide}
\begin{itemize}
\tightlist
\item
  a
\item
  b
\end{itemize}

\begin{block}{Sub}
\protect\phantomsection\label{sub}
text
\end{block}
\end{frame}

\begin{frame}{Two}
\protect\phantomsection\label{two}
\begin{enumerate}
\tightlist
\item
  x
\item
  y
\end{enumerate}
\end{frame}"###
    );
}
