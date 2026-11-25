//! Stack-headroom, parity, width-guard, and adversarial regression tests for
//! bounded HCL2 parsing on `hcl-rs` 0.20 / `hcl-edit` 0.10 (#608).

use std::fmt::Write as _;

use super::Format;
use super::hcl::{
    MAX_HCL_BLOCK_DEPTH, MAX_HCL_BYTES, MAX_HCL_OBJECT_ITEMS, MAX_HCL_OBJECT_WORK, parse_edit_body,
};

/// Worst-case inputs right AT `hcl-edit` 0.10's built-in limits (`MAX_NESTING_DEPTH = 64`,
/// `MAX_OPERATOR_DEPTH = 128`) and far ABOVE them, plus adversarial flat/postfix shapes.
#[allow(clippy::too_many_lines)]
fn all_hcl_stress_cases() -> Vec<(&'static str, String, bool)> {
    let mut cases: Vec<(&'static str, String, bool)> = Vec::new();

    // 1. At-limit accepted shapes (must parse cleanly without stack overflow).
    cases.push((
        "blocks_64",
        format!("{}x = 1\n{}", "a {\n".repeat(64), "}\n".repeat(64)),
        true,
    ));
    cases.push((
        "objects_64",
        format!("x = {}1{}", "{ a = ".repeat(64), " }".repeat(64)),
        true,
    ));
    cases.push((
        "arrays_64",
        format!("x = {}1{}", "[".repeat(64), "]".repeat(64)),
        true,
    ));
    cases.push((
        "parens_64",
        format!("x = {}1{}", "(".repeat(64), ")".repeat(64)),
        true,
    ));
    cases.push((
        "func_calls_64",
        format!("x = {}1{}", "f(".repeat(64), ")".repeat(64)),
        true,
    ));
    cases.push(("unary_minus_64", format!("x = {}1", "-".repeat(64)), true));
    cases.push(("unary_not_64", format!("x = {}true", "!".repeat(64)), true));
    cases.push((
        "ternary_64",
        format!("x = {}1", "1 ? 1 : ".repeat(64)),
        true,
    ));
    cases.push(("binary_128", format!("x = 1{}", " + 1".repeat(128)), true));
    cases.push((
        "mixed_prec_binary_128",
        format!("x = 1{}", " + 2 * 3".repeat(64)),
        true,
    ));
    cases.push((
        "comparison_chain_128",
        format!("x = 1{}", " == 1".repeat(128)),
        true,
    ));
    cases.push((
        "for_list_32",
        format!("x = {}1{}", "[for i in ".repeat(32), " : i]".repeat(32)),
        true,
    ));
    cases.push((
        "interp_parens_63",
        format!("x = \"${{{}1{}}}\"", "(".repeat(63), ")".repeat(63)),
        true,
    ));
    cases.push((
        "nested_templates_32",
        format!("x = {}{}{}", "\"${".repeat(32), "1", "}\"".repeat(32)),
        true,
    ));
    cases.push((
        "nested_if_directives_63",
        format!(
            "x = \"{}ok{}\"",
            "%{if true}".repeat(63),
            "%{endif}".repeat(63)
        ),
        true,
    ));
    cases.push((
        "combined_blocks_64_plus_binary_128",
        format!(
            "{}x = 1{}\n{}",
            "a {\n".repeat(64),
            " + 1".repeat(128),
            "}\n".repeat(64)
        ),
        true,
    ));
    cases.push((
        "combined_parens_64_plus_binary_128",
        format!(
            "x = {}1{}{}",
            "(".repeat(64),
            " + 1".repeat(128),
            ")".repeat(64)
        ),
        true,
    ));

    // 2. Adversarial flat/postfix shapes (no nesting depth, must parse iteratively).
    cases.push((
        "traversal_attr_10k",
        format!("x = a{}", ".b".repeat(10_000)),
        true,
    ));
    cases.push((
        "traversal_index_10k",
        format!("x = a{}", "[0]".repeat(10_000)),
        true,
    ));
    cases.push((
        "traversal_splat_10k",
        format!("x = a{}", ".*".repeat(10_000)),
        true,
    ));
    cases.push((
        "func_namespace_5k",
        format!("x = a{}::f()", "::b".repeat(5_000)),
        true,
    ));
    // Large valid Terraform file well past the former 64 KiB cap (> 200 KiB).
    let mut large_tf = String::with_capacity(280 * 1024);
    for i in 0..3_000 {
        let _ = writeln!(
            large_tf,
            "resource \"aws_s3_bucket\" \"b_{i}\" {{\n  bucket = \"bucket-{i}\"\n  acl = \"private\"\n}}"
        );
    }
    assert!(large_tf.len() > 200 * 1024);
    cases.push(("large_valid_tf_256k", large_tf, true));

    // 3. Over-limit bombs (must return Err, NEVER overflow stack).
    let d = 60_000;
    cases.push((
        "bomb_blocks_65",
        format!("{}x = 1\n{}", "a {\n".repeat(65), "}\n".repeat(65)),
        false,
    ));
    // Cumulative block + label depth bomb: 64 nested blocks each carrying 64
    // labels would build a 4,160-deep JSON object tree and overflow Value::drop.
    let label_64 = "\"l\" ".repeat(64);
    cases.push((
        "bomb_blocks_64x64_labels",
        format!(
            "{}x = 1\n{}",
            format!("b {label_64}{{\n").repeat(64),
            "}\n".repeat(64)
        ),
        false,
    ));
    cases.push((
        "bomb_blocks_60k",
        format!("{}x = 1\n{}", "a {".repeat(d), "}".repeat(d)),
        false,
    ));
    cases.push((
        "bomb_objects_60k",
        format!("x = {}1{}", "{a=".repeat(d), "}".repeat(d)),
        false,
    ));
    cases.push((
        "bomb_arrays_60k",
        format!("x = {}1{}", "[".repeat(d), "]".repeat(d)),
        false,
    ));
    cases.push((
        "bomb_parens_60k",
        format!("x = {}1{}", "(".repeat(d), ")".repeat(d)),
        false,
    ));
    cases.push((
        "bomb_func_calls_60k",
        format!("x = {}1{}", "f(".repeat(d), ")".repeat(d)),
        false,
    ));
    cases.push((
        "bomb_unary_minus_60k",
        format!("x = {}1", "-".repeat(d)),
        false,
    ));
    cases.push((
        "bomb_unary_spaced_60k",
        format!("x = {}1", "- ".repeat(d)),
        false,
    ));
    cases.push((
        "bomb_unary_not_60k",
        format!("x = {}true", "!".repeat(d)),
        false,
    ));
    cases.push((
        "bomb_binary_129",
        format!("x = 1{}", "+1".repeat(129)),
        false,
    ));
    cases.push((
        "bomb_binary_80k",
        format!("x = 1{}", "+1".repeat(80_000)),
        false,
    ));
    cases.push((
        "bomb_ternary_65k",
        format!("x = {}1", "1?1:".repeat(65_000)),
        false,
    ));
    cases.push((
        "bomb_interp_parens_60k",
        format!("x = \"${{{}1{}}}\"", "(".repeat(d), ")".repeat(d)),
        false,
    ));
    cases.push((
        "bomb_interp_binary_131k",
        format!("x = \"${{1{}}}\"", "+1".repeat(131_000)),
        false,
    ));
    cases.push((
        "bomb_heredoc_interp_arrays_60k",
        format!("x = <<EOT\n${{{}1{}}}\nEOT\n", "[".repeat(d), "]".repeat(d)),
        false,
    ));
    cases.push((
        "bomb_heredoc_interp_binary_131k",
        format!("x = <<EOT\n${{1{}}}\nEOT\n", "+1".repeat(131_000)),
        false,
    ));
    cases.push((
        "bomb_heredoc_after_comment_term",
        format!(
            "x = <<EOT\nbody\nEOT # comment\nz = {}1{}\n",
            "[".repeat(500),
            "]".repeat(500)
        ),
        false,
    ));
    cases.push((
        "bomb_comment_in_interp",
        format!(
            "x = \"${{1 # }}\n{}1{}}}\"\n",
            "(".repeat(500),
            ")".repeat(500)
        ),
        false,
    ));
    cases.push((
        "bomb_nested_templates_10k",
        format!(
            "x = {}{}{}",
            "\"${".repeat(10_000),
            "1",
            "}\"".repeat(10_000)
        ),
        false,
    ));
    cases.push((
        "bomb_if_directives_10k",
        format!(
            "x = \"{}ok{}\"",
            "%{if true}".repeat(10_000),
            "%{endif}".repeat(10_000)
        ),
        false,
    ));

    cases
}

#[test]
fn hcl_parses_all_at_limit_and_bomb_shapes_on_normal_worker_stack() {
    // Release builds pass all 43 shapes at 640 KiB (tested at 1 MiB + 2 MiB);
    // unoptimized debug builds need ~1.4 MiB on Linux (tested at 2 MiB; MSVC
    // debug frames can be slightly larger, so test at 4 MiB on debug Windows).
    let stack_sizes: &[usize] = if cfg!(not(debug_assertions)) {
        &[1024 * 1024, 2 * 1024 * 1024]
    } else if cfg!(windows) {
        &[4 * 1024 * 1024]
    } else {
        &[2 * 1024 * 1024]
    };

    for &stack_bytes in stack_sizes {
        for (name, input, expect_ok) in all_hcl_stress_cases() {
            let input_edit = input.clone();
            let (val_ok, edit_ok) = std::thread::Builder::new()
                .name(format!("hcl-stress-{name}"))
                .stack_size(stack_bytes)
                .spawn(move || {
                    let val_res = Format::Hcl.parse(&input);
                    let edit_res = parse_edit_body(&input_edit);
                    (val_res.is_ok(), edit_res.is_ok())
                })
                .expect("spawn stack-bounded thread")
                .join()
                .unwrap_or_else(|_| panic!("HCL stress case {name} panicked"));
            assert_eq!(
                val_ok, expect_ok,
                "Format::Hcl.parse mismatch for {name} on {stack_bytes}-byte stack"
            );
            assert_eq!(
                edit_ok, expect_ok,
                "parse_edit_body mismatch for {name} on {stack_bytes}-byte stack"
            );
        }
    }
}

#[test]
fn hcl_direct_body_conversion_matches_hcl_from_str_json_spec() {
    let docs = [
        r#"
            scalar_int = 42
            scalar_neg = -7
            scalar_float = 3.5
            scalar_bool = true
            scalar_null = null
            scalar_str = "hello\nworld"
            arr = [1, "two", [3, 4], { nested = true }]
            obj = {
              a = 1,
              "b-key" = "v",
              (var.dyn_key) = "dyn",
              ("paren-key") = "paren",
              deep = { k = [1, 2] },
            }
            paren_val = (123)
            expr_var = var.region
            expr_inf = inf
            expr_nan = nan
            expr_interp = "prefix-${var.env}-suffix"
            expr_escaped = "$$${literal}"
            expr_call = upper("abc")
            expr_bin = 1 + 2 * 3
            expr_cond = true ? "yes" : "no"
            expr_for_list = [for v in var.items : upper(v) if v != ""]
            expr_for_obj = { for k, v in var.map : k => v }
            expr_splat = var.list[*].id
            doc = <<-EOT
              line one
              line two
            EOT
            empty_doc = <<EOF
            EOF
        "#,
        r#"
            terraform {
              required_version = ">= 1.5"
              required_providers {
                aws = { source = "hashicorp/aws", version = "~> 5.0" }
              }
            }
            resource "aws_s3_bucket" "alpha" {
              bucket = "a"
              tags = { env = "prod" }
            }
            resource "aws_s3_bucket" "beta" {
              bucket = "b"
            }
            resource "aws_s3_bucket" "alpha" {
              bucket = "a-repeated"
            }
            ingress {
              from_port = 80
            }
            ingress {
              from_port = 443
            }
            singleton {
              enabled = true
            }
            bare_label my_id {
              ok = true
            }
        "#,
        // Block-overrides-attribute and attribute-overrides-block ordering.
        r"
            x = 1
            x {
              a = 2
            }
            y {
              a = 3
            }
            y = 4
        ",
    ];
    for (idx, doc) in docs.iter().enumerate() {
        let ours = Format::Hcl
            .parse(doc)
            .unwrap_or_else(|e| panic!("doc {idx} failed Format::Hcl.parse: {e}"));
        let upstream: serde_json::Value =
            hcl::from_str(doc).unwrap_or_else(|e| panic!("doc {idx} failed hcl::from_str: {e}"));
        assert_eq!(ours, upstream, "HCL JSON Spec mismatch on doc {idx}");
    }

    // Non-string literal object keys must fail just like `hcl::from_str::<Value>`.
    for invalid_key_doc in [
        "x = { (1) = \"a\" }\n",
        "x = { (true) = \"a\" }\n",
        "x = { (null) = \"a\" }\n",
        "x = { ([1]) = \"a\" }\n",
        "x = { ({ a = 1 }) = \"a\" }\n",
    ] {
        assert!(
            Format::Hcl.parse(invalid_key_doc).is_err(),
            "non-string object key should fail: {invalid_key_doc:?}"
        );
        assert!(
            hcl::from_str::<serde_json::Value>(invalid_key_doc).is_err(),
            "upstream should also reject non-string object key: {invalid_key_doc:?}"
        );
    }
}

fn wide_object_body(count: usize) -> String {
    let mut out = String::with_capacity(count * 8);
    for i in 0..count {
        let _ = writeln!(out, "k{i} = 1");
    }
    out
}

#[test]
fn hcl_over_wide_object_is_rejected_while_large_bodies_locals_and_heredocs_pass() {
    // Over-wide single object literal (> MAX_HCL_OBJECT_ITEMS) is rejected fast
    // instead of triggering `hcl-edit`'s O(N^2) VecMap insertion loop.
    let wide_obj = format!("x = {{\n{}}}\n", wide_object_body(MAX_HCL_OBJECT_ITEMS + 1));
    let err = Format::Hcl.parse(&wide_obj).unwrap_err();
    assert!(err.contains("width"), "over-wide object rejected: {err}");
    assert!(
        parse_edit_body(&wide_obj).is_err(),
        "structured_fix shares the object-width guard"
    );

    // Colon-separated object literal inside `${...}` interpolation is also caught.
    let mut wide_interp = String::from("x = \"${ {");
    for i in 0..=MAX_HCL_OBJECT_ITEMS {
        let _ = write!(wide_interp, "k{i}:1,");
    }
    wide_interp.push_str("} }\"\n");
    assert!(Format::Hcl.parse(&wide_interp).is_err());

    // Over-wide object inside a heredoc `${...}` interpolation is caught.
    let mut wide_heredoc_interp = String::from("x = <<EOT\n${ {");
    for i in 0..=MAX_HCL_OBJECT_ITEMS {
        let _ = write!(wide_heredoc_interp, "k{i}=1,");
    }
    wide_heredoc_interp.push_str("} }\nEOT\n");
    assert!(Format::Hcl.parse(&wide_heredoc_interp).is_err());

    // Positive control 1: > MAX_HCL_OBJECT_ITEMS TOP-LEVEL attributes parse fast
    // in O(N log N) because top-level bodies use FnvHashSet + BTreeMap, not VecMap.
    let mut many_top_attrs = String::new();
    for i in 0..(MAX_HCL_OBJECT_ITEMS + 500) {
        let _ = writeln!(many_top_attrs, "k{i} = {i}");
    }
    let parsed = Format::Hcl
        .parse(&many_top_attrs)
        .expect("large flat top-level .tfvars parses cleanly");
    assert_eq!(parsed["k0"], serde_json::json!(0));
    assert_eq!(
        parsed[format!("k{}", MAX_HCL_OBJECT_ITEMS + 499)],
        serde_json::json!(MAX_HCL_OBJECT_ITEMS + 499)
    );

    // Positive control 2: > MAX_HCL_OBJECT_ITEMS attributes inside a `locals {}`
    // block (block body, parsed in O(N) via FnvHashSet, NOT an object literal)
    // must NOT false-reject.
    let mut large_locals = String::from("locals {\n");
    for i in 0..(MAX_HCL_OBJECT_ITEMS + 500) {
        let _ = writeln!(large_locals, "  v{i} = {i}");
    }
    large_locals.push_str("}\n");
    let parsed_locals = Format::Hcl
        .parse(&large_locals)
        .expect("large locals {} block must not false-reject on object width");
    assert_eq!(parsed_locals["locals"]["v0"], serde_json::json!(0));

    // Positive control 3: list `for` expressions and parenthesized ternaries
    // inside an object literal must NOT charge `:` against the outer object.
    let mut expr_rich_obj = String::from("x = {\n");
    for i in 0..(MAX_HCL_OBJECT_ITEMS / 2) {
        let _ = writeln!(
            expr_rich_obj,
            "  k{i} = [for item in [1, 2] : (item == 1 ? \"a\" : \"b\")]"
        );
    }
    expr_rich_obj.push_str("}\n");
    assert!(
        Format::Hcl.parse(&expr_rich_obj).is_ok(),
        "nested brackets/parens with `:` must not false-reject outer object"
    );

    // Positive control 4: a large JSON policy heredoc inside a block has many
    // `:` tokens inside heredoc body content, which must NOT trigger width rejection.
    let mut json_heredoc =
        String::from("resource \"aws_iam_policy\" \"p\" {\n  policy = <<EOF\n{\n");
    for i in 0..(MAX_HCL_OBJECT_ITEMS + 500) {
        let _ = writeln!(json_heredoc, "  \"k{i}\": \"v{i}\",");
    }
    json_heredoc.push_str("  \"end\": true\n}\nEOF\n}\n");
    assert!(
        Format::Hcl.parse(&json_heredoc).is_ok(),
        "large JSON heredoc inside a block must not false-reject on colons"
    );

    // Positive control 5: banners / dashes / parens inside strings, heredocs,
    // comments, and escaped `$${` parse cleanly.
    let banners = format!(
        "banner = \"{}\"\nparens = \"{}\"\nd = <<EOT\n{}\nEOT\n# {}\nesc = \"$${{{}1}}\"\n",
        "-".repeat(500),
        "[".repeat(500),
        vec!["-".repeat(80); 20].join("\n"),
        "-".repeat(500),
        "-".repeat(500),
    );
    assert!(Format::Hcl.parse(&banners).is_ok());
}

#[test]
fn hcl_width_guard_resists_all_lexical_desync_bypasses() {
    let wide = format!("z = {{\n{}}}\n", wide_object_body(MAX_HCL_OBJECT_ITEMS + 1));

    // Bypass 2a: odd `$` / `%` runs before `{` (`"$$${"` and `"%%%{"`) are
    // literal prefix char + 3-byte escaped marker (NOT an interpolation), so
    // the closing `"` really closes the string and `z = { ... }` is real code.
    for prefix in [
        "a = \"$$${\"\n",
        "a = \"%%%{\"\n",
        "a = \"$$$$${\"\n",
        "a = \"%%%%{\"\n",
    ] {
        let doc = format!("{prefix}{wide}");
        assert!(
            Format::Hcl.parse(&doc).is_err(),
            "odd-run template escape bypass must be rejected: {prefix:?}"
        );
    }

    // Bypass 2b: Unicode heredoc delimiter (`<<é`) containing a `"` inside the
    // heredoc body must not invert string/code state before `z = { ... }`.
    let unicode_heredoc = format!("a = <<é\n\"\né\n{wide}");
    assert!(
        Format::Hcl.parse(&unicode_heredoc).is_err(),
        "Unicode heredoc delimiter bypass must be rejected"
    );

    // Bypass 2c-1: line 1 starting with `EOT"` followed by a real line-2 `EOT`
    // terminator must NOT close on line 1 and swallow `z = { ... }` into `"`.
    let line1_quote = format!("a = <<EOT\nEOT\"\nEOT\n{wide}");
    assert!(
        Format::Hcl.parse(&line1_quote).is_err(),
        "premature line-1 heredoc close bypass must be rejected"
    );

    // Bypass 2c-2: Unicode XID_Continue combining mark immediately after tag
    // (`EOT\u{0301}"`) is NOT a heredoc terminator in `hcl-edit`.
    let combining_mark = format!("a = <<EOT\nbody\nEOT\u{0301}\"\nEOT\n{wide}");
    assert!(
        Format::Hcl.parse(&combining_mark).is_err(),
        "Unicode combining-mark heredoc terminator bypass must be rejected"
    );

    // Bypass 2c-3: genuine empty heredoc on line 1 followed immediately by a
    // wide object inside an array (`[<<EOT\nEOT, { ... }]`).
    let empty_heredoc_array = format!(
        "x = [<<EOT\nEOT, {{\n{}}}]\n",
        wide_object_body(MAX_HCL_OBJECT_ITEMS + 1)
    );
    assert!(
        Format::Hcl.parse(&empty_heredoc_array).is_err(),
        "empty heredoc followed by wide object must be rejected"
    );

    // Bypass 2d-1: `%{if true}"%{endif}` inside a quoted string -- `hcl-edit`
    // ignores `"` inside directive bodies.
    let directive_quote = format!("a = \"%{{if true}}\"%{{endif}}\"\n{wide}");
    assert!(
        Format::Hcl.parse(&directive_quote).is_err(),
        "quote inside %{{if}} directive bypass must be rejected"
    );

    // Bypass 2d-2: `%{for x in [1]}\nEOT\n%{endfor}` inside a heredoc --
    // `hcl-edit` ignores the enclosing heredoc tag inside directive bodies.
    let directive_heredoc =
        format!("a = <<EOT\n%{{for x in [1]}}\nEOT\n\"%{{endfor}}\nEOT\n{wide}");
    assert!(
        Format::Hcl.parse(&directive_heredoc).is_err(),
        "heredoc tag inside %{{for}} directive bypass must be rejected"
    );

    // Multiline continuation after array/operator (`x = [1] +\n { ... }`) must
    // still classify `{ ... }` as an ObjectBrace, not a BlockBrace.
    let multiline_expr_obj = format!(
        "x = [1] +\n{{\n{}}}\n",
        wide_object_body(MAX_HCL_OBJECT_ITEMS + 1)
    );
    assert!(
        Format::Hcl.parse(&multiline_expr_obj).is_err(),
        "multiline expression object after operator must be rejected"
    );
}

#[test]
fn hcl_quadratic_work_budget_and_scan_depth_cap_reject_adversarial_shapes() {
    // 1. Many sibling at-limit objects (each <= MAX_HCL_OBJECT_ITEMS) exceed
    // MAX_HCL_OBJECT_WORK and must be rejected before hcl-edit runs O(M * K^2).
    let mut sibling_wide = String::new();
    let one_obj = wide_object_body(MAX_HCL_OBJECT_ITEMS);
    for m in 0..40 {
        let _ = write!(sibling_wide, "o{m} = {{\n{one_obj}}}\n");
    }
    let err = Format::Hcl.parse(&sibling_wide).unwrap_err();
    assert!(
        err.contains("complexity"),
        "many sibling wide objects must hit MAX_HCL_OBJECT_WORK ({MAX_HCL_OBJECT_WORK}): {err}"
    );

    // 2. A single object with <= MAX_HCL_OBJECT_ITEMS entries whose expression
    // keys are ~1 KiB each exceeds MAX_HCL_OBJECT_WORK and is rejected fast.
    let long_prefix = "a+".repeat(450);
    let mut long_key_obj = String::from("x = {\n");
    for i in 0..500 {
        let _ = writeln!(long_key_obj, "  ({long_prefix}k{i}) = 1");
    }
    long_key_obj.push_str("}\n");
    let err = Format::Hcl.parse(&long_key_obj).unwrap_err();
    assert!(
        err.contains("complexity"),
        "long expression keys in one object must hit MAX_HCL_OBJECT_WORK: {err}"
    );

    // 3. Adversarial 4 MiB `{`-stream hits MAX_HCL_SCAN_DEPTH immediately with
    // tiny bounded scanner memory (never allocates a 4M-element Vec).
    let brace_stream = "{".repeat(MAX_HCL_BYTES);
    let err = Format::Hcl.parse(&brace_stream).unwrap_err();
    assert!(
        err.contains("nesting depth"),
        "4 MiB brace stream must hit scan-depth cap immediately: {err}"
    );
}

#[test]
fn hcl_cumulative_block_and_label_depth_is_rejected_not_a_stack_overflow() {
    // Single block at cumulative depth 64 (1 block + 63 labels = 64) succeeds.
    let at_limit = format!(
        "b {} {{ x = 1 }}\n",
        "\"l\" ".repeat(MAX_HCL_BLOCK_DEPTH - 1)
    );
    assert!(Format::Hcl.parse(&at_limit).is_ok());
    assert!(parse_edit_body(&at_limit).is_ok());

    // Single block at cumulative depth 65 (1 block + 64 labels = 65) fails.
    let single_over = format!("b {} {{ x = 1 }}\n", "\"l\" ".repeat(MAX_HCL_BLOCK_DEPTH));
    let err = Format::Hcl.parse(&single_over).unwrap_err();
    assert!(
        err.contains("block and label nesting"),
        "over-labeled block rejected: {err}"
    );
    assert!(
        parse_edit_body(&single_over).is_err(),
        "parse_edit_body enforces the same cumulative block+label depth cap"
    );

    // Cumulative nested blocks + labels: 8 nested blocks x (1 + 8 labels) = 72 > 64.
    let nested_over = format!(
        "{}x = 1\n{}",
        "b \"l1\" \"l2\" \"l3\" \"l4\" \"l5\" \"l6\" \"l7\" \"l8\" {\n".repeat(8),
        "}\n".repeat(8)
    );
    let err = Format::Hcl.parse(&nested_over).unwrap_err();
    assert!(
        err.contains("block and label nesting"),
        "cumulative nested block+label depth rejected: {err}"
    );
    assert!(parse_edit_body(&nested_over).is_err());

    // Thousands of labels on one block rejected cleanly without overflowing.
    let huge = format!("b {} {{ x = 1 }}\n", "\"l\" ".repeat(5_000));
    assert!(Format::Hcl.parse(&huge).is_err());
    assert!(parse_edit_body(&huge).is_err());
}

#[test]
fn hcl_size_cap_rejects_oversize_input_for_check_and_fix() {
    let over = format!("a = \"{}\"\n", "x".repeat(MAX_HCL_BYTES));
    assert!(over.len() > MAX_HCL_BYTES);
    let err = Format::Hcl.parse(&over).unwrap_err();
    assert!(
        err.contains("maximum supported size"),
        "oversize HCL rejected: {err}"
    );
    assert!(parse_edit_body(&over).is_err());
}

#[test]
fn hcl_heredoc_terminators_and_interp_comments_behave_consistently() {
    // Lenient heredoc terminators (trailing whitespace / comments / indentation)
    // still parse for valid docs and still reject deep bombs placed after them.
    let deep = format!("{}1{}", "[".repeat(100), "]".repeat(100));
    for term in [
        "EOT",
        "EOT ",
        "EOT\t",
        "EOT   ",
        "  EOT",
        "  EOT  ",
        "EOT # c",
        "EOT#c",
        "EOT//c",
        "  EOT\t// c",
    ] {
        let bomb = format!("x = <<EOT\nbody\n{term}\nz = {deep}\n");
        assert!(
            Format::Hcl.parse(&bomb).is_err(),
            "bomb after heredoc terminator {term:?} must be rejected"
        );
    }
    for term in [
        "EOT",
        "EOT ",
        "  EOT",
        "  EOT  ",
        "EOT # done",
        "EOT // done",
    ] {
        let ok = format!("x = <<-EOT\n  hello\n{term}\ny = 1\n");
        assert!(
            Format::Hcl.parse(&ok).is_ok(),
            "valid heredoc with terminator {term:?} must parse"
        );
    }
    // Comments inside `${...}` interpolations: valid ones parse, bombs after them fail.
    for src in [
        "x = <<EOT\n${1 # c\n+ 1}\nEOT\n",
        "x = <<EOT\n${1 // c\n+ 1}\nEOT\n",
        "x = <<EOT\n${1 /* c */ + 1}\nEOT\n",
    ] {
        assert!(
            Format::Hcl.parse(src).is_ok(),
            "valid interp comment: {src:?}"
        );
    }
    for lead in ["#", "//", "/*"] {
        let close = if lead == "/*" { "*/" } else { "" };
        let bomb = format!("x = <<EOT\n${{1 {lead} }} {close}\n+ {deep} }}\nEOT\n");
        assert!(Format::Hcl.parse(&bomb).is_err());
    }
}

#[test]
fn hcl_nested_object_in_object_key_dos_is_rejected_while_for_and_jsonencode_pass() {
    use std::fmt::Write as _;

    // C1: Depth-3 nested objects in key position (`O(K^(2D))` in `VecMap::eq`).
    let mut inner = String::from("{ ");
    for k in 0..39 {
        let _ = write!(inner, "a{k} = 1, ");
    }
    let mut nested = String::from("x = {\n");
    for outer in 0..40 {
        nested.push_str("  { ");
        for mid in 0..39 {
            let _ = write!(nested, "{inner}id = {mid} }} = 1, ");
        }
        let _ = writeln!(nested, "z = {outer} }} = 1");
    }
    nested.push_str("}\n");
    let err = Format::Hcl
        .parse(&nested)
        .expect_err("exponential nested object-in-key DoS must be rejected");
    assert!(
        err.contains("object") || err.contains("complexity") || err.contains("width"),
        "unexpected error: {err}"
    );

    // Value-side wide object inside a key object is also bounded.
    let mut val_key = String::from("x = {\n");
    let mut wide_val = String::from("{ ");
    for k in 0..70 {
        let _ = write!(wide_val, "k{k} = 1, ");
    }
    wide_val.push('}');
    for i in 0..5 {
        let _ = writeln!(val_key, "  {{ a = {wide_val}, z = {i} }} = 1");
    }
    val_key.push_str("}\n");
    assert!(Format::Hcl.parse(&val_key).is_err());

    // Legitimate small expression key and wide `{ for ... in { ... } : ... }`
    // comprehension remain accepted.
    let legit_key = r#"x = { jsonencode({ a = { b = 1 } }) = "v" }"#;
    assert!(Format::Hcl.parse(legit_key).is_ok());

    let mut for_expr = String::from("x = { for k, v in {\n");
    for i in 0..200 {
        let _ = writeln!(for_expr, "  k{i} = {i}");
    }
    for_expr.push_str("} : k => v }\n");
    assert!(Format::Hcl.parse(&for_expr).is_ok());
}

#[test]
fn hcl_adjacent_less_than_heredoc_cannot_bypass_object_width_guard() {
    use std::fmt::Write as _;

    for opener in ["<<<EOT", "<<<-EOT"] {
        let mut hcl = format!("x = a {opener}\n\"\nEOT\nz = {{\n");
        for i in 0..=MAX_HCL_OBJECT_ITEMS {
            let _ = writeln!(hcl, "  k{i} = {i}");
        }
        hcl.push_str("}\n");
        let err = Format::Hcl
            .parse(&hcl)
            .expect_err("`<` adjacent to heredoc must not bypass object width guard");
        assert!(
            err.contains("maximum supported width"),
            "opener={opener}: {err}"
        );
    }
}

#[test]
fn hcl_empty_heredoc_mid_file_and_buried_eof_backtracking_cannot_bypass_guard() {
    use std::fmt::Write as _;

    let mut wide_obj = String::from("z = {\n");
    for i in 0..=MAX_HCL_OBJECT_ITEMS {
        let _ = writeln!(wide_obj, "  k{i} = {i}");
    }
    wide_obj.push_str("}\n");

    // Repro 3a: inner unclosed scope (`# %{x"` and `# ${"`) buried above empty heredoc.
    for trigger in ["# %{x\"", "# ${\""] {
        let hcl = format!("a = <<EOT\nEOT\n{trigger}\n{wide_obj}");
        let err = Format::Hcl
            .parse(&hcl)
            .expect_err("buried empty heredoc EOF backtrack must enforce object width");
        assert!(
            err.contains("maximum supported width"),
            "trigger={trigger}: {err}"
        );
    }

    // Repro 3b: mid-file winnow backtrack (`# %{x}`, `# ${<}`) hidden by a later
    // delimiter inside a multiline string.
    for trigger in ["# %{x}", "# ${<}", "# %{else}"] {
        let hcl = format!("a = <<EOT\nEOT\n{trigger}\n{wide_obj}c = \"\nEOT\n\"\n");
        let err = Format::Hcl
            .parse(&hcl)
            .expect_err("mid-file empty heredoc backtrack must enforce object width");
        assert!(
            err.contains("maximum supported width"),
            "trigger={trigger}: {err}"
        );
    }
}

#[test]
fn hcl_hyphenated_blocks_legacy_splat_and_trailing_hyphen_idents_are_not_misclassified() {
    use std::fmt::Write as _;

    let mut attrs = String::new();
    for i in 0..=MAX_HCL_OBJECT_ITEMS {
        let _ = writeln!(attrs, "    k{i} = {i}");
    }

    // Hyphenated block label (`env us-east-1 { locals { ... } }`).
    let hyphen_block = format!("env us-east-1 {{\n  locals {{\n{attrs}  }}\n}}\n");
    assert!(Format::Hcl.parse(&hyphen_block).is_ok());

    // Preceding legacy attribute splat (`a.*`).
    let splat = format!("all = aws_instance.web.*\nlocals {{\n{attrs}}}\n");
    assert!(Format::Hcl.parse(&splat).is_ok());

    // Preceding identifier with trailing `-`.
    let trailing_dash = format!("x = foo-\nlocals {{\n{attrs}}}\n");
    assert!(Format::Hcl.parse(&trailing_dash).is_ok());
}

#[test]
fn hcl_large_value_objects_ternaries_and_multiple_empty_heredocs_pass_cleanly() {
    use std::fmt::Write as _;

    // H5: 17-entry object with a ~3.5 MiB heredoc value must NOT trip MAX_HCL_OBJECT_WORK.
    let mut big_val = String::from("x = {\n  policy = <<EOF\n");
    for _ in 0..56_000 {
        big_val.push_str("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde\n");
    }
    big_val.push_str("EOF\n");
    for i in 1..=17 {
        let _ = writeln!(big_val, "  k{i} = {i}");
    }
    big_val.push_str("}\n");
    assert!(Format::Hcl.parse(&big_val).is_ok());

    // H5: 500-entry lookup map where each value is a ~500 B nested object.
    let mut lookup = String::from("x = {\n");
    for i in 0..500 {
        let _ = writeln!(
            lookup,
            "  k{i} = {{ a = \"{}\", b = \"{}\", c = {i} }}",
            "x".repeat(200),
            "y".repeat(200)
        );
    }
    lookup.push_str("}\n");
    assert!(Format::Hcl.parse(&lookup).is_ok());

    // L10: 600-entry object with unparenthesized ternaries must NOT double-charge.
    let mut ternaries = String::from("x = {\n");
    for i in 0..600 {
        let _ = writeln!(ternaries, "  k{i} = var.flag ? \"yes\" : \"no\"");
    }
    ternaries.push_str("}\n");
    assert!(Format::Hcl.parse(&ternaries).is_ok());

    // M8 & M9: Multi-MiB file with multiple empty heredocs near the top passes
    // without exceeding rescan budget or multi-charging `total_work`.
    let mut multi_empty = String::from("a = <<EOF\nEOF\nb = <<EOT\nEOT\nc = <<END\nEND\n");
    multi_empty.push_str("big = <<BODY\n");
    for _ in 0..34_000 {
        multi_empty.push_str("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde\n");
    }
    multi_empty.push_str("BODY\n");
    assert!(multi_empty.len() > 2 * 1024 * 1024);
    assert!(Format::Hcl.parse(&multi_empty).is_ok());

    // L11: Invalid non-ASCII string escape `\é` must return Err cleanly without panicking.
    assert!(Format::Hcl.parse("x = \"\\é\"\n").is_err());
}
