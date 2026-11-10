#!/usr/bin/env bash
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PDFLATEX="${PDFLATEX:-$(command -v pdflatex || echo "$HOME/Library/TinyTeX/bin/universal-darwin/pdflatex")}"

if [[ ! -x "$PDFLATEX" ]]; then
  echo "error: pdflatex not found (set PDFLATEX to its path)" >&2
  exit 1
fi

python3 "$REPO/scripts/sync_grammar_listings.py"

cd "$REPO/docs"
for pass in table_of_contents final; do
  "$PDFLATEX" -interaction=nonstopmode -halt-on-error grammar.tex >/dev/null
done
rm -f grammar.aux grammar.log grammar.toc grammar.out
echo "docs/grammar.pdf rebuilt"
