# orch

Reference implementation of the [Orchfile specification](SPEC.md), a declarative specification for service orchestration.

`orch` merges one or more Orchfiles, expands their variables, checks every rule in the grammar and the spec, and prints the result as a single canonical Orchfile. A runtime such as [orchd](https://github.com/adiled/orchd) takes that result and runs it.

## Install

```sh
cargo install the-orch
```

Installs the `orch` binary from crates.io.

## Usage

```
orch resolve <file> [<file> ...] [--json] [--arg name=value ...]
orch validate <file> [<file> ...] [--arg name=value ...]
```

| Command    | Prints                                   | Exit code                |
|------------|------------------------------------------|--------------------------|
| `resolve`  | the canonical Orchfile, or JSON with `--json` | 0 on success, 1 if invalid |
| `validate` | `valid` on stderr                        | 0 if valid, 1 if invalid |

Exit code 2 means the command line itself was wrong (unknown option, unreadable file).

## Example

`Orchfile`:

```
ARG port=8080

SERVICE db
FROM postgres:15
VOLUME pgdata:/var/lib/postgresql/data
HEALTHCHECK pg_isready

SERVICE web
RUN ./server --port ${port}
ENV MODE=production
REQUIRES db
```

`staging.orch`, an overlay:

```
SERVICE web
ENV MODE=staging
ENV DEBUG=true
```

```
$ orch resolve Orchfile staging.orch --arg port=9090
ORCH_VERSION 1.0.0-rc

SERVICE db
FROM postgres:15
VOLUME pgdata:/var/lib/postgresql/data
HEALTHCHECK pg_isready

SERVICE web
RUN ./server --port 9090
ENV DEBUG=true
ENV MODE=staging
REQUIRES db
```

The output is the [canonical form](SPEC.md#output): one block per service, a fixed directive order, variables expanded, defaults omitted. It is a valid Orchfile, and resolving it again prints the same text, so it can be diffed, hashed and stored.

## Multiple files

Files are merged left to right. The first is the base; each later file is an overlay.

- **Scalars** (`FROM`, `MEMORY`, ...): the overlay's value replaces the base's.
- **`ENV`**: merged by key.
- **`PUBLISH`**: an overlay's mappings for a container port replace all of the base's mappings for that port.
- **`VOLUME`**: merged by destination.
- **`REQUIRES`, `AFTER`, `ENV_FILE`**: appended, without duplicates.
- **`CLEAR <directive>`**: empties that list before the overlay's own values apply.

See [File Composition](SPEC.md#file-composition-overlay-model) in the spec.

## Variables

An `ARG` declares a variable and its default. `${name}` in a directive's value is replaced by it.

```sh
orch resolve Orchfile --arg port=9999          # flag, repeatable
orch resolve Orchfile --arg=port=9999          # same
ORCH_ARG_port=9999 orch resolve Orchfile       # environment
```

- `--arg` beats `ORCH_ARG_*`, which beats the default written on the `ARG` line.
- Only a variable that has an `ARG` line in one of the given Orchfiles can be overridden. `orch` warns when an `--arg` names one that has none.
- Every `${name}` must be a declared ARG. Write `$${` for a literal `${`, as in `RUN sh -c 'echo $${HOME}'`.

## JSON

`--json` prints the same result as JSON:

```sh
orch resolve Orchfile --json | jq -r '.services[] | select(.mode == "container") | .image'
```

The shape is described by [`schema/orch.schema.json`](schema/orch.schema.json) and carries a `schema_version`. The test suite checks the output against the schema.

## Errors

Every error names what is wrong and where:

```
$ orch validate bad.orch
error: parse error: line 3: MEMORY expects a whole number followed by K, M or G (like 512M), got '2G # two gigs'
error: parse error: line 4: undefined variable '${port}' (C5): declare it with ARG, or write $${ for a literal ${
```

With several files, the location includes the file name.

## As a library

```rust
use std::collections::HashMap;

let content = std::fs::read_to_string("Orchfile").expect("readable Orchfile");
let files = vec![("Orchfile".to_string(), content)];
let resolved = the_orch::parse_files(&files, &HashMap::new()).expect("valid Orchfile");
print!("{}", the_orch::canonical::render(&resolved));
```

`the_orch::json::render` gives the JSON view, and `the_orch::types` holds the resolved model.

## Implementing Orchfile elsewhere

- [`grammar.ebnf`](grammar.ebnf) is the syntax, in W3C EBNF. [`docs/grammar.pdf`](docs/grammar.pdf) is the typeset version with railroad diagrams.
- [`SPEC.md`](SPEC.md) defines merging, expansion, the constraints C1–C6, and the canonical form.
- [`conformance/`](conformance/) is a language-neutral suite: inputs with their expected canonical form, and inputs that must be rejected. An implementation conforms when it passes every case.

## Tests

```sh
cargo test
```

## Changing the grammar

Edit `grammar.ebnf`, then run `scripts/build_grammar_pdf.sh`. It copies the grammar into `docs/grammar.tex` and rebuilds `docs/grammar.pdf` (needs `pdflatex`). CI fails when the two are out of sync.

## License

Apache 2.0
