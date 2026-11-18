//! A rule nested in an iteration reads the iterated entry's values as text
//! (ENG-1 of the third post-v0.22.1 audit). A kind that renders its `message:`
//! again for each finding (`{{ctx.primary}}`) has the braces of those values
//! marked by the iteration, so the list of such kinds must name every kind
//! whose rule renders a message template.

use std::collections::BTreeSet;
use std::path::Path;

/// The kinds whose source renders a message template: a file of this crate
/// that calls `render_message`, named for its kind (a `mod.rs` for its
/// directory).
fn kinds_rendering_message_templates() -> BTreeSet<String> {
    fn walk(dir: &Path, out: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if path.extension().is_none_or(|extension| extension != "rs")
                || name.ends_with("_tests.rs")
            {
                continue;
            }
            if !std::fs::read_to_string(&path)
                .unwrap()
                .contains("render_message(")
            {
                continue;
            }
            let kind = if name == "mod.rs" {
                path.parent().and_then(Path::file_name)
            } else {
                path.file_stem()
            };
            out.insert(kind.unwrap().to_string_lossy().into_owned());
        }
    }
    let mut out = BTreeSet::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
    out
}

#[test]
fn the_kinds_that_render_a_message_template_are_the_ones_the_iteration_marks() {
    let listed: BTreeSet<String> = alint_core::template::NESTED_MESSAGE_TEMPLATE_KINDS
        .iter()
        .map(|kind| (*kind).to_owned())
        .collect();
    assert_eq!(listed, kinds_rendering_message_templates());
    // The iteration looks a kind up by the spelling the configuration wrote, so
    // an alias of a listed kind would be rendered unmarked.
    let registry = crate::builtin_registry();
    for spelling in registry.known_kinds() {
        let canonical = registry.canonical_kind(spelling);
        assert_eq!(
            listed.contains(spelling),
            listed.contains(canonical),
            "`{spelling}` (canonical `{canonical}`)"
        );
    }
}
