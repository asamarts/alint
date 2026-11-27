//! Shared `CommonMark` structure projection for Markdown rule kinds.
//!
//! `pulldown-cmark` remains the grammar owner. This module only retains the
//! image, heading, and link fields needed by structural assertions, together
//! with source spans in the original (possibly BOM-prefixed) UTF-8 input.

use std::collections::HashSet;
use std::ops::Range;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use unicode_general_category::{GeneralCategory, get_general_category};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkdownImage {
    pub(crate) dest: String,
    pub(crate) alt: String,
    pub(crate) span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkdownHeading {
    pub(crate) level: u8,
    pub(crate) text: String,
    pub(crate) explicit_id: Option<String>,
    pub(crate) span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkdownLink {
    pub(crate) target: String,
    pub(crate) span: Range<usize>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct MarkdownDocument {
    pub(crate) images: Vec<MarkdownImage>,
    pub(crate) headings: Vec<MarkdownHeading>,
    pub(crate) links: Vec<MarkdownLink>,
}

#[derive(Debug, Clone, Copy)]
struct Projection {
    images: bool,
    headings: bool,
    links: bool,
}

#[cfg(test)]
const ALL: Projection = Projection {
    images: true,
    headings: true,
    links: true,
};

#[derive(Debug)]
struct OpenImage {
    dest: String,
    alt: String,
    span: Range<usize>,
}

#[derive(Debug)]
struct OpenHeading {
    level: u8,
    text: String,
    explicit_id: Option<String>,
    span: Range<usize>,
}

/// Parse live Markdown images. The caller validates UTF-8 before calling.
pub(crate) fn images(text: &str) -> Vec<MarkdownImage> {
    project(
        text,
        Projection {
            images: true,
            headings: false,
            links: false,
        },
    )
    .images
}

/// Parse live Markdown headings. The caller validates UTF-8 before calling.
pub(crate) fn headings(text: &str) -> Vec<MarkdownHeading> {
    project(
        text,
        Projection {
            images: false,
            headings: true,
            links: false,
        },
    )
    .headings
}

/// Parse live Markdown links. The caller validates UTF-8 before calling.
pub(crate) fn links(text: &str) -> Vec<MarkdownLink> {
    project(
        text,
        Projection {
            images: false,
            headings: false,
            links: true,
        },
    )
    .links
}

fn project(text: &str, projection: Projection) -> MarkdownDocument {
    let markdown = text.strip_prefix('\u{feff}').unwrap_or(text);
    let source_offset = text.len() - markdown.len();
    let mut options = Options::empty();
    options.insert(
        Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
            | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_HEADING_ATTRIBUTES,
    );

    let mut document = MarkdownDocument::default();
    let mut image_stack: Vec<OpenImage> = Vec::new();
    let mut heading: Option<OpenHeading> = None;

    for (event, span) in Parser::new_ext(markdown, options).into_offset_iter() {
        let span = span.start + source_offset..span.end + source_offset;
        match event {
            Event::Start(Tag::Image { dest_url, .. }) if projection.images => {
                image_stack.push(OpenImage {
                    dest: dest_url.into_string(),
                    alt: String::new(),
                    span,
                });
            }
            Event::End(TagEnd::Image) if projection.images => {
                if let Some(image) = image_stack.pop() {
                    document.images.push(MarkdownImage {
                        dest: image.dest,
                        alt: image.alt,
                        span: image.span,
                    });
                }
            }
            Event::Start(Tag::Heading { level, id, .. }) if projection.headings => {
                heading = Some(OpenHeading {
                    level: heading_level(level),
                    text: String::new(),
                    explicit_id: id.map(pulldown_cmark::CowStr::into_string),
                    span,
                });
            }
            Event::End(TagEnd::Heading(_)) if projection.headings => {
                if let Some(heading) = heading.take() {
                    document.headings.push(MarkdownHeading {
                        level: heading.level,
                        text: heading.text,
                        explicit_id: heading.explicit_id,
                        span: heading.span,
                    });
                }
            }
            Event::Start(Tag::Link { dest_url, .. }) if projection.links => {
                document.links.push(MarkdownLink {
                    target: dest_url.into_string(),
                    span,
                });
            }
            Event::Text(value) | Event::Code(value) => {
                append_text(&mut image_stack, heading.as_mut(), &value);
            }
            Event::SoftBreak | Event::HardBreak => {
                append_text(&mut image_stack, heading.as_mut(), " ");
            }
            _ => {}
        }
    }

    document
}

#[cfg(test)]
fn parse(text: &str) -> MarkdownDocument {
    project(text, ALL)
}

fn append_text(images: &mut [OpenImage], heading: Option<&mut OpenHeading>, text: &str) {
    // An image label contributes its rendered alt text to every containing image
    // and to a containing heading, matching the parser's visible-text projection.
    for image in images {
        image.alt.push_str(text);
    }
    if let Some(heading) = heading {
        heading.text.push_str(text);
    }
}

const fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// GitHub-style base heading anchor used by duplicate and fragment assertions.
pub(crate) fn heading_base_anchor(heading: &MarkdownHeading) -> String {
    heading
        .explicit_id
        .clone()
        .unwrap_or_else(|| slugify_heading(&heading.text))
}

pub(crate) fn slugify_heading(text: &str) -> String {
    use GeneralCategory::{
        DecimalNumber, EnclosingMark, LetterNumber, LowercaseLetter, ModifierLetter,
        NonspacingMark, OtherLetter, OtherNumber, SpacingMark, TitlecaseLetter, UppercaseLetter,
    };

    let mut slug = String::with_capacity(text.len());
    for ch in text.chars().flat_map(char::to_lowercase) {
        if ch == ' ' {
            slug.push('-');
        } else if ch == '-'
            || ch == '_'
            || matches!(
                get_general_category(ch),
                UppercaseLetter
                    | LowercaseLetter
                    | TitlecaseLetter
                    | ModifierLetter
                    | OtherLetter
                    | NonspacingMark
                    | SpacingMark
                    | EnclosingMark
                    | DecimalNumber
                    | LetterNumber
                    | OtherNumber
            )
        {
            slug.push(ch);
        }
    }
    slug
}

/// Return the concrete anchors generated in document order. Collisions use the
/// first available `-N` suffix, including collisions with naturally suffixed or
/// explicit ids that have already been emitted.
pub(crate) fn generated_anchors(headings: &[MarkdownHeading]) -> Vec<String> {
    let mut used = HashSet::with_capacity(headings.len());
    let mut anchors = Vec::with_capacity(headings.len());
    for heading in headings {
        let base = heading_base_anchor(heading);
        let mut anchor = base.clone();
        let mut suffix = 1usize;
        while used.contains(&anchor) {
            anchor.clear();
            anchor.push_str(&base);
            anchor.push('-');
            anchor.push_str(&suffix.to_string());
            suffix += 1;
        }
        used.insert(anchor.clone());
        anchors.push(anchor);
    }
    anchors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_ignores_code_and_retains_live_structure() {
        let text = concat!(
            "\u{feff}---\ntitle: '![](metadata.png)'\n---\n",
            "# A *heading* {#explicit}\n\n",
            "![**visible** `code`](x.png)\n\n",
            "`![no](inline-code.png)`\n",
            "<!-- ![](comment.png) -->\n",
            "```\n![](fenced.png)\n```\n",
        );
        let doc = parse(text);
        assert_eq!(doc.headings.len(), 1);
        assert_eq!(doc.headings[0].text, "A heading");
        assert_eq!(doc.headings[0].explicit_id.as_deref(), Some("explicit"));
        assert_eq!(doc.images.len(), 1);
        assert_eq!(doc.images[0].alt, "visible code");
        assert_eq!(doc.headings[0].span.start, text.find("# A").unwrap());
    }

    #[test]
    fn anchors_normalize_and_disambiguate_in_document_order() {
        let headings = [
            MarkdownHeading {
                level: 2,
                text: "Hello, World!".into(),
                explicit_id: None,
                span: 0..1,
            },
            MarkdownHeading {
                level: 2,
                text: "HELLO WORLD".into(),
                explicit_id: None,
                span: 1..2,
            },
            MarkdownHeading {
                level: 2,
                text: "hello-world-1".into(),
                explicit_id: None,
                span: 2..3,
            },
            MarkdownHeading {
                level: 2,
                text: "ignored".into(),
                explicit_id: Some("hello-world".into()),
                span: 3..4,
            },
        ];
        assert_eq!(
            generated_anchors(&headings),
            [
                "hello-world",
                "hello-world-1",
                "hello-world-1-1",
                "hello-world-2"
            ]
        );
    }

    #[test]
    fn slug_matches_github_for_unicode_punctuation_symbols_and_marks() {
        assert_eq!(slugify_heading("Über — API: v2!"), "über--api-v2");
        assert_eq!(slugify_heading("Ship 🚀 now"), "ship--now");
        assert_eq!(slugify_heading("Cafe\u{301}"), "cafe\u{301}");
        assert_eq!(slugify_heading("a\tb\u{a0}c"), "abc");
    }
}
