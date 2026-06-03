//! Tera-based template engine for runtime HTML generation.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;
use tera::{Context, Tera, Value};

use crate::config::SiteConfig;
use crate::content::{Content, NavItem};
use crate::error::{CompileError, CompileResult};
use crate::render::Anchor;

/// Default template for standalone pages.
const TEMPLATE_PAGE: &str = "page.html";
/// Default template for content items (blog posts, projects, etc.).
const TEMPLATE_CONTENT_DEFAULT: &str = "content/default.html";
/// Fallback template for section index pages.
const TEMPLATE_SECTION_DEFAULT: &str = "section/default.html";
/// Default template for tag listing pages.
const TEMPLATE_TAG_DEFAULT: &str = "tags/default.html";

/// Wrapper around Tera for site-specific template rendering.
pub struct TemplateEngine {
    tera: Tera,
}

impl TemplateEngine {
    /// Load templates from a directory (glob pattern: `templates/**/*`).
    pub fn new(template_dir: &Path) -> CompileResult<Self> {
        let pattern = template_dir.join("**/*").display().to_string();
        let mut tera = Tera::new(&pattern).map_err(CompileError::TemplateLoad)?;

        // Register custom filters
        tera.register_filter("strip_parens", strip_parens_filter);

        Ok(Self { tera })
    }

    /// Render a template by name with the given context.
    pub fn render(&self, template_name: &str, context: &Context) -> CompileResult<String> {
        self.tera
            .render(template_name, context)
            .map_err(|e| CompileError::TemplateRender {
                template: template_name.to_string(),
                source: e,
            })
    }

    /// Render a standalone page (about, collab, etc.).
    ///
    /// Supports per-page template override via `frontmatter.template`,
    /// falling back to `TEMPLATE_PAGE` if not specified.
    pub fn render_page(
        &self,
        content: &Content,
        html_body: &str,
        page_path: &str,
        config: &SiteConfig,
        nav: &[NavItem],
        anchors: &[Anchor],
        absolute: bool,
    ) -> CompileResult<String> {
        let template = content
            .frontmatter
            .template
            .as_deref()
            .unwrap_or(TEMPLATE_PAGE);
        let mut ctx = self.base_context(page_path, config, nav, absolute);
        ctx.insert("title", &content.frontmatter.title);
        ctx.insert(
            "page",
            &FrontmatterContext::new(&content.frontmatter, config),
        );
        ctx.insert("content", html_body);
        ctx.insert("anchors", anchors);
        self.render(template, &ctx)
    }

    /// Render a content item (blog post, project, etc.).
    pub fn render_content(
        &self,
        content: &Content,
        html_body: &str,
        page_path: &str,
        config: &SiteConfig,
        nav: &[NavItem],
        anchors: &[Anchor],
    ) -> CompileResult<String> {
        let template = content
            .frontmatter
            .template
            .as_deref()
            .unwrap_or(TEMPLATE_CONTENT_DEFAULT);
        let mut ctx = self.base_context(page_path, config, nav, false);
        ctx.insert("title", &content.frontmatter.title);
        ctx.insert(
            "page",
            &FrontmatterContext::new(&content.frontmatter, config),
        );
        ctx.insert("content", html_body);
        ctx.insert("anchors", anchors);
        self.render(template, &ctx)
    }

    /// Render a section index page (blog index, projects index).
    ///
    /// Tries `section/<type>.html` first, falls back to `section/default.html`
    /// if no type-specific template exists.
    pub fn render_section(
        &self,
        section: &Content,
        section_type: &str,
        items: &[ContentContext],
        page_path: &str,
        config: &SiteConfig,
        nav: &[NavItem],
    ) -> CompileResult<String> {
        let preferred = format!("section/{}.html", section_type);
        let template = if self.tera.get_template_names().any(|n| n == preferred) {
            preferred
        } else {
            TEMPLATE_SECTION_DEFAULT.to_string()
        };

        let mut ctx = self.base_context(page_path, config, nav, false);
        ctx.insert("title", &section.frontmatter.title);
        ctx.insert(
            "section",
            &FrontmatterContext::new(&section.frontmatter, config),
        );
        ctx.insert("items", items);
        self.render(&template, &ctx)
    }

    /// Render a tag listing page.
    ///
    /// Shows all content items tagged with the given tag.
    pub fn render_tag_page(
        &self,
        tag: &str,
        items: &[ContentContext],
        page_path: &str,
        config: &SiteConfig,
        nav: &[NavItem],
    ) -> CompileResult<String> {
        let mut ctx = self.base_context(page_path, config, nav, false);
        ctx.insert("title", tag);
        ctx.insert("tag", tag);
        ctx.insert("items", items);
        self.render(TEMPLATE_TAG_DEFAULT, &ctx)
    }

    /// Render a raw prose string as a Tera template to resolve embedded variables.
    pub fn render_prose_string(
        &self,
        text: &str,
        page_path: &str,
        config: &SiteConfig,
        nav: &[NavItem],
        absolute: bool,
    ) -> String {
        let ctx = self.base_context(page_path, config, nav, absolute);
        match Tera::one_off(text, &ctx, false) {
            Ok(rendered) => rendered,
            Err(e) => {
                eprintln!("Warning: failed to render template variables in prose: {e}");
                text.to_string()
            },
        }
    }

    /// Build base context with common variables.
    ///
    /// When `absolute` is true, `prefix` is set to the empty string so that
    /// template asset references (`{{ prefix }}/style.css`) produce absolute
    /// root paths (`/style.css`). This is required for pages whose serving
    /// location is unknown at build time (e.g., the 404 page).
    pub(crate) fn base_context(
        &self,
        page_path: &str,
        config: &SiteConfig,
        nav: &[NavItem],
        absolute: bool,
    ) -> Context {
        let mut ctx = Context::new();
        ctx.insert("config", &ConfigContext::from(config));
        ctx.insert("nav", nav);
        ctx.insert("page_path", page_path);
        let prefix = if absolute {
            String::new()
        } else {
            relative_prefix(page_path)
        };
        ctx.insert("prefix", &prefix);
        ctx
    }
}

/// Compute relative path prefix based on page depth.
fn relative_prefix(page_path: &str) -> String {
    let depth = page_path.matches('/').count().saturating_sub(1);
    if depth == 0 {
        ".".to_string()
    } else {
        (0..depth).map(|_| "..").collect::<Vec<_>>().join("/")
    }
}

// ============================================================================
// Context structs for Tera serialization
// ============================================================================

/// Site config context for templates.
#[derive(Serialize)]
pub(crate) struct ConfigContext {
    pub title: String,
    pub author: String,
    pub base_url: String,
    /// Navigation settings for templates.
    pub nav: NavContext,
    /// Extra site-wide variables.
    pub extra: Option<toml::Table>,
}

/// Navigation context for templates.
#[derive(Serialize)]
pub(crate) struct NavContext {
    /// Whether to display nested navigation.
    pub nested: bool,
    /// Whether table of contents is globally enabled.
    pub toc: bool,
}

impl From<&SiteConfig> for ConfigContext {
    fn from(config: &SiteConfig) -> Self {
        Self {
            title: config.title.clone(),
            author: config.author.clone(),
            base_url: config.base_url.trim_end_matches('/').to_string(),
            nav: NavContext {
                nested: config.nav.nested,
                toc: config.nav.toc,
            },
            extra: config.extra.clone(),
        }
    }
}

/// Frontmatter context for templates.
#[derive(Serialize)]
pub struct FrontmatterContext {
    pub title: String,
    pub description: Option<String>,
    pub date: Option<String>,
    pub tags: Vec<String>,
    pub weight: Option<i64>,
    pub link_to: Option<String>,
    /// Enable table of contents (anchor nav in sidebar)
    pub toc: bool,
    /// Whether this content is a draft
    pub draft: bool,
    /// Alternative URL paths
    pub aliases: Vec<String>,
}

impl FrontmatterContext {
    /// Create context from frontmatter with config fallback for toc.
    pub fn new(fm: &crate::content::Frontmatter, config: &SiteConfig) -> Self {
        Self {
            title: fm.title.clone(),
            description: fm.description.clone(),
            date: fm.date.map(|d| d.to_string()),
            tags: fm.tags.iter().map(|t| t.to_string()).collect(),
            weight: fm.weight,
            link_to: fm.link_to.clone(),
            toc: fm.toc.unwrap_or(config.nav.toc),
            draft: fm.draft,
            aliases: fm.aliases.clone(),
        }
    }
}

/// Content item context for section listings.
#[derive(Serialize)]
pub struct ContentContext {
    pub frontmatter: FrontmatterContext,
    pub body: String,
    pub slug: String,
    pub path: String,
}

impl ContentContext {
    pub fn from_content(content: &Content, config: &SiteConfig) -> Self {
        Self {
            frontmatter: FrontmatterContext::new(&content.frontmatter, config),
            body: content.body.clone(),
            slug: content.slug.clone(),
            path: format!("/{}", content.output_path.display()),
        }
    }
}

/// Tera filter to strip parenthetical text from strings.
/// E.g., "Content (in Section)" → "Content"
fn strip_parens_filter(value: &Value, _args: &HashMap<String, Value>) -> tera::Result<Value> {
    match value {
        Value::String(s) => {
            // Find opening paren and trim everything from there
            let result = if let Some(pos) = s.find('(') {
                s[..pos].trim().to_string()
            } else {
                s.clone()
            };
            Ok(Value::String(result))
        },
        _ => Ok(value.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relative_prefix_root() {
        assert_eq!(relative_prefix("/index.html"), ".");
    }

    #[test]
    fn test_relative_prefix_depth_1() {
        assert_eq!(relative_prefix("/blog/index.html"), "..");
    }

    #[test]
    fn test_relative_prefix_depth_2() {
        assert_eq!(relative_prefix("/blog/posts/foo.html"), "../..");
    }

    #[test]
    fn test_absolute_prefix_produces_empty_string() {
        // The 404 page (and any future pages with unknown serving location)
        // must use absolute asset paths. Verify that base_context with
        // absolute=true produces an empty prefix.
        let engine = TemplateEngine {
            tera: Tera::default(),
        };
        let config = SiteConfig {
            title: "Test".to_string(),
            author: "Author".to_string(),
            base_url: "https://example.com".to_string(),
            paths: crate::config::PathsConfig::default(),
            nav: crate::config::NavConfig::default(),
            feed: crate::config::FeedConfig::default(),
            sitemap: crate::config::SitemapConfig::default(),
            extra: None,
        };
        let nav: Vec<NavItem> = vec![];

        let ctx = engine.base_context("/404.html", &config, &nav, true);
        // Tera Context doesn't expose get(), so verify via JSON serialization
        let json = ctx.into_json();
        assert_eq!(json["prefix"], "", "absolute prefix must be empty string");
    }

    #[test]
    fn test_config_context_nav_structure() {
        let config = SiteConfig {
            title: "Test".to_string(),
            author: "Author".to_string(),
            base_url: "https://example.com/".to_string(),
            paths: crate::config::PathsConfig::default(),
            nav: crate::config::NavConfig {
                nested: true,
                toc: true,
            },
            feed: crate::config::FeedConfig::default(),
            sitemap: crate::config::SitemapConfig::default(),
            extra: None,
        };

        let ctx = ConfigContext::from(&config);
        assert!(ctx.nav.nested);
        assert!(ctx.nav.toc);
        assert_eq!(
            ctx.base_url, "https://example.com",
            "trailing slash trimmed"
        );
        assert_eq!(ctx.title, "Test");
        assert_eq!(ctx.author, "Author");
    }

    #[test]
    fn test_toc_config_fallback() {
        use crate::content::Frontmatter;

        // Create configs with different toc defaults
        let config_toc_true = SiteConfig {
            title: "Test".to_string(),
            author: "Test".to_string(),
            base_url: "https://test.com".to_string(),
            paths: crate::config::PathsConfig::default(),
            nav: crate::config::NavConfig {
                nested: false,
                toc: true,
            },
            feed: crate::config::FeedConfig::default(),
            sitemap: crate::config::SitemapConfig::default(),
            extra: None,
        };

        let config_toc_false = SiteConfig {
            title: "Test".to_string(),
            author: "Test".to_string(),
            base_url: "https://test.com".to_string(),
            paths: crate::config::PathsConfig::default(),
            nav: crate::config::NavConfig {
                nested: false,
                toc: false,
            },
            feed: crate::config::FeedConfig::default(),
            sitemap: crate::config::SitemapConfig::default(),
            extra: None,
        };

        // Frontmatter with explicit toc: true
        let fm_explicit_true = Frontmatter {
            title: "Test".to_string(),
            description: None,
            date: None,
            tags: vec![],
            weight: None,
            link_to: None,
            nav_label: None,
            section_type: None,
            template: None,
            toc: Some(true),
            draft: false,
            aliases: vec![],
        };

        // Frontmatter with explicit toc: false
        let fm_explicit_false = Frontmatter {
            title: "Test".to_string(),
            description: None,
            date: None,
            tags: vec![],
            weight: None,
            link_to: None,
            nav_label: None,
            section_type: None,
            template: None,
            toc: Some(false),
            draft: false,
            aliases: vec![],
        };

        // Frontmatter with no toc specified (None)
        let fm_none = Frontmatter {
            title: "Test".to_string(),
            description: None,
            date: None,
            tags: vec![],
            weight: None,
            link_to: None,
            nav_label: None,
            section_type: None,
            template: None,
            toc: None,
            draft: false,
            aliases: vec![],
        };

        // Explicit true overrides config false
        let ctx = FrontmatterContext::new(&fm_explicit_true, &config_toc_false);
        assert!(
            ctx.toc,
            "explicit toc: true should override config toc: false"
        );

        // Explicit false overrides config true
        let ctx = FrontmatterContext::new(&fm_explicit_false, &config_toc_true);
        assert!(
            !ctx.toc,
            "explicit toc: false should override config toc: true"
        );

        // None falls back to config true
        let ctx = FrontmatterContext::new(&fm_none, &config_toc_true);
        assert!(ctx.toc, "toc: None should fall back to config toc: true");

        // None falls back to config false
        let ctx = FrontmatterContext::new(&fm_none, &config_toc_false);
        assert!(!ctx.toc, "toc: None should fall back to config toc: false");
    }
}
