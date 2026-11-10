# Conformance suite

Language-neutral cases for Orchfile implementations. An implementation
conforms when it produces the expected result for every case.

## Layout

```
valid/<case>/      one or more *.orch inputs, optional args, expected.orch
invalid/<case>/    one or more *.orch inputs, optional args, reason
```

- **Inputs**: every `*.orch` file in the case except `expected.orch`, taken in
  name order. The first is the base; the rest are overlays.
- **`args`** (optional): one `name=value` override per line, applied the way
  `--arg name=value` is. In a value, `\n` stands for a line feed.
- **`expected.orch`** (valid cases): the canonical Orchfile the inputs resolve
  to, byte for byte.
- **`reason`** (invalid cases): one line saying which rule the inputs break.
  The inputs MUST be rejected. Error wording is not part of the suite.

## Running it against this implementation

```sh
cargo test --test conformance
```
