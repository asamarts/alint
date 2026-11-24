//! Stack-headroom, parity, width-guard, and adversarial regression tests for
//! bounded HCL2 parsing on `hcl-rs` 0.20 / `hcl-edit` 0.10 (#608).

use std::fmt::Write as _;

use super::Format;
use super::hcl::{MAX_HCL_BLOCK_LABELS, MAX_HCL_BYTES, MAX_HCL_OBJECT_ITEMS, parse_edit_body};

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
    // Release builds pass all 42 shapes at 640 KiB (tested at 1 MiB + 2 MiB);
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
            scalar_float = 3.5
            scalar_bool = true
            scalar_null = null
            scalar_str = "hello\nworld"
            arr = [1, "two", [3, 4], { nested = true }]
            obj = { a = 1, "b-key" = "v", deep = { k = [1, 2] } }
            expr_var = var.region
            expr_inf = inf
            expr_nan = nan
            expr_interp = "prefix-${var.env}-suffix"
            expr_call = upper("abc")
            expr_bin = 1 + 2 * 3
            expr_cond = true ? "yes" : "no"
            doc = <<-EOT
              line one
              line two
            EOT
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
}

#[test]
fn hcl_over_wide_object_is_rejected_while_large_bodies_and_heredocs_pass() {
    // Over-wide single object literal (> MAX_HCL_OBJECT_ITEMS) is rejected fast
    // instead of triggering `hcl-edit`'s O(N^2) VecMap insertion loop.
    let mut wide_obj = String::from("x = {\n");
    for i in 0..=MAX_HCL_OBJECT_ITEMS {
        let _ = writeln!(wide_obj, "k{i} = 1");
    }
    wide_obj.push_str("}\n");
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

    // Positive control 2: a large JSON policy heredoc inside a block has many
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

    // Positive control 3: banners / dashes / parens inside strings, heredocs,
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
fn hcl_over_labeled_block_is_rejected_not_a_stack_overflow() {
    // `hcl-edit` parses block labels iteratively into a Vec, so `hcl::from_str`'s
    // recursive `IntoJsonSpec` overflows on thousands of labels; our `body_to_value`
    // rejects blocks over `MAX_HCL_BLOCK_LABELS` cleanly.
    let too_many = format!(
        "b {} {{ x = 1 }}\n",
        "\"l\" ".repeat(MAX_HCL_BLOCK_LABELS + 1)
    );
    let err = Format::Hcl.parse(&too_many).unwrap_err();
    assert!(err.contains("label"), "over-labeled block rejected: {err}");

    let huge = format!("b {} {{ x = 1 }}\n", "\"l\" ".repeat(5_000));
    assert!(Format::Hcl.parse(&huge).is_err());

    let ok = format!("b {} {{ x = 1 }}\n", "\"l\" ".repeat(8));
    assert!(Format::Hcl.parse(&ok).is_ok());
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
