//! Parse functor: S → C.
//!
//! Discovers site structure from the filesystem and parses markdown
//! frontmatter into typed content values. This module implements the
//! Parse phase of the compiler pipeline.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::{fmt, fs};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::error::{ParseError, ParseResult, Result};

/// Conventional filename for section index pages.
pub const SECTION_INDEX: &str = "_index.md";
/// Conventional filename for the custom 404 page.
pub const PAGE_404: &str = "_404.md";
/// Output filename for directory index pages.
pub const OUTPUT_INDEX: &str = "index.html";

/// The type of content being processed.
#[derive(Debug, Clone, PartialEq)]
pub enum ContentKind {
    /// Blog post with full metadata (date, tags, etc.)
    Post,
    /// Standalone page (about, collab)
    Page,
    /// Section index (_index.md)
    Section,
    /// Project card with external link
    Project,
}

// ============================================================================
// Category C types — formal model alignment
// ============================================================================

/// A structured block of content parsed from markdown.
///
/// Represents the coproduct from the formal model. Each variant corresponds
/// to a build-time interception point in the rendering pipeline.
///
/// Five variants: four intercepted (Code, Math, Diagram, Heading) and one
/// identity case (Prose). This is the minimal coproduct — every variant
/// earns its existence. See `docs/models/sukr-compiler.md` §Content Block Algebra.
#[derive(Debug, Clone, PartialEq)]
pub enum ContentBlock {
    /// Fenced code block with optional language annotation.
    Code {
        language: Option<String>,
        source: String,
    },
    /// Mathematical expression (pulldown-latex). `display` = block vs inline.
    Math { source: String, display: bool },
    /// Diagram source (Mermaid).
    Diagram { source: String },
    /// Heading with computed slug for anchor navigation.
    Heading { level: u8, text: String, id: String },
    /// Standard-rendered HTML content (identity in the Render catamorphism).
    /// Paragraphs, lists, inline text, links, images, etc. — everything the
    /// parser library renders correctly without sukr interception.
    Prose(String),
}

/// A content tag — validated newtype over String.
///
/// Parse-don't-validate: constructed once, used everywhere with
/// compile-time type safety. No bare `String` can be confused for a tag.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Tag(String);

#[cfg(test)]
impl Tag {
    /// Create a new tag from any string-like value (test-only).
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Access the inner tag string (test-only).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Tag {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Serialize for Tag {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Tag {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        String::deserialize(deserializer).map(Tag)
    }
}

/// Section type for exhaustive dispatch on sort strategy and template selection.
///
/// Replaces bare string comparisons against "blog", "projects", etc.
/// Adding a new section type forces handling at every match site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SectionType {
    Blog,
    Projects,
    Custom(String),
}

impl SectionType {
    /// Convert a string into a typed section kind.
    pub fn from_str(s: &str) -> Self {
        match s {
            "blog" => Self::Blog,
            "projects" => Self::Projects,
            other => Self::Custom(other.to_string()),
        }
    }
}

impl fmt::Display for SectionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blog => f.write_str("blog"),
            Self::Projects => f.write_str("projects"),
            Self::Custom(s) => f.write_str(s),
        }
    }
}

impl<'de> Deserialize<'de> for SectionType {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        String::deserialize(deserializer).map(|s| Self::from_str(&s))
    }
}

/// Sort key for ordered content collections.
///
/// Used as the key type in `BTreeMap<SortKey, Content>` for
/// sorted-by-construction section items. The variant is determined
/// by `SectionType` at construction time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortKey {
    /// Blog-style: sort by date, newest first. Falls back to weight
    /// for undated items.
    DateDesc(NaiveDate),
    /// Weight-based: sort by weight ascending, then title ascending.
    /// Used for projects and custom sections.
    WeightTitle(i64, String),
}

impl SortKey {
    /// Default weight when none is specified in frontmatter.
    pub const DEFAULT_WEIGHT: i64 = 50;

    /// Construct the appropriate sort key for a content item based on
    /// its section type and frontmatter.
    pub fn for_content(section_type: &SectionType, frontmatter: &Frontmatter) -> Self {
        match section_type {
            SectionType::Blog => {
                if let Some(date) = frontmatter.date {
                    Self::DateDesc(date)
                } else {
                    // Undated blog posts sort by weight+title as fallback
                    Self::WeightTitle(
                        frontmatter.weight.unwrap_or(Self::DEFAULT_WEIGHT),
                        frontmatter.title.clone(),
                    )
                }
            },
            _ => Self::WeightTitle(
                frontmatter.weight.unwrap_or(Self::DEFAULT_WEIGHT),
                frontmatter.title.clone(),
            ),
        }
    }
}

impl Ord for SortKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            // Newest date first (reverse chronological)
            (Self::DateDesc(a), Self::DateDesc(b)) => b.cmp(a),
            // Weight ascending, then title ascending
            (Self::WeightTitle(wa, ta), Self::WeightTitle(wb, tb)) => {
                wa.cmp(wb).then_with(|| ta.cmp(tb))
            },
            // DateDesc sorts before WeightTitle (dated items first)
            (Self::DateDesc(_), Self::WeightTitle(..)) => Ordering::Less,
            (Self::WeightTitle(..), Self::DateDesc(_)) => Ordering::Greater,
        }
    }
}

impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// An inter-page reference discovered during parsing.
///
/// Populated as a side-channel by `parse_blocks()` during markdown parsing.
/// Internal links are identified by the absence of a URL scheme.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkTarget {
    /// The URL or relative path as written in markdown.
    pub url: String,
    /// Whether this is an internal (relative) reference vs external URL.
    pub is_internal: bool,
}

/// Parse markdown body into structured content blocks and extracted link targets.
///
/// Walks pulldown-cmark events and separates intercepted blocks (Code, Math,
/// Diagram, Heading) from standard-rendered prose. Non-intercepted content is
/// rendered to HTML during parsing and emitted as `Prose` blocks. Internal link
/// URLs are extracted as a side-channel for reference validation.
///
/// Returns `(blocks, links)` where blocks are the content block sequence and
/// links are the extracted reference targets.
pub fn parse_blocks(markdown: &str) -> (Vec<ContentBlock>, Vec<LinkTarget>) {
    use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_MATH;

    let parser = Parser::new_ext(markdown, options);
    let mut blocks = Vec::new();
    let mut links = Vec::new();

    // Prose accumulator — flushes to a Prose block when an intercepted block starts
    let mut prose_buf = String::new();

    // Accumulation state for multi-event intercepted blocks
    let mut code_lang: Option<String> = None;
    let mut code_buf = String::new();
    let mut in_code = false;

    let mut heading_level: Option<u8> = None;
    let mut heading_buf = String::new();
    let mut heading_plain_buf = String::new();

    // Image alt text accumulation (images render to Prose but extract links)
    let mut image_alt_buf: Option<String> = None;
    let mut image_attrs: Option<(String, String)> = None; // (src, title)

    // Footnote numbering: GitHub-style sequential numbers regardless of label
    let mut footnote_counter: usize = 0;
    let mut footnote_numbers: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut footnote_name: Option<String> = None;
    let mut footnote_buf = String::new(); // accumulates HTML inside a definition
    let mut footnote_defs: Vec<(String, String)> = Vec::new(); // (label, html)

    /// Flush accumulated prose to a Prose block if non-empty.
    fn flush_prose(buf: &mut String, blocks: &mut Vec<ContentBlock>) {
        if !buf.is_empty() {
            blocks.push(ContentBlock::Prose(std::mem::take(buf)));
        }
    }

    for event in parser {
        match event {
            // =================================================================
            // Intercepted: Code blocks
            // =================================================================
            Event::Start(Tag::CodeBlock(kind)) => {
                flush_prose(&mut prose_buf, &mut blocks);
                code_lang = match kind {
                    CodeBlockKind::Fenced(lang) => {
                        let lang_str = lang.split_whitespace().next().unwrap_or("");
                        if lang_str.is_empty() {
                            None
                        } else {
                            Some(lang_str.to_string())
                        }
                    },
                    CodeBlockKind::Indented => None,
                };
                in_code = true;
                code_buf.clear();
            },
            Event::Text(text) if in_code => code_buf.push_str(&text),
            Event::End(TagEnd::CodeBlock) => {
                let block = if code_lang.as_deref() == Some("mermaid") {
                    ContentBlock::Diagram {
                        source: std::mem::take(&mut code_buf),
                    }
                } else {
                    ContentBlock::Code {
                        language: code_lang.take(),
                        source: std::mem::take(&mut code_buf),
                    }
                };
                blocks.push(block);
                code_lang = None;
                in_code = false;
            },

            // =================================================================
            // Intercepted: Headings
            // =================================================================
            Event::Start(Tag::Heading { level, .. }) => {
                flush_prose(&mut prose_buf, &mut blocks);
                heading_level = Some(level as u8);
                heading_buf.clear();
                heading_plain_buf.clear();
            },
            Event::Text(text) if heading_level.is_some() => {
                heading_buf.push_str(&text);
                heading_plain_buf.push_str(&text);
            },
            Event::Code(text) if heading_level.is_some() => {
                heading_buf.push_str("<code>");
                heading_buf.push_str(&crate::escape::html_escape(&text));
                heading_buf.push_str("</code>");
                heading_plain_buf.push_str(&text);
            },
            Event::Start(Tag::Emphasis) if heading_level.is_some() => {
                heading_buf.push_str("<em>");
            },
            Event::End(TagEnd::Emphasis) if heading_level.is_some() => {
                heading_buf.push_str("</em>");
            },
            Event::Start(Tag::Strong) if heading_level.is_some() => {
                heading_buf.push_str("<strong>");
            },
            Event::End(TagEnd::Strong) if heading_level.is_some() => {
                heading_buf.push_str("</strong>");
            },
            Event::Start(Tag::Strikethrough) if heading_level.is_some() => {
                heading_buf.push_str("<del>");
            },
            Event::End(TagEnd::Strikethrough) if heading_level.is_some() => {
                heading_buf.push_str("</del>");
            },
            Event::End(TagEnd::Heading(_)) => {
                if let Some(level) = heading_level.take() {
                    let id = crate::render::slugify(&heading_plain_buf);
                    blocks.push(ContentBlock::Heading {
                        level,
                        text: std::mem::take(&mut heading_buf),
                        id,
                    });
                }
            },

            // =================================================================
            // Intercepted: Math
            // =================================================================
            Event::InlineMath(latex) => {
                if footnote_name.is_some() {
                    // We are accumulating a footnote. Render eagerly and push to prose_buf.
                    // This avoids breaking the footnote content model which expects HTML.
                    match crate::math::render_math(&latex, false) {
                        Ok(mathml) => prose_buf.push_str(&mathml),
                        Err(e) => prose_buf
                            .push_str(&format!("<span class=\"error\">Math error: {}</span>", e)),
                    }
                } else {
                    flush_prose(&mut prose_buf, &mut blocks);
                    blocks.push(ContentBlock::Math {
                        source: latex.to_string(),
                        display: false,
                    });
                }
            },
            Event::DisplayMath(latex) => {
                if footnote_name.is_some() {
                    match crate::math::render_math(&latex, true) {
                        Ok(mathml) => prose_buf.push_str(&mathml),
                        Err(e) => prose_buf
                            .push_str(&format!("<span class=\"error\">Math error: {}</span>", e)),
                    }
                } else {
                    flush_prose(&mut prose_buf, &mut blocks);
                    blocks.push(ContentBlock::Math {
                        source: latex.to_string(),
                        display: true,
                    });
                }
            },

            // =================================================================
            // Prose: Images (render to HTML, extract link as side-channel)
            // =================================================================
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                image_alt_buf = Some(String::new());
                image_attrs = Some((dest_url.to_string(), title.to_string()));
                // Extract link target
                let url = dest_url.to_string();
                if !url.starts_with('#') {
                    let is_internal = !url.contains("://")
                        && !url.starts_with("mailto:")
                        && !url.starts_with("data:");
                    links.push(LinkTarget { url, is_internal });
                }
            },
            Event::Text(text) if image_alt_buf.is_some() => {
                if let Some(ref mut alt) = image_alt_buf {
                    alt.push_str(&text);
                }
            },
            Event::End(TagEnd::Image) => {
                let alt = image_alt_buf.take().unwrap_or_default();
                if let Some((src, title)) = image_attrs.take() {
                    if title.is_empty() {
                        prose_buf.push_str(&format!(
                            "<img src=\"{}\" alt=\"{}\" />",
                            crate::escape::html_escape(&src),
                            crate::escape::html_escape(&alt)
                        ));
                    } else {
                        prose_buf.push_str(&format!(
                            "<img src=\"{}\" alt=\"{}\" title=\"{}\" />",
                            crate::escape::html_escape(&src),
                            crate::escape::html_escape(&alt),
                            crate::escape::html_escape(&title)
                        ));
                    }
                }
            },

            // =================================================================
            // Prose: Links (render to HTML, extract link as side-channel)
            // =================================================================
            Event::Start(Tag::Link {
                dest_url, title, ..
            }) => {
                // Extract link target
                let url = dest_url.to_string();
                if !url.starts_with('#') {
                    let is_internal = !url.contains("://")
                        && !url.starts_with("mailto:")
                        && !url.starts_with("data:");
                    links.push(LinkTarget { url, is_internal });
                }
                // Render opening <a> tag
                if title.is_empty() {
                    prose_buf.push_str(&format!(
                        "<a href=\"{}\">",
                        crate::escape::html_escape(&dest_url)
                    ));
                } else {
                    prose_buf.push_str(&format!(
                        "<a href=\"{}\" title=\"{}\">",
                        crate::escape::html_escape(&dest_url),
                        crate::escape::html_escape(&title)
                    ));
                }
            },
            Event::End(TagEnd::Link) => {
                prose_buf.push_str("</a>");
            },

            // =================================================================
            // Prose: Structural HTML (paragraphs, lists, etc.)
            // =================================================================
            Event::Start(Tag::Paragraph) => prose_buf.push_str("<p>"),
            Event::End(TagEnd::Paragraph) => prose_buf.push_str("</p>\n"),
            Event::Start(Tag::BlockQuote(_)) => prose_buf.push_str("<blockquote>\n"),
            Event::End(TagEnd::BlockQuote(_)) => prose_buf.push_str("</blockquote>\n"),
            Event::Start(Tag::List(Some(start))) => {
                prose_buf.push_str(&format!("<ol start=\"{}\">\n", start));
            },
            Event::Start(Tag::List(None)) => prose_buf.push_str("<ul>\n"),
            Event::End(TagEnd::List(ordered)) => {
                if ordered {
                    prose_buf.push_str("</ol>\n");
                } else {
                    prose_buf.push_str("</ul>\n");
                }
            },
            Event::Start(Tag::Item) => prose_buf.push_str("<li>"),
            Event::End(TagEnd::Item) => prose_buf.push_str("</li>\n"),
            Event::Start(Tag::Table(_)) => prose_buf.push_str("<table>\n"),
            Event::End(TagEnd::Table) => prose_buf.push_str("</table>\n"),
            Event::Start(Tag::TableHead) => prose_buf.push_str("<thead>\n<tr>\n"),
            Event::End(TagEnd::TableHead) => prose_buf.push_str("</tr>\n</thead>\n"),
            Event::Start(Tag::TableRow) => prose_buf.push_str("<tr>\n"),
            Event::End(TagEnd::TableRow) => prose_buf.push_str("</tr>\n"),
            Event::Start(Tag::TableCell) => prose_buf.push_str("<td>"),
            Event::End(TagEnd::TableCell) => prose_buf.push_str("</td>\n"),
            Event::Start(Tag::Emphasis) => prose_buf.push_str("<em>"),
            Event::End(TagEnd::Emphasis) => prose_buf.push_str("</em>"),
            Event::Start(Tag::Strong) => prose_buf.push_str("<strong>"),
            Event::End(TagEnd::Strong) => prose_buf.push_str("</strong>"),
            Event::Start(Tag::Strikethrough) => prose_buf.push_str("<del>"),
            Event::End(TagEnd::Strikethrough) => prose_buf.push_str("</del>"),
            Event::Start(Tag::FootnoteDefinition(name)) => {
                // Flush any pending prose before switching to footnote buffer
                flush_prose(&mut prose_buf, &mut blocks);
                // Swap: prose_buf becomes the footnote accumulator
                std::mem::swap(&mut prose_buf, &mut footnote_buf);
                prose_buf.clear();
                footnote_name = Some(name.to_string());
            },
            Event::End(TagEnd::FootnoteDefinition) => {
                if let Some(name) = footnote_name.take() {
                    // prose_buf currently holds the footnote content
                    let content = std::mem::take(&mut prose_buf);
                    // Swap back: restore the real prose_buf
                    std::mem::swap(&mut prose_buf, &mut footnote_buf);
                    footnote_defs.push((name, content));
                }
            },
            Event::Start(Tag::DefinitionList) => prose_buf.push_str("<dl>"),
            Event::End(TagEnd::DefinitionList) => prose_buf.push_str("</dl>\n"),
            Event::Start(Tag::DefinitionListTitle) => prose_buf.push_str("<dt>"),
            Event::End(TagEnd::DefinitionListTitle) => prose_buf.push_str("</dt>\n"),
            Event::Start(Tag::DefinitionListDefinition) => prose_buf.push_str("<dd>"),
            Event::End(TagEnd::DefinitionListDefinition) => prose_buf.push_str("</dd>\n"),
            Event::Start(Tag::HtmlBlock) | Event::End(TagEnd::HtmlBlock) => {},
            Event::Start(Tag::MetadataBlock(_)) | Event::End(TagEnd::MetadataBlock(_)) => {},

            // =================================================================
            // Prose: Inline content
            // =================================================================
            Event::Text(text) => {
                prose_buf.push_str(&crate::escape::html_escape(&text));
            },
            Event::Code(text) => {
                prose_buf.push_str("<code>");
                prose_buf.push_str(&crate::escape::html_escape(&text));
                prose_buf.push_str("</code>");
            },
            Event::Html(html) | Event::InlineHtml(html) => {
                prose_buf.push_str(&html);
            },
            Event::SoftBreak => prose_buf.push('\n'),
            Event::HardBreak => prose_buf.push_str("<br />\n"),
            Event::Rule => {
                flush_prose(&mut prose_buf, &mut blocks);
                blocks.push(ContentBlock::Prose("<hr />\n".to_string()));
            },
            Event::FootnoteReference(name) => {
                let num = footnote_numbers
                    .get(name.as_ref())
                    .copied()
                    .unwrap_or_else(|| {
                        footnote_counter += 1;
                        footnote_numbers.insert(name.to_string(), footnote_counter);
                        footnote_counter
                    });
                prose_buf.push_str(&format!(
                    "<sup class=\"footnote-ref\" id=\"fn-ref-{}\"><a href=\"#fn-{}\" \
                     data-footnote=\"{}\" aria-label=\"Footnote {}\"></a></sup>",
                    num, num, num, num
                ));
            },
            Event::TaskListMarker(checked) => {
                if checked {
                    prose_buf.push_str("<input type=\"checkbox\" checked disabled />");
                } else {
                    prose_buf.push_str("<input type=\"checkbox\" disabled />");
                }
            },
        }
    }

    flush_prose(&mut prose_buf, &mut blocks);

    // Emit footnotes sorted by reference order (assigned number)
    if !footnote_defs.is_empty() {
        let mut sorted_fns: Vec<(usize, String, String)> = footnote_defs
            .into_iter()
            .map(|(label, html)| {
                let num = footnote_numbers.get(&label).copied().unwrap_or(0);
                (num, label, html)
            })
            .collect();
        sorted_fns.sort_by_key(|(num, ..)| *num);

        let mut fn_html = String::from("<aside class=\"footnotes\">\n");
        for (num, _label, content) in &sorted_fns {
            fn_html.push_str(&format!(
                "<div class=\"footnote\" id=\"fn-{}\"><span class=\"footnote-num\">[{}]</span> {}",
                num, num, content
            ));
            fn_html.push_str(&format!(
                " <a href=\"#fn-ref-{}\" class=\"footnote-backref\" title=\"Back to \
                 text\">↩</a></div>\n",
                num
            ));
        }
        fn_html.push_str("</aside>\n");
        blocks.push(ContentBlock::Prose(fn_html));
    }
    (blocks, links)
}

/// Normalize a markdown internal link URL to a canonical output path.
///
/// Strips fragments (`#section`), adds `.html` suffix if missing,
/// and ensures a leading `/` for consistency with output_path format.
fn normalize_link_url(url: &str) -> String {
    // Strip fragment
    let path = url.split('#').next().unwrap_or(url);
    if path.is_empty() {
        return String::new();
    }

    // Ensure leading /
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    };

    // Add .html if no extension present
    if Path::new(&path).extension().is_none() {
        format!("{}.html", path)
    } else {
        path
    }
}

/// A navigation menu item discovered from the filesystem.
#[derive(Debug, Clone, Serialize)]
pub struct NavItem {
    /// Display label (from nav_label or title)
    pub label: String,
    /// URL path (e.g., "/blog/index.html" or "/about.html")
    pub path: String,
    /// Sort order (lower = first, default 50)
    pub weight: i64,
    /// Child navigation items (section pages)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<NavItem>,
}

impl Eq for NavItem {}

/// Equality compares sort discriminants only (`weight`, `label`).
/// `path` and `children` are excluded intentionally — two nav items at the
/// same position with the same label are considered duplicates regardless
/// of their subtree or target path.
impl PartialEq for NavItem {
    fn eq(&self, other: &Self) -> bool {
        self.weight == other.weight && self.label == other.label
    }
}

impl Ord for NavItem {
    fn cmp(&self, other: &Self) -> Ordering {
        self.weight
            .cmp(&other.weight)
            .then_with(|| self.label.cmp(&other.label))
    }
}

impl PartialOrd for NavItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Parsed frontmatter from a content file.
#[derive(Debug, Clone, Deserialize)]
pub struct Frontmatter {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "deserialize_date")]
    pub date: Option<NaiveDate>,
    #[serde(default)]
    pub tags: Vec<Tag>,
    /// Sort order for nav and listings
    #[serde(default)]
    pub weight: Option<i64>,
    /// For project cards: external link
    #[serde(default)]
    pub link_to: Option<String>,
    /// Custom navigation label (defaults to title)
    #[serde(default)]
    pub nav_label: Option<String>,
    /// Section type for template dispatch (e.g., "blog", "projects")
    #[serde(default)]
    pub section_type: Option<SectionType>,
    /// Override template for this content item
    #[serde(default)]
    pub template: Option<String>,
    /// Enable table of contents (anchor nav in sidebar)
    #[serde(default)]
    pub toc: Option<bool>,
    /// Whether this content is a draft (excluded from output)
    #[serde(default)]
    pub draft: bool,
    /// Alternative URL paths that redirect to this content
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// A content item ready for rendering.
#[derive(Debug, Clone)]
pub struct Content {
    pub frontmatter: Frontmatter,
    pub body: String,
    pub source_path: PathBuf,
    pub slug: String,
    /// Output path relative to the output directory, computed once during Parse.
    pub output_path: PathBuf,
    /// Structured content blocks (populated in Phase 2).
    pub blocks: Vec<ContentBlock>,
    /// Inter-page references (populated in Phase 2).
    pub links: Vec<LinkTarget>,
}

impl Content {
    /// Load and parse a markdown file with TOML frontmatter.
    pub fn from_path(
        path: impl AsRef<Path>,
        kind: ContentKind,
        content_root: &Path,
    ) -> Result<Self> {
        Self::from_path_inner(path.as_ref(), kind, content_root)
    }

    fn from_path_inner(path: &Path, kind: ContentKind, content_root: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path).map_err(|e| ParseError::ReadFile {
            path: path.to_path_buf(),
            source: e,
        })?;

        let (toml_block, body) = extract_frontmatter(&raw, path)?;
        let frontmatter = parse_frontmatter(path, &toml_block)?;

        // Derive slug from filename (without extension)
        let slug = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled")
            .to_string();

        // Compute output path once during Parse, storing as a field.
        let output_path = {
            let relative = path.strip_prefix(content_root).unwrap_or(path);

            match kind {
                ContentKind::Section => {
                    // _index.md → parent/index.html
                    let parent = relative.parent().unwrap_or(Path::new(""));
                    parent.join(OUTPUT_INDEX)
                },
                _ => {
                    // Regular content → parent/slug.html
                    let parent = relative.parent().unwrap_or(Path::new(""));
                    parent.join(format!("{}.html", slug))
                },
            }
        };

        let page_path = format!("/{}", output_path.display());
        let depth = page_path.matches('/').count().saturating_sub(1);
        let prefix = if depth == 0 {
            ".".to_string()
        } else {
            "../".repeat(depth).trim_end_matches('/').to_string()
        };

        let processed_body = body
            .replace("{{ prefix }}", &prefix)
            .replace("{{prefix}}", &prefix);
        let (blocks, links) = parse_blocks(&processed_body);

        Ok(Content {
            frontmatter,
            body: processed_body,
            source_path: path.to_path_buf(),
            slug,
            output_path,
            blocks,
            links,
        })
    }
}

/// Extract TOML frontmatter block and body from raw content.
/// Frontmatter must be delimited by `+++` at start and end.
fn extract_frontmatter(raw: &str, path: &Path) -> ParseResult<(String, String)> {
    let trimmed = raw.trim_start();

    if !trimmed.starts_with("+++") {
        return Err(ParseError::Frontmatter {
            path: path.to_path_buf(),
            message: "missing frontmatter delimiter".to_string(),
        });
    }

    // Find the closing +++
    let after_first = &trimmed[3..].trim_start_matches(['\r', '\n']);
    let end_idx = after_first
        .find("\n+++")
        .ok_or_else(|| ParseError::Frontmatter {
            path: path.to_path_buf(),
            message: "missing closing frontmatter delimiter".to_string(),
        })?;

    let toml_block = after_first[..end_idx].to_string();
    let body = after_first[end_idx + 4..].trim_start().to_string();

    Ok((toml_block, body))
}

/// Parse TOML frontmatter into structured fields.
fn parse_frontmatter(path: &Path, toml_str: &str) -> ParseResult<Frontmatter> {
    toml::from_str(toml_str).map_err(|e| ParseError::Frontmatter {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

/// Deserialize a TOML native date into `Option<NaiveDate>`.
///
/// TOML native dates (`date = 2026-01-15`) are parsed by the `toml` crate as
/// `toml::value::Datetime`. This deserializer accepts that type, extracts the
/// date component, and constructs a validated `chrono::NaiveDate`.
fn deserialize_date<'de, D>(deserializer: D) -> std::result::Result<Option<NaiveDate>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let dt = toml::value::Datetime::deserialize(deserializer)?;
    let date = dt
        .date
        .ok_or_else(|| serde::de::Error::custom("datetime must include a date component"))?;
    NaiveDate::from_ymd_opt(date.year.into(), date.month.into(), date.day.into())
        .ok_or_else(|| serde::de::Error::custom("invalid calendar date"))
        .map(Some)
}

/// Build navigation items from already-parsed sections and pages.
///
/// Replaces the filesystem-based `discover_nav` — derives nav from typed
/// objects that were already parsed by `discover_sections`/`discover_pages`.
/// Result is sorted by weight (ascending), then label (alphabetic).
fn derive_nav(sections: &[Section], pages: &[Content]) -> Vec<NavItem> {
    let mut nav_items = Vec::new();

    // Sections → nav items with children
    for section in sections {
        let fm = &section.index.frontmatter;
        let children: Vec<NavItem> = section
            .items
            .iter()
            .filter(|item| !item.frontmatter.draft)
            .map(|item| NavItem {
                label: item
                    .frontmatter
                    .nav_label
                    .clone()
                    .unwrap_or_else(|| item.frontmatter.title.clone()),
                path: format!("/{}/{}.html", section.name, item.slug),
                weight: item.frontmatter.weight.unwrap_or(SortKey::DEFAULT_WEIGHT),
                children: Vec::new(),
            })
            .collect();

        nav_items.push(NavItem {
            label: fm.nav_label.clone().unwrap_or_else(|| fm.title.clone()),
            path: format!("/{}/index.html", section.name),
            weight: fm.weight.unwrap_or(SortKey::DEFAULT_WEIGHT),
            children,
        });
    }

    // Pages → leaf nav items
    for page in pages {
        if page.frontmatter.draft {
            continue;
        }
        nav_items.push(NavItem {
            label: page
                .frontmatter
                .nav_label
                .clone()
                .unwrap_or_else(|| page.frontmatter.title.clone()),
            path: format!("/{}.html", page.slug),
            weight: page.frontmatter.weight.unwrap_or(SortKey::DEFAULT_WEIGHT),
            children: Vec::new(),
        });
    }

    // Sort by weight, then alphabetically by label
    nav_items.sort_by(|a, b| a.weight.cmp(&b.weight).then_with(|| a.label.cmp(&b.label)));

    nav_items
}

/// A discovered section from the content directory.
#[derive(Debug)]
pub struct Section {
    /// The section's index content (_index.md)
    pub index: Content,
    /// Directory name (e.g., "blog", "projects")
    pub name: String,
    /// Section type for template dispatch (from frontmatter or directory name)
    pub section_type: SectionType,
    /// Section items, sorted at construction time by section type:
    /// - Blog: date descending
    /// - Projects: weight ascending (unweighted items sink to 99)
    /// - Custom: weight ascending, then title alphabetically
    pub items: Vec<Content>,
}

/// Discover all sections (directories with _index.md) in the content directory.
///
/// Section items are collected and sorted at construction time based on
/// section type. Callers access `section.items` directly.
fn walk_dirs(dir: &Path, content_dir: &Path, sections: &mut Vec<Section>) -> Result<()> {
    let index_path = dir.join(SECTION_INDEX);
    let has_index = index_path.exists();

    let mut md_files = Vec::new();
    let mut subdirs = Vec::new();

    let entries = fs::read_dir(dir).map_err(|e| ParseError::ReadFile {
        path: dir.to_path_buf(),
        source: e,
    })?;

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
        } else if path.is_file()
            && path.extension().is_some_and(|ext| ext == "md")
            && path
                .file_name()
                .is_some_and(|n| n != SECTION_INDEX && n != PAGE_404)
        {
            md_files.push(path);
        }
    }

    if dir != content_dir && (has_index || !md_files.is_empty()) {
        let relative_path = dir.strip_prefix(content_dir).unwrap_or(dir);
        let name = relative_path.to_string_lossy().to_string();

        let index = if has_index {
            Content::from_path(&index_path, ContentKind::Section, content_dir)?
        } else {
            let dir_name = dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("section")
                .to_string();
            let fm = Frontmatter {
                title: dir_name,
                description: None,
                date: None,
                tags: Vec::new(),
                weight: None,
                link_to: None,
                nav_label: None,
                section_type: None,
                template: None,
                toc: None,
                draft: false,
                aliases: Vec::new(),
            };
            Content {
                frontmatter: fm,
                body: String::new(),
                source_path: index_path.clone(),
                slug: SECTION_INDEX.replace(".md", ""),
                output_path: relative_path.join(OUTPUT_INDEX),
                blocks: Vec::new(),
                links: Vec::new(),
            }
        };

        let section_type = index.frontmatter.section_type.clone().unwrap_or_else(|| {
            let last_component = relative_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("section");
            SectionType::from_str(last_component)
        });

        let kind = match &section_type {
            SectionType::Blog => ContentKind::Post,
            SectionType::Projects => ContentKind::Project,
            _ => ContentKind::Page,
        };

        let mut items = Vec::new();
        for file_path in md_files {
            let content = Content::from_path(&file_path, kind.clone(), content_dir)?;
            if !content.frontmatter.draft {
                items.push(content);
            }
        }

        items.sort_by(|a, b| {
            let key_a = SortKey::for_content(&section_type, &a.frontmatter);
            let key_b = SortKey::for_content(&section_type, &b.frontmatter);
            key_a.cmp(&key_b)
        });

        sections.push(Section {
            index,
            name,
            section_type,
            items,
        });
    }

    for subdir in subdirs {
        walk_dirs(&subdir, content_dir, sections)?;
    }

    Ok(())
}

pub(crate) fn discover_sections(content_dir: &Path) -> Result<Vec<Section>> {
    let mut sections = Vec::new();
    walk_dirs(content_dir, content_dir, &mut sections)?;

    // Sort sections by weight
    sections.sort_by(|a, b| {
        let wa = a
            .index
            .frontmatter
            .weight
            .unwrap_or(SortKey::DEFAULT_WEIGHT);
        let wb = b
            .index
            .frontmatter
            .weight
            .unwrap_or(SortKey::DEFAULT_WEIGHT);
        wa.cmp(&wb)
    });

    Ok(sections)
}

/// Discover standalone pages (top-level .md files except _index.md and _404.md).
fn discover_pages(content_dir: &Path) -> Result<Vec<Content>> {
    let mut pages = Vec::new();

    let entries = fs::read_dir(content_dir).map_err(|e| ParseError::ReadFile {
        path: content_dir.to_path_buf(),
        source: e,
    })?;

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file()
            && path.extension().is_some_and(|ext| ext == "md")
            && path
                .file_name()
                .is_some_and(|n| n != SECTION_INDEX && n != PAGE_404)
        {
            let content = Content::from_path(&path, ContentKind::Page, content_dir)?;
            if !content.frontmatter.draft {
                pages.push(content);
            }
        }
    }

    Ok(pages)
}

/// Complete site content manifest from a single discovery pass.
///
/// Aggregates all content types for use by rendering, feed, and sitemap generation.
#[derive(Debug)]
pub struct SiteManifest {
    /// Homepage content (content/_index.md)
    pub homepage: Content,
    /// Custom 404 page (content/_404.md), if present
    pub page_404: Option<Content>,
    /// All sections (directories with _index.md)
    pub sections: Vec<Section>,
    /// Standalone pages (top-level .md files)
    pub pages: Vec<Content>,
    /// Blog posts for feed generation (items from "blog" sections)
    pub posts: Vec<Content>,
    /// Navigation menu items
    pub nav: Vec<NavItem>,
}

impl SiteManifest {
    /// Discover all site content in a single pass.
    pub fn discover(content_dir: impl AsRef<Path>) -> Result<Self> {
        Self::discover_inner(content_dir.as_ref())
    }

    fn discover_inner(content_dir: &Path) -> Result<Self> {
        // Load homepage
        let homepage_path = content_dir.join(SECTION_INDEX);
        let homepage = Content::from_path(&homepage_path, ContentKind::Section, content_dir)?;

        // Load 404 page if present
        let page_404_path = content_dir.join(PAGE_404);
        let page_404 = if page_404_path.exists() {
            Some(Content::from_path(
                &page_404_path,
                ContentKind::Page,
                content_dir,
            )?)
        } else {
            None
        };

        // Discover sections
        let sections = discover_sections(content_dir)?;

        // Collect blog posts from pre-sorted section items
        let mut posts = Vec::new();
        for section in &sections {
            if section.section_type == SectionType::Blog {
                posts.extend(section.items.iter().cloned());
            }
        }

        // Discover standalone pages
        let pages = discover_pages(content_dir)?;

        // Derive navigation from already-parsed sections and pages
        let nav = derive_nav(&sections, &pages);

        let manifest = SiteManifest {
            homepage,
            page_404,
            sections,
            pages,
            posts,
            nav,
        };

        // Validate internal links (non-fatal: print warnings)
        let broken = manifest.validate_internal_links();
        for err in &broken {
            eprintln!("warning: {}", err);
        }

        Ok(manifest)
    }

    /// Check all internal links against known output paths.
    ///
    /// Returns a list of `BrokenLink` errors for any internal link
    /// that doesn't resolve to a known page. This is non-fatal —
    /// callers decide whether to treat these as warnings or errors.
    pub fn validate_internal_links(&self) -> Vec<ParseError> {
        use std::collections::HashSet;

        // Build set of all known canonical output paths (with leading /)
        let mut known_paths: HashSet<String> = HashSet::new();

        // Homepage
        known_paths.insert(format!("/{}", self.homepage.output_path.display()));

        // 404
        if let Some(ref page) = self.page_404 {
            known_paths.insert(format!("/{}", page.output_path.display()));
        }

        // Sections (index + items)
        for section in &self.sections {
            known_paths.insert(format!("/{}", section.index.output_path.display()));
            for item in &section.items {
                known_paths.insert(format!("/{}", item.output_path.display()));
            }
        }

        // Standalone pages
        for page in &self.pages {
            known_paths.insert(format!("/{}", page.output_path.display()));
        }

        // Check all content's internal links
        let mut broken = Vec::new();

        let all_content: Vec<&Content> = std::iter::once(&self.homepage)
            .chain(self.page_404.iter())
            .chain(
                self.sections
                    .iter()
                    .flat_map(|s| std::iter::once(&s.index).chain(s.items.iter())),
            )
            .chain(self.pages.iter())
            .collect();

        for content in all_content {
            for link in &content.links {
                if !link.is_internal {
                    continue;
                }
                let normalized = normalize_link_url(&link.url);
                if normalized.is_empty() {
                    continue;
                }
                if !known_paths.contains(&normalized) {
                    broken.push(ParseError::BrokenLink {
                        source_page: content.source_path.clone(),
                        target: link.url.clone(),
                    });
                }
            }
        }

        broken
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn create_test_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("failed to create temp dir")
    }

    fn write_frontmatter(path: &Path, title: &str, weight: Option<i64>, nav_label: Option<&str>) {
        let mut content = format!("+++\ntitle = \"{}\"\n", title);
        if let Some(w) = weight {
            content.push_str(&format!("weight = {}\n", w));
        }
        if let Some(label) = nav_label {
            content.push_str(&format!("nav_label = \"{}\"\n", label));
        }
        content.push_str("+++\n\nBody content.");
        fs::write(path, content).expect("failed to write test file");
    }

    // =========================================================================
    // derive_nav tests
    // =========================================================================

    #[test]
    fn test_derive_nav_builds_from_sections_and_pages() {
        let dir = create_test_dir();
        let content_dir = dir.path();

        // Set up a section with children and standalone pages
        let blog_dir = content_dir.join("blog");
        fs::create_dir(&blog_dir).unwrap();
        write_section_index(&blog_dir.join("_index.md"), "Blog", None, Some(20));

        // Use TOML+++ frontmatter since sukr's parser requires it
        fs::write(
            blog_dir.join("post1.md"),
            "+++\ntitle = \"Post 1\"\nweight = 10\n+++\n",
        )
        .unwrap();
        fs::write(
            blog_dir.join("post2.md"),
            "+++\ntitle = \"Post 2\"\nweight = 5\n+++\n",
        )
        .unwrap();

        write_frontmatter(&content_dir.join("about.md"), "About", Some(10), None);

        let sections = discover_sections(content_dir).unwrap();
        let pages = discover_pages(content_dir).unwrap();
        let nav = derive_nav(&sections, &pages);

        // Pages (weight 10) come before sections (weight 20)
        assert_eq!(nav.len(), 2);
        assert_eq!(nav[0].label, "About");
        assert_eq!(nav[0].path, "/about.html");
        assert_eq!(nav[1].label, "Blog");
        assert_eq!(nav[1].path, "/blog/index.html");

        // Section children sorted by weight
        assert_eq!(nav[1].children.len(), 2);
        assert_eq!(nav[1].children[0].label, "Post 2"); // weight 5
        assert_eq!(nav[1].children[1].label, "Post 1"); // weight 10
    }

    // =========================================================================
    // discover_sections tests
    // =========================================================================

    fn write_section_index(
        path: &Path,
        title: &str,
        section_type: Option<&str>,
        weight: Option<i64>,
    ) {
        let mut content = format!("+++\ntitle = \"{}\"\n", title);
        if let Some(st) = section_type {
            content.push_str(&format!("section_type = \"{}\"\n", st));
        }
        if let Some(w) = weight {
            content.push_str(&format!("weight = {}\n", w));
        }
        content.push_str("+++\nSection content.\n");
        fs::write(path, content).expect("failed to write section index");
    }

    #[test]
    fn test_discover_sections_finds_directories() {
        let dir = create_test_dir();
        let content_dir = dir.path();

        // Create two sections
        fs::create_dir(content_dir.join("blog")).unwrap();
        write_section_index(&content_dir.join("blog/_index.md"), "Blog", None, None);

        fs::create_dir(content_dir.join("projects")).unwrap();
        write_section_index(
            &content_dir.join("projects/_index.md"),
            "Projects",
            None,
            None,
        );

        let sections = discover_sections(content_dir).expect("discover_sections failed");
        assert_eq!(sections.len(), 2);

        let names: Vec<_> = sections.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"blog"));
        assert!(names.contains(&"projects"));
    }

    #[test]
    fn test_discover_sections_uses_section_type_from_frontmatter() {
        let dir = create_test_dir();
        let content_dir = dir.path();

        fs::create_dir(content_dir.join("writings")).unwrap();
        write_section_index(
            &content_dir.join("writings/_index.md"),
            "My Writings",
            Some("blog"),
            None,
        );

        let sections = discover_sections(content_dir).expect("discover_sections failed");
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].name, "writings");
        assert_eq!(sections[0].section_type, SectionType::Blog); // From frontmatter, not dir name
    }

    #[test]
    fn test_discover_sections_falls_back_to_dir_name() {
        let dir = create_test_dir();
        let content_dir = dir.path();

        fs::create_dir(content_dir.join("gallery")).unwrap();
        write_section_index(
            &content_dir.join("gallery/_index.md"),
            "Gallery",
            None,
            None,
        );

        let sections = discover_sections(content_dir).expect("discover_sections failed");
        assert_eq!(sections.len(), 1);
        assert_eq!(
            sections[0].section_type,
            SectionType::Custom("gallery".to_string())
        ); // Falls back to dir name
    }

    #[test]
    fn test_discover_sections_sorts_by_weight() {
        let dir = create_test_dir();
        let content_dir = dir.path();

        fs::create_dir(content_dir.join("blog")).unwrap();
        write_section_index(&content_dir.join("blog/_index.md"), "Blog", None, Some(20));

        fs::create_dir(content_dir.join("projects")).unwrap();
        write_section_index(
            &content_dir.join("projects/_index.md"),
            "Projects",
            None,
            Some(10),
        );

        let sections = discover_sections(content_dir).expect("discover_sections failed");
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].name, "projects"); // weight 10
        assert_eq!(sections[1].name, "blog"); // weight 20
    }

    #[test]
    fn test_section_collect_items() {
        let dir = create_test_dir();
        let content_dir = dir.path();

        fs::create_dir(content_dir.join("blog")).unwrap();
        write_section_index(
            &content_dir.join("blog/_index.md"),
            "Blog",
            Some("blog"),
            None,
        );
        write_frontmatter(&content_dir.join("blog/post1.md"), "Post 1", None, None);
        write_frontmatter(&content_dir.join("blog/post2.md"), "Post 2", None, None);

        let sections = discover_sections(content_dir).expect("discover_sections failed");
        assert_eq!(sections.len(), 1);

        let items = &sections[0].items;
        assert_eq!(items.len(), 2);

        let titles: Vec<_> = items.iter().map(|c| c.frontmatter.title.as_str()).collect();
        assert!(titles.contains(&"Post 1"));
        assert!(titles.contains(&"Post 2"));
    }

    // =========================================================================
    // SiteManifest tests
    // =========================================================================

    #[test]
    fn test_manifest_discovers_homepage() {
        let dir = create_test_dir();
        let content_dir = dir.path();

        write_frontmatter(&content_dir.join("_index.md"), "Home", None, None);

        let manifest = SiteManifest::discover(content_dir).expect("discover failed");
        assert_eq!(manifest.homepage.frontmatter.title, "Home");
    }

    #[test]
    fn test_manifest_discovers_sections() {
        let dir = create_test_dir();
        let content_dir = dir.path();

        write_frontmatter(&content_dir.join("_index.md"), "Home", None, None);
        fs::create_dir(content_dir.join("blog")).unwrap();
        write_section_index(
            &content_dir.join("blog/_index.md"),
            "Blog",
            Some("blog"),
            None,
        );

        let manifest = SiteManifest::discover(content_dir).expect("discover failed");
        assert_eq!(manifest.sections.len(), 1);
        assert_eq!(manifest.sections[0].name, "blog");
    }

    #[test]
    fn test_manifest_discovers_pages() {
        let dir = create_test_dir();
        let content_dir = dir.path();

        write_frontmatter(&content_dir.join("_index.md"), "Home", None, None);
        write_frontmatter(&content_dir.join("about.md"), "About", None, None);
        write_frontmatter(&content_dir.join("contact.md"), "Contact", None, None);

        let manifest = SiteManifest::discover(content_dir).expect("discover failed");
        assert_eq!(manifest.pages.len(), 2);

        let titles: Vec<_> = manifest
            .pages
            .iter()
            .map(|c| c.frontmatter.title.as_str())
            .collect();
        assert!(titles.contains(&"About"));
        assert!(titles.contains(&"Contact"));
    }

    #[test]
    fn test_manifest_collects_blog_posts() {
        let dir = create_test_dir();
        let content_dir = dir.path();

        write_frontmatter(&content_dir.join("_index.md"), "Home", None, None);
        fs::create_dir(content_dir.join("blog")).unwrap();
        write_section_index(
            &content_dir.join("blog/_index.md"),
            "Blog",
            Some("blog"),
            None,
        );

        // Create blog posts with dates
        let post1 = "+++\ntitle = \"Post 1\"\ndate = 2026-01-15\n+++\nContent.".to_string();
        let post2 = "+++\ntitle = \"Post 2\"\ndate = 2026-01-20\n+++\nContent.".to_string();
        fs::write(content_dir.join("blog/post1.md"), &post1).unwrap();
        fs::write(content_dir.join("blog/post2.md"), &post2).unwrap();

        let manifest = SiteManifest::discover(content_dir).expect("discover failed");
        assert_eq!(manifest.posts.len(), 2);

        // Should be sorted by date, newest first
        assert_eq!(manifest.posts[0].frontmatter.title, "Post 2"); // 2026-01-20
        assert_eq!(manifest.posts[1].frontmatter.title, "Post 1"); // 2026-01-15
    }

    #[test]
    fn test_manifest_discovers_nav() {
        let dir = create_test_dir();
        let content_dir = dir.path();

        write_frontmatter(&content_dir.join("_index.md"), "Home", None, None);
        write_frontmatter(&content_dir.join("about.md"), "About", Some(10), None);
        fs::create_dir(content_dir.join("blog")).unwrap();
        write_section_index(&content_dir.join("blog/_index.md"), "Blog", None, Some(20));

        let manifest = SiteManifest::discover(content_dir).expect("discover failed");
        assert_eq!(manifest.nav.len(), 2);

        // Nav should be sorted by weight
        assert_eq!(manifest.nav[0].label, "About"); // weight 10
        assert_eq!(manifest.nav[1].label, "Blog"); // weight 20
    }

    fn write_draft(path: &Path, title: &str) {
        let content = format!(
            "+++\ntitle = \"{}\"\ndraft = true\n+++\n\nDraft content.",
            title
        );
        fs::write(path, content).expect("failed to write draft file");
    }

    #[test]
    fn test_collect_items_excludes_drafts() {
        let dir = create_test_dir();
        let section_dir = dir.path().join("features");
        fs::create_dir(&section_dir).unwrap();
        write_frontmatter(&section_dir.join("_index.md"), "Features", None, None);
        write_frontmatter(&section_dir.join("visible.md"), "Visible", None, None);
        write_draft(&section_dir.join("hidden.md"), "Hidden");

        let sections = discover_sections(dir.path()).unwrap();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].items.len(), 1);
        assert_eq!(sections[0].items[0].frontmatter.title, "Visible");
    }

    #[test]
    fn test_discover_pages_excludes_drafts() {
        let dir = create_test_dir();
        let content_dir = dir.path();

        write_frontmatter(&content_dir.join("about.md"), "About", None, None);
        write_draft(&content_dir.join("wip.md"), "Work in Progress");

        let pages = discover_pages(content_dir).unwrap();
        assert_eq!(pages.len(), 1, "draft page should not appear in pages");
        assert_eq!(pages[0].frontmatter.title, "About");
    }

    #[test]
    fn test_discover_pages_excludes_404() {
        let dir = create_test_dir();
        let content_dir = dir.path();

        write_frontmatter(&content_dir.join("about.md"), "About", None, None);
        fs::write(
            content_dir.join("_404.md"),
            "+++\ntitle = \"Not Found\"\n+++\n\n404 body.",
        )
        .unwrap();

        let pages = discover_pages(content_dir).unwrap();
        assert_eq!(pages.len(), 1, "_404.md should not appear in pages");
        assert_eq!(pages[0].frontmatter.title, "About");
    }

    #[test]
    fn test_manifest_detects_404() {
        let dir = create_test_dir();
        let content_dir = dir.path();

        write_frontmatter(&content_dir.join("_index.md"), "Home", None, None);
        fs::write(
            content_dir.join("_404.md"),
            "+++\ntitle = \"Page Not Found\"\n+++\n\n404 body.",
        )
        .unwrap();

        let manifest = SiteManifest::discover(content_dir).unwrap();
        assert!(manifest.page_404.is_some(), "page_404 should be populated");
        assert_eq!(
            manifest.page_404.unwrap().frontmatter.title,
            "Page Not Found"
        );
    }

    #[test]
    fn test_manifest_without_404() {
        let dir = create_test_dir();
        let content_dir = dir.path();

        write_frontmatter(&content_dir.join("_index.md"), "Home", None, None);

        let manifest = SiteManifest::discover(content_dir).unwrap();
        assert!(
            manifest.page_404.is_none(),
            "page_404 should be None when _404.md absent"
        );
    }

    // ========================================================================
    // Category C type tests
    // ========================================================================

    #[test]
    fn test_content_block_construction() {
        let code = ContentBlock::Code {
            language: Some("rust".to_string()),
            source: "fn main() {}".to_string(),
        };
        assert_eq!(
            code,
            ContentBlock::Code {
                language: Some("rust".to_string()),
                source: "fn main() {}".to_string(),
            }
        );

        let prose = ContentBlock::Prose("hello".to_string());
        assert_eq!(prose, ContentBlock::Prose("hello".to_string()));

        let prose2 = ContentBlock::Prose("<div>html</div>".to_string());
        assert_ne!(prose2, prose);
    }

    #[test]
    fn test_tag_ordering() {
        let a = Tag::new("alpha");
        let b = Tag::new("beta");
        let a2 = Tag::new("alpha");

        assert!(a < b);
        assert_eq!(a, a2);
        assert!(b > a);
    }

    #[test]
    fn test_tag_display_and_as_ref() {
        let tag = Tag::new("rust");
        assert_eq!(tag.to_string(), "rust");
        assert_eq!(tag.as_ref(), "rust");
        assert_eq!(tag.as_str(), "rust");
    }

    #[test]
    fn test_tag_serde_round_trip() {
        let tag = Tag::new("testing");
        // Tag serializes as a plain string
        assert_eq!(tag.as_str(), "testing");

        // Demonstrate round-trip through TOML (the actual format)
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Wrapper {
            tag: Tag,
        }
        let w = Wrapper {
            tag: Tag::new("round-trip"),
        };
        let toml_str = toml::to_string(&w).unwrap();
        let back: Wrapper = toml::from_str(&toml_str).unwrap();
        assert_eq!(back, w);
    }

    #[test]
    fn test_tag_hash_dedup() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(Tag::new("rust"));
        set.insert(Tag::new("rust"));
        set.insert(Tag::new("go"));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_section_type_from_str() {
        assert_eq!(SectionType::from_str("blog"), SectionType::Blog);
        assert_eq!(SectionType::from_str("projects"), SectionType::Projects);
        assert_eq!(
            SectionType::from_str("gallery"),
            SectionType::Custom("gallery".to_string())
        );
    }

    #[test]
    fn test_section_type_display_round_trip() {
        let cases = [SectionType::Blog, SectionType::Projects];
        for st in &cases {
            let s = st.to_string();
            assert_eq!(SectionType::from_str(&s), *st);
        }

        let custom = SectionType::Custom("wiki".to_string());
        assert_eq!(custom.to_string(), "wiki");
        assert_eq!(SectionType::from_str("wiki"), custom);
    }

    #[test]
    fn test_section_type_serde() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Wrapper {
            kind: SectionType,
        }
        let w = Wrapper {
            kind: SectionType::Blog,
        };
        let toml_str = toml::to_string(&w).unwrap();
        assert!(toml_str.contains("blog"));

        let back: Wrapper = toml::from_str(&toml_str).unwrap();
        assert_eq!(back.kind, SectionType::Blog);
    }

    #[test]
    fn test_sort_key_date_desc_newest_first() {
        let newer = SortKey::DateDesc(NaiveDate::from_ymd_opt(2026, 2, 21).unwrap());
        let older = SortKey::DateDesc(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());

        // Newer should sort BEFORE older (reverse chronological)
        assert!(newer < older);
    }

    #[test]
    fn test_sort_key_weight_title_ordering() {
        let light = SortKey::WeightTitle(10, "Zebra".to_string());
        let heavy = SortKey::WeightTitle(99, "Alpha".to_string());
        let same_weight_a = SortKey::WeightTitle(50, "Alpha".to_string());
        let same_weight_z = SortKey::WeightTitle(50, "Zebra".to_string());

        // Lower weight sorts first
        assert!(light < heavy);
        // Same weight: alphabetical by title
        assert!(same_weight_a < same_weight_z);
    }

    #[test]
    fn test_sort_key_cross_variant() {
        let dated = SortKey::DateDesc(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
        let weighted = SortKey::WeightTitle(1, "First".to_string());

        // DateDesc sorts before WeightTitle
        assert!(dated < weighted);
    }

    #[test]
    fn test_sort_key_for_content_blog() {
        let fm = Frontmatter {
            title: "Test Post".to_string(),
            date: Some(NaiveDate::from_ymd_opt(2026, 2, 21).unwrap()),
            weight: None,
            description: None,
            tags: vec![],
            link_to: None,
            nav_label: None,
            section_type: None,
            template: None,
            toc: None,
            draft: false,
            aliases: vec![],
        };

        let key = SortKey::for_content(&SectionType::Blog, &fm);
        assert_eq!(
            key,
            SortKey::DateDesc(NaiveDate::from_ymd_opt(2026, 2, 21).unwrap())
        );
    }

    #[test]
    fn test_sort_key_for_content_blog_undated_fallback() {
        let fm = Frontmatter {
            title: "Undated Post".to_string(),
            date: None,
            weight: Some(10),
            description: None,
            tags: vec![],
            link_to: None,
            nav_label: None,
            section_type: None,
            template: None,
            toc: None,
            draft: false,
            aliases: vec![],
        };

        let key = SortKey::for_content(&SectionType::Blog, &fm);
        assert_eq!(key, SortKey::WeightTitle(10, "Undated Post".to_string()));
    }

    #[test]
    fn test_sort_key_for_content_projects() {
        let fm = Frontmatter {
            title: "My Project".to_string(),
            date: Some(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()),
            weight: None,
            description: None,
            tags: vec![],
            link_to: None,
            nav_label: None,
            section_type: None,
            template: None,
            toc: None,
            draft: false,
            aliases: vec![],
        };

        // Projects always use WeightTitle, even if dated
        let key = SortKey::for_content(&SectionType::Projects, &fm);
        assert_eq!(key, SortKey::WeightTitle(50, "My Project".to_string()));
    }

    #[test]
    fn test_nav_item_ordering() {
        let first = NavItem {
            label: "About".to_string(),
            path: "/about.html".to_string(),
            weight: 10,
            children: vec![],
        };
        let second = NavItem {
            label: "Blog".to_string(),
            path: "/blog/index.html".to_string(),
            weight: 20,
            children: vec![],
        };
        let same_weight = NavItem {
            label: "Contact".to_string(),
            path: "/contact.html".to_string(),
            weight: 10,
            children: vec![],
        };

        assert!(first < second);
        // Same weight: alphabetical
        assert!(first < same_weight);
    }

    #[test]
    fn test_link_target_construction() {
        let internal = LinkTarget {
            url: "../other-page.md".to_string(),
            is_internal: true,
        };
        let external = LinkTarget {
            url: "https://example.com".to_string(),
            is_internal: false,
        };

        assert_ne!(internal, external);
        assert!(internal.is_internal);
        assert!(!external.is_internal);
    }

    // =========================================================================
    // parse_blocks tests
    // =========================================================================

    #[test]
    fn test_parse_blocks_code_block() {
        let md = "```rust\nfn main() {}\n```\n";
        let (blocks, _links) = parse_blocks(md);
        assert!(
            blocks
                .iter()
                .any(|b| matches!(b, ContentBlock::Code { language: Some(lang), source } if lang == "rust" && source.contains("fn main()"))),
            "expected Code block with language=rust, got: {:?}",
            blocks
        );
    }

    #[test]
    fn test_parse_blocks_mermaid_diagram() {
        let md = "```mermaid\ngraph TD\n  A --> B\n```\n";
        let (blocks, _links) = parse_blocks(md);
        assert!(
            blocks.iter().any(
                |b| matches!(b, ContentBlock::Diagram { source } if source.contains("graph TD"))
            ),
            "expected Diagram block, got: {:?}",
            blocks
        );
    }

    #[test]
    fn test_parse_blocks_heading() {
        let md = "## Hello World\n";
        let (blocks, _links) = parse_blocks(md);
        assert!(
            blocks
                .iter()
                .any(|b| matches!(b, ContentBlock::Heading { level: 2, text, id } if text == "Hello World" && id == "hello-world")),
            "expected Heading level=2, got: {:?}",
            blocks
        );
    }

    #[test]
    fn test_parse_blocks_link() {
        let md = "[click here](https://example.com \"A title\")\n";
        let (blocks, links) = parse_blocks(md);
        // Link renders to Prose HTML
        let prose = blocks.iter().any(|b| match b {
            ContentBlock::Prose(html) => html.contains("<a href=") && html.contains("click here"),
            _ => false,
        });
        assert!(prose, "expected Prose with <a> tag, got: {:?}", blocks);
        // Link extracted as side-channel
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].url, "https://example.com");
        assert!(!links[0].is_internal);
    }

    #[test]
    fn test_parse_blocks_image() {
        let md = "![alt text](/img/photo.png)\n";
        let (blocks, links) = parse_blocks(md);
        // Image renders to Prose HTML
        let prose = blocks.iter().any(|b| match b {
            ContentBlock::Prose(html) => html.contains("<img src=") && html.contains("alt text"),
            _ => false,
        });
        assert!(prose, "expected Prose with <img> tag, got: {:?}", blocks);
        // Image URL extracted as side-channel
        assert_eq!(links.len(), 1);
        assert!(links[0].is_internal);
        assert_eq!(links[0].url, "/img/photo.png");
    }

    #[test]
    fn test_parse_blocks_inline_math() {
        let md = "The formula $E = mc^2$ is famous.\n";
        let (blocks, _links) = parse_blocks(md);
        assert!(
            blocks.iter().any(
                |b| matches!(b, ContentBlock::Math { source, display: false } if source == "E = mc^2")
            ),
            "expected inline Math block, got: {:?}",
            blocks
        );
    }

    #[test]
    fn test_parse_blocks_display_math() {
        let md = "$$\n\\int_0^1 x^2 dx\n$$\n";
        let (blocks, _links) = parse_blocks(md);
        assert!(
            blocks
                .iter()
                .any(|b| matches!(b, ContentBlock::Math { display: true, .. })),
            "expected display Math block, got: {:?}",
            blocks
        );
    }

    #[test]
    fn test_parse_blocks_text() {
        let md = "Hello world\n";
        let (blocks, _links) = parse_blocks(md);
        // Plain text now renders as Prose with html_escape applied
        let prose = blocks.iter().any(|b| match b {
            ContentBlock::Prose(html) => html.contains("Hello world"),
            _ => false,
        });
        assert!(prose, "expected Prose block with text, got: {:?}", blocks);
    }

    // =========================================================================
    // Link extraction (side-channel) tests
    // =========================================================================

    #[test]
    fn test_parse_blocks_extracts_internal_and_external_links() {
        let md = "[About](/about.html) and [Example](https://example.com)\n";
        let (_blocks, links) = parse_blocks(md);
        assert_eq!(links.len(), 2);
        assert!(links[0].is_internal);
        assert!(!links[1].is_internal);
    }

    #[test]
    fn test_parse_blocks_skips_fragment_links() {
        let md = "[jump](#section)\n";
        let (_blocks, links) = parse_blocks(md);
        assert!(links.is_empty(), "fragment-only links should be skipped");
    }

    #[test]
    fn test_parse_blocks_excludes_mailto_and_data_from_internal() {
        let md = "[email](mailto:user@example.com) and [data](data:text/plain;base64,SGVsbG8=)\n";
        let (_blocks, links) = parse_blocks(md);
        assert_eq!(links.len(), 2);
        assert!(!links[0].is_internal, "mailto should not be internal");
        assert!(!links[1].is_internal, "data: should not be internal");
    }

    #[test]
    fn test_parse_blocks_extracts_image_links() {
        let md = "![photo](/img/photo.png)\n";
        let (_blocks, links) = parse_blocks(md);
        assert_eq!(links.len(), 1);
        assert!(links[0].is_internal);
        assert_eq!(links[0].url, "/img/photo.png");
    }

    // =========================================================================
    // validate_internal_links tests
    // =========================================================================

    #[test]
    fn test_normalize_link_url() {
        // Absolute with extension
        assert_eq!(normalize_link_url("/about.html"), "/about.html");
        // Absolute without extension → adds .html
        assert_eq!(normalize_link_url("/about"), "/about.html");
        // Relative → adds leading / and .html
        assert_eq!(normalize_link_url("about"), "/about.html");
        // With fragment → strips fragment
        assert_eq!(normalize_link_url("/about#section"), "/about.html");
        // Already correct
        assert_eq!(normalize_link_url("/blog/post.html"), "/blog/post.html");
        // Empty after fragment strip
        assert_eq!(normalize_link_url("#section"), "");
    }

    #[test]
    fn test_validate_internal_links_valid() {
        let dir = create_test_dir();
        let content_dir = dir.path();

        // Set up homepage and a page that links to another page
        write_frontmatter(&content_dir.join("_index.md"), "Home", None, None);
        fs::write(
            content_dir.join("about.md"),
            "+++\ntitle = \"About\"\n+++\n\n[Home](/index.html)\n",
        )
        .unwrap();

        let manifest = SiteManifest::discover(content_dir).unwrap();
        let broken = manifest.validate_internal_links();
        assert!(
            broken.is_empty(),
            "expected no broken links, got: {:?}",
            broken
        );
    }

    #[test]
    fn test_validate_internal_links_broken() {
        let dir = create_test_dir();
        let content_dir = dir.path();

        // Set up homepage and a page with a broken link
        write_frontmatter(&content_dir.join("_index.md"), "Home", None, None);
        fs::write(
            content_dir.join("about.md"),
            "+++\ntitle = \"About\"\n+++\n\n[Missing](/nonexistent)\n",
        )
        .unwrap();

        let manifest = SiteManifest::discover(content_dir).unwrap();
        let broken = manifest.validate_internal_links();
        assert_eq!(broken.len(), 1, "expected 1 broken link, got: {:?}", broken);
        match &broken[0] {
            ParseError::BrokenLink { target, .. } => {
                assert_eq!(target, "/nonexistent");
            },
            other => panic!("expected BrokenLink, got: {:?}", other),
        }
    }

    #[test]
    fn test_validate_internal_links_ignores_external() {
        let dir = create_test_dir();
        let content_dir = dir.path();

        // External link should not be validated
        write_frontmatter(&content_dir.join("_index.md"), "Home", None, None);
        fs::write(
            content_dir.join("about.md"),
            "+++\ntitle = \"About\"\n+++\n\n[External](https://example.com/missing)\n",
        )
        .unwrap();

        let manifest = SiteManifest::discover(content_dir).unwrap();
        let broken = manifest.validate_internal_links();
        assert!(
            broken.is_empty(),
            "external links should not be validated, got: {:?}",
            broken
        );
    }
}
