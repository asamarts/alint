# v0.24 HCL Parser Evaluation Spike (#608)

Status: completed (2026-11-24).
Decision: upgrade `hcl-rs` from `0.18` to `0.20` (`hcl-edit 0.10.0`), retire the 512 MiB parse
thread, 64 KiB byte ceiling, and 270-line hand-rolled recursion pre-scanner, and **do not** spin
up a standalone `asamarts/ahcl` crate for v0.24 (ADR-0048 decision 4c).

## 1. Question

Issue #608 and ADR-0048 decision 4c asked two ordered questions:

1. Do newer `hcl-rs` / `hcl-edit` releases (`0.20` / `0.10`) add internal recursion and stack
   bounds so HCL parsing runs safely on normal worker stacks without spawning a 512 MiB thread or
   enforcing a 64 KiB byte ceiling?
2. If upstream remains unbounded, is spinning up a standalone `asamarts/ahcl` crate justified?

## 2. Upstream evaluation (`hcl-rs 0.20.0` / `hcl-edit 0.10.0`)

`hcl-rs 0.19.10` / `0.20.0` (`hcl-edit 0.9.9` / `0.10.0`, released 2026-10-29) landed native
recursion and operator-depth bounds in `hcl-edit/src/parser/nesting.rs` (upstream PR #571), plus
boxed `Element::Directive` (#436), Pratt binary operator precedence (#456), missing expression
spans (#509), heredoc delimiter prefix fix (#512), `winnow 1.0` (#530), duplicate object-key
rejection (#562), left-associative chained comparisons (#569), and HCL2 string-escape strictness
rejecting `\b`, `\f`, and `\/` (#573):

- `MAX_NESTING_DEPTH = 64`: enforced via `nested(...)` across blocks (`structure.rs`), nested
  expressions, objects, arrays, parens, function arguments, unary operators, conditionals, `for`
  expressions (`expr.rs`), and nested templates, interpolations, and directives (`template.rs`).
- `MAX_OPERATOR_DEPTH = 128`: enforced via `check_operator_depth` / `operator_scope` so chained
  binary operators cannot build a deep left-leaning expression tree that overflows the stack on
  conversion, formatting, or drop.
- Supply chain & MSRV: adds one tiny MIT/Apache-2.0 crate (`pratt 0.4.0`), passes `cargo deny`
  with zero license exceptions, and compiles cleanly on MSRV Rust `1.88.0`.
- Byte spans: `hcl::edit::Span` already supplies the CST byte spans used by `structured_fix::hcl`
  for `set_value` and `remove_value`, and upgrading automatically brings the same 64/128 depth
  bounds to `structured_fix::hcl`.

## 3. Empirical measurements

### 3.1 Stack headroom (42 at-limit, flat/postfix, large-file, and bomb shapes)

All 42 stress shapes (64-deep blocks, objects, arrays, parens, function calls, unary `-`/`!`,
ternary conditionals, `for` expressions, template interpolations, `%{if}` directives; 128-op
binary, mixed-precedence, and comparison chains; combined 64-deep + 128-op binary; 10k-step
attribute/index/splat traversals; 5k-segment function namespaces; 256 KiB multi-resource `.tf`;
and 60k-131k recursion/heredoc/interpolation bombs) were executed directly through both
`hcl::from_str::<serde_json::Value>` and `hcl::edit::parser::parse_body` without any helper thread
on Linux x86_64:

| Build profile | Highest overflowing stack | Worst shape | Lowest passing stack | Headroom on 1 MiB | Headroom on 2 MiB |
|---|---|---|---|---|---|
| Release (`opt-level = 3`) | 576 KiB | `func_calls_64` | **640 KiB** | >= 384 KiB (1.6x) | >= 1,408 KiB (3.2x) |
| Debug (`opt-level = 0`) | 1,280 KiB | `nested_if_directives_63` / `blocks_64` | **1,408 KiB** | overflows (< 1 MiB) | >= 640 KiB (1.45x) |

Conclusion: in shipping release builds, every accepted and rejected HCL shape fits within 640 KiB
of stack (well below 1 MiB Windows main threads and 2 MiB rayon / 8 MiB LSP worker threads). In
unoptimized debug builds, Linux x86_64 passes at 1,408 KiB (within 2 MiB worker stacks). The
512 MiB `std::thread::Builder` parse thread (which also caused virtual-memory reservation failures
under hardened `RLIMIT_AS` containers) and the 270-line hand-rolled `hcl_depth_within_limit` /
`count_interp` / `skip_heredoc` pre-scanner are no longer needed.

### 3.2 Memory amplification and three residual upstream hazards uncovered during audit

Benchmarking `hcl-rs 0.20` on 1 MiB and 4 MiB dense/adversarial inputs uncovered three residual
upstream behaviors that must be handled in `alint-core`:

1. **`IntoJsonSpec` `VecMap` quadratic CPU blowup in `hcl::from_str`:** `hcl::from_str::<Value>`
   converts `hcl::Body` via `IntoJsonSpec::into_json_spec`, which calls `Expression::from_iter`
   into `hcl::expr::Object` (`vecmap::VecMap`, `O(N)` linear scan per insert -> `O(N^2)` overall),
   even though `hcl-edit` itself already parses bodies in `O(N)` using `FnvHashSet`:

   | Shape | Input size | `hcl::parse` (O(N)) | `hcl::from_str::<Value>` (`VecMap` O(N^2)) |
   |---|---:|---:|---:|
   | Flat attributes (`kN=1`) | 64 KiB | 5.0 ms | 177.5 ms |
   | Flat attributes (`kN=1`) | 256 KiB | 20.1 ms | 1.97 s |
   | Flat attributes (`kN=1`) | 512 KiB | 39.6 ms | 8.44 s |
   | Flat attributes (`kN=1`) | 1 MiB | 76.5 ms | **31.5 s** |
   | Flat attributes (`kN=1`) | 4 MiB | ~300 ms | **651.1 s (10.8 min)** |
   | Labeled blocks (`b "nN" { a = 1 }`) | 256 KiB | 42.2 ms | 476.2 ms |
   | Labeled blocks (`b "nN" { a = 1 }`) | 1 MiB | 162.2 ms | **7.97 s** |
   | Labeled blocks (`b "nN" { a = 1 }`) | 4 MiB | ~650 ms | **233.3 s (3.9 min)** |

   **Mitigation in `alint-core`:** `crates/alint-core/src/structured_format/hcl.rs` parses with
   `hcl::parse(text)` and converts `hcl::Body` directly into `serde_json::Value` using a
   `BTreeMap`-backed `JsonNode` merge (`O(N log N)`), deserializing leaf attribute expressions with
   `Value::deserialize(attr.expr)`. Output matches `hcl::from_str::<Value>` identically (verified
   by parity tests) while eliminating the `O(N^2)` body/block blowup completely (411x faster at
   1 MiB attributes, ~2,100x faster at 4 MiB).

2. **Unbounded block-label recursion in `IntoJsonSpec`:** `hcl-edit` parses block labels
   iteratively into a `Vec`, but `hcl-rs`'s `IntoJsonSpec` recurses once per label when nesting
   `JsonNode::Map`, so a crafted `b "a" "a" ... {}` with thousands of labels overflows the stack.
   **Mitigation in `alint-core`:** `body_to_value` enforces `MAX_HCL_BLOCK_LABELS = 64` (matching
   `MAX_NESTING_DEPTH = 64`) before building nested maps.

3. **Lossless-CST RSS amplification and `hcl-edit::expr::Object` `VecMap` width:**
   - Dense 1 MiB / 4 MiB inputs (`array`, `traversal`) peaked at 103-127 MiB RSS (1 MiB) and
     394-485 MiB RSS (4 MiB) -- roughly 100-125x input size because `hcl-edit` retains a lossless CST
     with per-token `Decor`/`Span` alongside `hcl::Body` and `serde_json::Value`.
   - `hcl-edit`'s `object_items` stores object-literal entries in a `VecMap` and runs
     `object.contains_key(&key)` on every key (`O(N^2)`): a single 256 KiB object literal
     `x = { k0=1, ... }` takes 3.6 s and a 512 KiB object literal exceeds 15 s.
   - **Mitigations in `alint-core`:**
     - Raise `MAX_HCL_BYTES` 64x from 64 KiB to **4 MiB** (matching `MAX_DOTENV_BYTES = 4 MiB`,
       keeping worst-case peak RSS under ~500 MiB while comfortably admitting large real-world
       Terraform/Nomad/Packer files).
     - Enforce `MAX_HCL_OBJECT_ITEMS = 4_096` per `{ ... }` brace scope via a small iterative,
       zero-recursion scanner (`check_object_width`) applied uniformly to both `Format::Hcl.parse`
       and `structured_fix::hcl` (`parse_hcl_edit_body`), keeping worst-case single-object parse
       under ~100 ms while leaving top-level attributes, sibling blocks, and large heredoc bodies
       (such as JSON IAM policies) unrestricted.

## 4. Recommendation on `asamarts/ahcl`

**Do not build `asamarts/ahcl` for v0.24.**

- Upstream `hcl-rs 0.20` / `hcl-edit 0.10` is actively maintained (recent releases in late 2026),
  MIT/Apache-2.0, MSRV 1.88 compatible, and now enforces native recursion and operator-depth
  bounds directly in the parser.
- The three residual hazards (`IntoJsonSpec` `VecMap`, unbounded block labels, and single-object
  `VecMap` + CST RSS amplification) are cleanly closed by ~220 lines in
  `crates/alint-core/src/structured_format/hcl.rs` without spawning a 512 MiB thread or capping
  files at 64 KiB.
- Re-implementing full HCL2 (blocks, expressions, precedence, `for` comprehensions, string
  templates, indented heredocs, directive stripping, comments, and lossless CST spans) in a
  standalone crate would cost ~5,000-8,000 LOC of grammar maintenance for marginal gain over the
  hardened `hcl-rs 0.20` integration.

Revisit `ahcl` only if upstream maintenance stalls, if real-world adoption demands parsing single
HCL files larger than 4 MiB under tight RSS budgets, or if future `version: 2` HCL entry-row
extraction needs zero-copy streaming spans without building a full CST.
