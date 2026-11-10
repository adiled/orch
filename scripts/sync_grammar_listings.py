#!/usr/bin/env python3
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
GRAMMAR = REPO / "grammar.ebnf"
TEX = REPO / "docs" / "grammar.tex"

SECTION_TITLE_AND_FIRST_GRAMMAR_LINE = [
    (r"\subsection{Lines}", "/* Lines."),
    (r"\subsection{Document}", "/* Document."),
    (r"\subsection{Service Blocks}", "/* Service blocks."),
    (r"\subsection{Directives}", "directive "),
    (r"\subsubsection{Overlay Composition}", "/* Overlay composition"),
    (r"\subsubsection{Execution Mode}", "/* Execution mode"),
    (r"\subsubsection{Container-only Directives}", "/* Container-only"),
    (r"\subsubsection{Host-only Directives}", "/* Host-only"),
    (r"\subsubsection{Common Directives}", "/* Common - environment"),
    (r"\subsection{Terminals}", "/* Terminals"),
]

NOTE_AFTER_LISTINGS = [
    r"\medskip",
    r"\noindent",
    r"\textbf{Note:} The grammar describes each value \emph{after} variable",
    r"expansion. \texttt{var\_ref} (\texttt{\$\{name\}}) may appear anywhere in the",
    r"value of a directive inside a SERVICE block, and \texttt{var\_escape}",
    r"(\texttt{\$\$\{}) stands for a literal \texttt{\$\{}. With",
    r"\texttt{ARG port=8080}, \texttt{PUBLISH \$\{port\}:80} is checked as",
    r"\texttt{PUBLISH 8080:80}. Every name must be a declared ARG (C5).",
    "",
]


def line_starting_with(lines, prefix):
    matches = [index for index, line in enumerate(lines) if line.startswith(prefix)]
    if len(matches) != 1:
        sys.exit(f"grammar.ebnf: expected one line starting with {prefix!r}, found {len(matches)}")
    return matches[0]


def without_trailing_blank_lines(lines):
    while lines and not lines[-1].strip():
        lines = lines[:-1]
    return lines


def grammar_sections(grammar_lines):
    starts = [line_starting_with(grammar_lines, first) for _, first in SECTION_TITLE_AND_FIRST_GRAMMAR_LINE]
    ends = starts[1:] + [len(grammar_lines)]
    sections = [
        (title, without_trailing_blank_lines(grammar_lines[start:end]))
        for (title, _), start, end in zip(SECTION_TITLE_AND_FIRST_GRAMMAR_LINE, starts, ends)
    ]
    covered = [line for _, lines in sections for line in lines if line.strip()]
    if covered != [line for line in grammar_lines if line.strip()]:
        sys.exit("the sections do not cover grammar.ebnf exactly")
    return sections


def listings(grammar_lines):
    rendered = []
    for title, lines in grammar_sections(grammar_lines):
        rendered += [title, "", r"\begin{lstlisting}", *lines, r"\end{lstlisting}", ""]
    return rendered + NOTE_AFTER_LISTINGS


def with_listings_replaced(tex_lines, new_listings):
    grammar_section = tex_lines.index(r"\section{EBNF Grammar}")
    first_listing = next(
        index for index in range(grammar_section, len(tex_lines)) if tex_lines[index].startswith(r"\subsection{")
    )
    diagrams_section = tex_lines.index(r"\section{Railroad Diagrams}")
    banner_before_diagrams = max(
        index for index in range(first_listing, diagrams_section) if tex_lines[index].startswith("% =====")
    )
    return tex_lines[:first_listing] + new_listings + tex_lines[banner_before_diagrams:]


def main():
    check_only = sys.argv[1:] == ["--check"]
    if sys.argv[1:] and not check_only:
        sys.exit("usage: sync_grammar_listings.py [--check]")

    current = TEX.read_text()
    synced = "\n".join(with_listings_replaced(current.splitlines(), listings(GRAMMAR.read_text().splitlines()))) + "\n"

    if synced == current:
        return
    if check_only:
        sys.exit("docs/grammar.tex is out of sync with grammar.ebnf: run scripts/sync_grammar_listings.py")
    TEX.write_text(synced)
    print("docs/grammar.tex updated; rebuild docs/grammar.pdf")


main()
