//! Android string-resource and .NET `.resx` key rows (v0.21 R9).
//!
//! Both dialects parse through the same guarded XML path as every other XML
//! surface (`Format::Xml.parse` runs the depth, attribute-width and namespace
//! guards before `roxmltree` follows).

use crate::ByteSpan;
use crate::relation::adapter::{Adapter, AdapterError, AdapterOutput, check_row_count};
use crate::relation::limits::Limits;
use crate::relation::pointer::{Pointer, Segment};
use crate::relation::resource::Resource;
use crate::relation::row::{Row, RowState};
use crate::structured_format::Format;

/// Android `<resources>` string bundles.
///
/// A key is `(element type, name)`, so `<string name="x">` and
/// `<plurals name="x">` do not collide. `translatable="false"` entries exist
/// only in the default `values/` directory by design and are skipped.
/// `<string-array>` and `<plurals>` are one key and are never descended.
#[derive(Debug, Clone, Copy, Default)]
pub struct AndroidStringsAdapter;

impl AndroidStringsAdapter {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Adapter for AndroidStringsAdapter {
    fn id(&self) -> &'static str {
        "entry-android-strings"
    }

    fn extract(
        &self,
        resource: &Resource,
        bytes: &[u8],
        limits: &Limits,
    ) -> Result<AdapterOutput, AdapterError> {
        let text = xml_text(bytes)?;
        guarded_xml(text)?;
        let document = roxmltree::Document::parse(text)
            .map_err(|error| AdapterError::Parse(error.to_string()))?;
        let root = document.root_element();
        if root.tag_name().name() != "resources" {
            return Err(AdapterError::Parse(
                "Android string resources must have a <resources> root".to_string(),
            ));
        }
        let mut rows = Vec::new();
        for node in root.children().filter(roxmltree::Node::is_element) {
            let element = node.tag_name().name();
            if !matches!(element, "string" | "string-array" | "plurals") {
                continue;
            }
            let Some(name) = node.attribute_node("name") else {
                return Err(AdapterError::Parse(format!(
                    "Android <{element}> has no name attribute"
                )));
            };
            if node.attribute("translatable") == Some("false") {
                continue;
            }
            let span = name.range_value();
            check_row_count(limits, rows.len() + 1)?;
            rows.push(
                Row::new(
                    resource.clone(),
                    Some(Pointer::from_segments(vec![
                        Segment::Key(element.into()),
                        Segment::Key(name.value().into()),
                    ])),
                    u32::try_from(rows.len()).expect("the row limit is far below u32::MAX"),
                    RowState::Key,
                )
                .with_span(ByteSpan::new(span.start, span.end)),
            );
        }
        Ok(AdapterOutput::new(rows))
    }
}

/// .NET `.resx` string resources.
///
/// String `<data name="…">` entries are keys. Entries with `type` or
/// `mimetype` are non-string resources, and designer metadata (`>>…`, `$this…`)
/// is skipped. .NET falls back to the neutral file for missing keys, so
/// `direction: extra` is the usual key-parity setting for this dialect.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResxAdapter;

impl ResxAdapter {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Adapter for ResxAdapter {
    fn id(&self) -> &'static str {
        "entry-resx"
    }

    fn extract(
        &self,
        resource: &Resource,
        bytes: &[u8],
        limits: &Limits,
    ) -> Result<AdapterOutput, AdapterError> {
        let text = xml_text(bytes)?;
        guarded_xml(text)?;
        let document = roxmltree::Document::parse(text)
            .map_err(|error| AdapterError::Parse(error.to_string()))?;
        if document.root_element().tag_name().name() != "root" {
            return Err(AdapterError::Parse(
                ".resx resources must have a <root> element".to_string(),
            ));
        }
        let mut rows = Vec::new();
        for node in document
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "data")
        {
            if node.attribute("type").is_some() || node.attribute("mimetype").is_some() {
                continue;
            }
            let Some(name) = node.attribute_node("name") else {
                return Err(AdapterError::Parse(
                    ".resx <data> has no name attribute".to_string(),
                ));
            };
            if name.value().starts_with(">>") || name.value().starts_with("$this") {
                continue;
            }
            let span = name.range_value();
            check_row_count(limits, rows.len() + 1)?;
            rows.push(
                Row::new(
                    resource.clone(),
                    Some(Pointer::from_segments(vec![Segment::Key(
                        name.value().into(),
                    )])),
                    u32::try_from(rows.len()).expect("the row limit is far below u32::MAX"),
                    RowState::Key,
                )
                .with_span(ByteSpan::new(span.start, span.end)),
            );
        }
        Ok(AdapterOutput::new(rows))
    }
}

fn xml_text(bytes: &[u8]) -> Result<&str, AdapterError> {
    std::str::from_utf8(bytes)
        .map_err(|_| AdapterError::Parse("the XML document is not valid UTF-8".to_string()))
}

fn guarded_xml(text: &str) -> Result<(), AdapterError> {
    Format::Xml
        .parse(text)
        .map(|_| ())
        .map_err(AdapterError::Parse)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource() -> Resource {
        Resource::for_test(0, "strings.xml")
    }

    fn rows(adapter: &dyn Adapter, text: &str) -> AdapterOutput {
        adapter
            .extract(&resource(), text.as_bytes(), &Limits::default())
            .expect("XML extracts")
    }

    fn rendered(output: &AdapterOutput) -> Vec<String> {
        output
            .rows
            .iter()
            .map(|row| row.pointer.as_ref().unwrap().render())
            .collect()
    }

    #[test]
    fn android_strings_key_by_element_and_name_and_skip_untranslatable() {
        let output = rows(
            &AndroidStringsAdapter::new(),
            r#"<resources>
                 <string name="app">App</string>
                 <string name="build" translatable="false">1</string>
                 <string-array name="tabs"><item>One</item></string-array>
                 <plurals name="items"><item quantity="other">%d items</item></plurals>
               </resources>"#,
        );
        assert_eq!(
            rendered(&output),
            ["string.app", "string-array.tabs", "plurals.items"]
        );
        let app = output
            .rows
            .iter()
            .find(|row| row.pointer.as_ref().unwrap().render() == "string.app")
            .unwrap();
        let text = r#"<resources>
                 <string name="app">App</string>
                 <string name="build" translatable="false">1</string>
                 <string-array name="tabs"><item>One</item></string-array>
                 <plurals name="items"><item quantity="other">%d items</item></plurals>
               </resources>"#;
        assert_eq!(&text[app.span.unwrap().as_range()], "app");
    }

    #[test]
    fn resx_strings_skip_non_strings_and_designer_metadata() {
        let output = rows(
            &ResxAdapter::new(),
            r#"<root>
                 <data name="Greeting"><value>Hello</value></data>
                 <data name="Icon" type="System.Drawing.Bitmap, System.Drawing" />
                 <data name="Other" mimetype="application/x" />
                 <data name="&gt;&gt;Designer"><value>meta</value></data>
                 <data name="$this.Icon"><value>meta</value></data>
               </root>"#,
        );
        assert_eq!(rendered(&output), ["Greeting"]);
    }

    #[test]
    fn malformed_and_wrong_root_documents_fail_loudly() {
        assert!(
            AndroidStringsAdapter::new()
                .extract(&resource(), b"<resources>", &Limits::default())
                .is_err()
        );
        assert!(
            AndroidStringsAdapter::new()
                .extract(&resource(), b"<root/>", &Limits::default())
                .is_err()
        );
        assert!(
            ResxAdapter::new()
                .extract(&resource(), b"<resources/>", &Limits::default())
                .is_err()
        );
    }

    #[test]
    fn the_guarded_xml_depth_limit_is_exercised() {
        let depth = crate::structured_format::MAX_XML_DEPTH + 1;
        let text = format!("{}x{}", "<a>".repeat(depth), "</a>".repeat(depth));
        let error = AndroidStringsAdapter::new()
            .extract(&resource(), text.as_bytes(), &Limits::default())
            .unwrap_err();
        assert!(error.to_string().contains("depth"), "{error}");
    }

    #[test]
    fn the_row_limit_is_enforced() {
        let limits = Limits::default()
            .with(crate::relation::LimitKind::RowsPerExtractor, 1)
            .unwrap();
        let error = AndroidStringsAdapter::new()
            .extract(
                &resource(),
                b"<resources><string name=\"a\">a</string><string name=\"b\">b</string></resources>",
                &limits,
            )
            .unwrap_err();
        assert!(matches!(error, AdapterError::Limit(_)), "{error}");
    }
}
