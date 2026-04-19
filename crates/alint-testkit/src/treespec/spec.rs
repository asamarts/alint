//! Core data types for the tree-spec format.
//!
//! Format documented in [`crates/alint-testkit/TREE_SPEC.md`]. Kept
//! free of any alint-specific types so this module can be spun out
//! as a standalone crate without refactoring.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A node in a tree spec: either a file (scalar) or a directory
/// (mapping of child names to nodes).
///
/// Serde `untagged` round-trip: a YAML scalar deserializes to
/// [`TreeNode::File`] and a YAML mapping deserializes to
/// [`TreeNode::Dir`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum TreeNode {
    File(String),
    Dir(BTreeMap<String, TreeNode>),
}

impl TreeNode {
    pub fn is_file(&self) -> bool {
        matches!(self, TreeNode::File(_))
    }

    pub fn is_dir(&self) -> bool {
        matches!(self, TreeNode::Dir(_))
    }
}

/// A tree-spec root: always a directory at the top level. The
/// outer map's keys are the children of the materialization root.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct TreeSpec {
    pub root: BTreeMap<String, TreeNode>,
}

impl TreeSpec {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a YAML tree spec.
    pub fn from_yaml(yaml: &str) -> Result<Self, serde_yaml_ng::Error> {
        serde_yaml_ng::from_str(yaml)
    }

    /// Emit the spec as YAML.
    pub fn to_yaml(&self) -> Result<String, serde_yaml_ng::Error> {
        serde_yaml_ng::to_string(self)
    }

    /// Iterate over (relative-path, node) pairs in a stable order.
    /// The paths are slash-separated and never lead with a slash.
    pub fn iter(&self) -> TreeSpecIter<'_> {
        self.into_iter()
    }
}

impl<'a> IntoIterator for &'a TreeSpec {
    type Item = (String, &'a TreeNode);
    type IntoIter = TreeSpecIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        TreeSpecIter {
            stack: vec![(String::new(), &self.root)],
            child_iter: None,
        }
    }
}

/// Depth-first iterator yielding every node (file or directory) in
/// the spec, with its relative path from the tree root.
#[derive(Debug)]
pub struct TreeSpecIter<'a> {
    stack: Vec<(String, &'a BTreeMap<String, TreeNode>)>,
    child_iter: Option<(
        String,
        std::collections::btree_map::Iter<'a, String, TreeNode>,
    )>,
}

impl<'a> Iterator for TreeSpecIter<'a> {
    type Item = (String, &'a TreeNode);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some((prefix, iter)) = &mut self.child_iter {
                if let Some((name, node)) = iter.next() {
                    let path = if prefix.is_empty() {
                        name.clone()
                    } else {
                        format!("{prefix}/{name}")
                    };
                    if let TreeNode::Dir(children) = node {
                        self.stack.push((path.clone(), children));
                    }
                    return Some((path, node));
                }
                self.child_iter = None;
            }
            let (prefix, children) = self.stack.pop()?;
            self.child_iter = Some((prefix, children.iter()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_yaml_mapping() {
        let src = r#"
Cargo.toml: "[package]\nname = \"demo\"\n"
src:
  main.rs: "fn main() {}\n"
  lib.rs: ""
docs: {}
"#;
        let spec = TreeSpec::from_yaml(src).unwrap();
        assert!(spec.root.contains_key("Cargo.toml"));
        let src_node = &spec.root["src"];
        let TreeNode::Dir(src_children) = src_node else {
            panic!("expected src to be a directory");
        };
        assert_eq!(
            src_children["main.rs"],
            TreeNode::File("fn main() {}\n".to_string())
        );
        assert_eq!(src_children["lib.rs"], TreeNode::File(String::new()));
        let docs_node = &spec.root["docs"];
        assert!(matches!(docs_node, TreeNode::Dir(m) if m.is_empty()));
    }

    #[test]
    fn iter_yields_every_node_with_its_path() {
        let src = r#"
a.txt: "alpha"
nested:
  b.txt: "bravo"
  sub:
    c.txt: "charlie"
"#;
        let spec = TreeSpec::from_yaml(src).unwrap();
        let paths: Vec<String> = spec.iter().map(|(p, _)| p).collect();
        // BTreeMap orders alphabetically; directory children come
        // after the directory itself due to the DFS stack shape.
        assert!(paths.contains(&"a.txt".to_string()));
        assert!(paths.contains(&"nested".to_string()));
        assert!(paths.contains(&"nested/b.txt".to_string()));
        assert!(paths.contains(&"nested/sub".to_string()));
        assert!(paths.contains(&"nested/sub/c.txt".to_string()));
    }

    #[test]
    fn round_trip_through_yaml_preserves_structure() {
        let src = r#"
a:
  b:
    c.txt: "x"
z.txt: "y"
"#;
        let spec = TreeSpec::from_yaml(src).unwrap();
        let yaml = spec.to_yaml().unwrap();
        let re = TreeSpec::from_yaml(&yaml).unwrap();
        assert_eq!(spec, re);
    }
}
