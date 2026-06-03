//! Content block rendering — the Render catamorphism.
//!
//! Dispatches over `ContentBlock` variants: Code → tree-sitter highlighting,
//! Math → MathML, Diagram → Mermaid SVG, Heading → slug/anchor/pilcrow,
//! Prose → identity passthrough.

use serde::Serialize;

use crate::escape::{code_escape, html_escape};
use crate::highlight::{Language, highlight_code};

/// A heading anchor extracted from markdown content.
#[derive(Debug, Clone, Serialize)]
pub struct Anchor {
    /// Heading ID attribute (slug)
    pub id: String,
    /// Heading text content
    pub label: String,
    /// Heading level (2-6, h1 excluded)
    pub level: u8,
}

/// Render content blocks to HTML with syntax highlighting.
///
/// This is the Render catamorphism from the formal model: it dispatches over
/// the `ContentBlock` coproduct, applying build-time interception to the four
/// intercepted variants and passing Prose through as identity.
///
/// Returns the HTML output and a list of extracted heading anchors, or an
/// error if math rendering fails.
pub fn render_blocks(
    blocks: &[crate::content::ContentBlock],
    render_prose: impl Fn(&str) -> String,
) -> Result<(String, Vec<Anchor>), String> {
    use crate::content::ContentBlock;

    let mut html_output = String::new();
    let mut anchors = Vec::new();

    for block in blocks {
        match block {
            // --- Intercepted: Code (tree-sitter highlighting) ---
            ContentBlock::Code { language, source } => {
                html_output.push_str("<pre><code");

                let lang_str = language.as_deref().unwrap_or("");
                if let Some(lang) = Language::from_fence(lang_str) {
                    html_output.push_str(&format!(" class=\"language-{}\">", lang_str));
                    html_output.push_str(&highlight_code(lang, source));
                } else {
                    if !lang_str.is_empty() {
                        html_output.push_str(&format!(" class=\"language-{}\">", lang_str));
                    } else {
                        html_output.push('>');
                    }
                    html_output.push_str(&code_escape(source));
                }

                html_output.push_str("</code></pre>\n");
            },

            // --- Intercepted: Diagram (Mermaid → SVG) ---
            ContentBlock::Diagram { source } => match crate::mermaid::render_diagram(source) {
                Ok(svg) => {
                    html_output.push_str("<div class=\"mermaid-diagram\">\n");
                    html_output.push_str(&svg);
                    html_output.push_str("\n</div>\n");
                },
                Err(e) => {
                    eprintln!("mermaid render error: {e}");
                    html_output.push_str("<pre class=\"mermaid-error\"><code>");
                    html_output.push_str(&html_escape(source));
                    html_output.push_str("</code></pre>\n");
                },
            },

            // --- Intercepted: Math (LaTeX → MathML) ---
            ContentBlock::Math { source, display } => {
                let rendered = crate::math::render_math(source, *display)
                    .map_err(|e| format!("math render error in `{source}`: {e}"))?;
                html_output.push_str(&rendered);
                if *display {
                    html_output.push('\n');
                }
            },

            // --- Intercepted: Heading (slug + anchor + pilcrow) ---
            ContentBlock::Heading { level, text, id } => {
                html_output.push_str(&format!("<h{} id=\"{}\">", level, id));
                html_output.push_str(&html_escape(text));
                html_output.push_str(&format!(
                    "<a class=\"heading-anchor\" href=\"#{}\">¶</a></h{}>\n",
                    id, level
                ));

                // Extract anchor for h2-h6 (skip h1)
                if *level >= 2 {
                    anchors.push(Anchor {
                        id: id.clone(),
                        label: text.clone(),
                        level: *level,
                    });
                }
            },

            // --- Identity: Prose (passthrough) ---
            ContentBlock::Prose(html) => {
                html_output.push_str(&render_prose(html));
            },
        }
    }

    Ok((html_output, anchors))
}

/// Convert heading text to a URL-friendly slug ID.
pub(crate) fn slugify(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::parse_blocks;

    /// Helper: parse markdown and render via the full pipeline.
    fn render_md(md: &str) -> (String, Vec<Anchor>) {
        let (blocks, _links) = parse_blocks(md);
        render_blocks(&blocks, |s| s.to_string()).unwrap()
    }

    #[test]
    fn test_basic_markdown() {
        let md = "# Hello\n\nThis is a *test*.";
        let (html, _) = render_md(md);
        assert!(html.contains(
            "<h1 id=\"hello\">Hello<a class=\"heading-anchor\" href=\"#hello\">¶</a></h1>"
        ));
        assert!(html.contains("<em>test</em>"));
    }

    #[test]
    fn test_code_block_highlighting() {
        let md = "```rust\nfn main() {}\n```";
        let (html, _) = render_md(md);
        assert!(html.contains("<pre><code"));
        assert!(html.contains("language-rust"));
        assert!(html.contains("class=\"hl-"));
    }

    #[test]
    fn test_code_block_unknown_language() {
        let md = "```unknown\nsome code\n```";
        let (html, _) = render_md(md);
        assert!(html.contains("<pre><code"));
        assert!(html.contains("language-unknown"));
        assert!(html.contains("some code"));
        assert!(!html.contains("class=\"hl-"));
    }

    #[test]
    fn test_inline_code() {
        let md = "Use `cargo run` to start.";
        let (html, _) = render_md(md);
        assert!(html.contains("<code>cargo run</code>"));
    }

    #[test]
    fn test_image_alt_text() {
        let md = "![Beautiful sunset](sunset.jpg \"Evening sky\")";
        let (html, _) = render_md(md);
        assert!(html.contains("alt=\"Beautiful sunset\""));
        assert!(html.contains("title=\"Evening sky\""));
        assert!(html.contains("src=\"sunset.jpg\""));
    }

    #[test]
    fn test_image_alt_text_no_title() {
        let md = "![Logo image](logo.png)";
        let (html, _) = render_md(md);
        assert!(html.contains("alt=\"Logo image\""));
        assert!(html.contains("src=\"logo.png\""));
        assert!(!html.contains("title="));
    }

    #[test]
    fn test_anchor_extraction() {
        let md = r#"# Page Title
## Getting Started
Some intro text.
### Installation
Install steps.
## Configuration
Config details.
#### Deep Heading
"#;
        let (html, anchors) = render_md(md);

        // h1 should NOT be extracted (page title, not TOC)
        assert!(anchors.iter().all(|a| a.level >= 2));

        // Should have 4 anchors: h2, h3, h2, h4
        assert_eq!(anchors.len(), 4);

        assert_eq!(anchors[0].id, "getting-started");
        assert_eq!(anchors[0].label, "Getting Started");
        assert_eq!(anchors[0].level, 2);

        assert_eq!(anchors[1].id, "installation");
        assert_eq!(anchors[1].level, 3);

        assert_eq!(anchors[2].id, "configuration");
        assert_eq!(anchors[2].level, 2);

        assert_eq!(anchors[3].id, "deep-heading");
        assert_eq!(anchors[3].level, 4);

        assert!(html.contains("id=\"getting-started\""));
        assert!(html.contains("id=\"installation\""));
    }

    #[test]
    fn test_slugify_edge_cases() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("Hello   World"), "hello-world");
        assert_eq!(slugify("What's New?"), "what-s-new");
        assert_eq!(slugify("Version 2.0"), "version-2-0");
        assert_eq!(slugify("  Padded  "), "padded");
        assert_eq!(slugify("CamelCase"), "camelcase");
        assert_eq!(slugify("A -- B"), "a-b");
    }

    #[test]
    fn test_link_url_escaping() {
        let md = r#"[click]("><script>alert(1)</script>)"#;
        let (html, _) = render_md(md);
        assert!(!html.contains("<script>"), "script tags should be escaped");
        assert!(html.contains("&gt;"), "angle brackets should be escaped");

        let md = r#"[click](javascript:alert(1))"#;
        let (html, _) = render_md(md);
        assert!(html.contains("href=\"javascript:alert(1)\""));
    }

    #[test]
    fn test_link_title_escaping() {
        let md = r#"[text](url "title with \"quotes\"")"#;
        let (html, _) = render_md(md);
        assert!(html.contains("&quot;"), "quotes in title should be escaped");
    }

    #[test]
    fn test_image_src_escaping() {
        let md = r#"![alt]("><script>alert(1)</script>)"#;
        let (html, _) = render_md(md);
        assert!(!html.contains("<script>"), "script tags should be escaped");
        assert!(
            html.contains("&quot;") || html.contains("&gt;"),
            "special chars in src should be escaped"
        );
    }

    #[test]
    fn test_unlabeled_code_block_preserves_quotes() {
        let md = "```\nContent-Security-Policy: default-src 'self';\n```";
        let (html, _) = render_md(md);
        assert!(html.contains("<pre><code>"), "should have code block");
        assert!(
            html.contains("'self'"),
            "single quotes should be preserved in code blocks"
        );
        assert!(
            !html.contains("&#39;"),
            "quotes should not be HTML-escaped in code blocks"
        );
    }
}
