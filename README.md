# Paraflate

Predictive, parallel, self-verifying **DEFLATE** compression for **ZIP** archives. Paraflate analyzes inputs, schedules work across a thread pool, and optional modes can verify archives after write.

## Requirements

- **Rust** 1.74 or later (see workspace `rust-version` in the root `Cargo.toml`)
- A standard Rust toolchain (`rustup`, `cargo`)

## Building

From the repository root:

```bash
cargo build --release
```

The `paraflate` binary is built from `crates/paraflate-cli`.

```bash
cargo run --release -- --help
```

## Command-line interface

| Command | Purpose |
| --- | --- |
| `create` | Compress a directory tree into a ZIP (`-i` input, `-o` output). Supports DEFLATE level, layout, scheduler, predictive and verification modes, global Huffman, adaptive block feedback, and `--debug` block traces. |
| `explain` | Emit a structured JSON report of compression decisions (`-i`). Use `--pretty` for formatted JSON, `--no-run` for prediction-only, and options aligned with `create` (e.g. `--threads`, `--prediction`, `--planning`, `--global-huffman`, `--adaptive`). |
| `analyze` | Print an intel summary for a directory without writing an archive. |
| `debug` | Build an archive with block debug output. |
| `compare` | Compare Paraflate against zlib-style DEFLATE on a flattened corpus. |
| `validate` | Verify an existing ZIP (`-p` / `--zip`). |
| `bench` | Timing-oriented archive creation for benchmarking. |
| `harness` | Full evaluation harness: writes under `test_file/` (corpus, Paraflate archives, `flate2` / naive / reference ZIP baselines, extracted entries, JSON and TSV reports). |

Run `paraflate <command> --help` for flags and defaults.

## Evaluation harness (`test_file/`)

From the repo root, the deterministic corpus and all artifacts live under **`test_file/`** (override with `--output` on the standalone binary):

```bash
cargo run --release -p paraflate-harness -- --output test_file
```

or:

```bash
cargo run --release -- harness --output test_file
```

Use `--skip-large` to omit the multi-megabyte workload while iterating.

Layout:

| Path | Contents |
| --- | --- |
| `test_file/manifest.json` | Every generated file path, size, SHA-256 |
| `test_file/corpus/wl_*` | Workload directories (text, binary, logs, nested files, mixed, etc.) |
| `test_file/reference/<workload>/` | `flate2_zlib.bin`, `naive_zlib_fast.bin`, `reference_deflated.zip` |
| `test_file/paraflate_archives/` | One ZIP per workload and harness mode |
| `test_file/extracted/` | Per-run extracted copies for inspection |
| `test_file/reports/` | `harness_run.json`, `summary.txt`, `comparisons.json`, `comparison_table.tsv`, `harness.log` |

Reports include verification outcomes, compression ratios, throughput, fallback counts (when debug is on), and workload-level winners versus `flate2` and the reference ZIP.

## Workspace layout

| Crate | Role |
| --- | --- |
| `paraflate-core` | Archive profile, entry descriptors, planning types, explain report types |
| `paraflate-io` | Directory scanning and read planning |
| `paraflate-dictionary` | Sampling and global analysis |
| `paraflate-index` | Cross-entry pattern index |
| `paraflate-lz77` | LZ77 tokenization |
| `paraflate-deflate` | DEFLATE engine and streaming encode |
| `paraflate-zip` | ZIP writer |
| `paraflate-verify` | Inflate and structural checks |
| `paraflate-scheduler` | Worker pool and task graph |
| `paraflate-pipeline` | End-to-end archive session, intel, explain report builder |
| `paraflate-cli` | `paraflate` binary |
| `paraflate-harness` | Corpus generation, references, validation, structured reports |
| `paraflate-bench` / `paraflate-tests` | Benchmarks and integration tests |

## Tests

```bash
cargo test
```

## License

Apache-2.0 license
