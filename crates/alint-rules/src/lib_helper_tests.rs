//! Tests of the crate root's helpers (`slash`, `non_string_extract_violation`).

#[test]
fn renders_forward_slashes_regardless_of_separator() {
    use std::path::Path;
    // On Linux `\` is a legal filename byte, so this exercises exactly the
    // replacement the fix relies on for Windows path separators.
    assert_eq!(super::slash(Path::new("a\\b\\c")), "a/b/c");
    assert_eq!(super::slash(Path::new("a/b/c")), "a/b/c");
}

#[test]
fn non_string_diagnostic_is_ordered_and_never_contains_values() {
    use alint_core::NonStringValueKind::{Boolean, Number, Object};
    use std::path::Path;

    let violation = super::non_string_extract_violation(
        Path::new("config.json"),
        "registry extract",
        &[Object, Number, Boolean, Number],
    )
    .expect("non-string matches produce a violation");

    assert_eq!(
        violation.message,
        "config.json: registry extract matched non-string value(s) \
         (boolean: 1, number: 2, object: 1); expected strings"
    );
    assert!(!violation.message.contains("do-not-report"));
    assert!(
        violation.not_fixable,
        "a typed extraction mismatch cannot be repaired by the owning rule's fixer"
    );
}
