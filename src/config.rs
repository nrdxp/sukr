//! Site configuration loading.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{ParseError, ParseResult};

/// Site-wide configuration loaded from site.toml.
#[derive(Debug, Deserialize)]
pub struct SiteConfig {
    /// Site title (used in page titles and nav).
    pub title: String,
    /// Site author name.
    pub author: String,
    /// Base URL for the site (used for feeds, canonical links).
    pub base_url: String,
    /// Path configuration (all optional with defaults).
    #[serde(default)]
    pub paths: PathsConfig,
    /// Navigation configuration.
    #[serde(default)]
    pub nav: NavConfig,
    /// Feed (Atom) configuration.
    #[serde(default)]
    pub feed: FeedConfig,
    /// Sitemap configuration.
    #[serde(default)]
    pub sitemap: SitemapConfig,
    /// Extra arbitrary configuration keys.
    #[serde(default)]
    pub extra: Option<toml::Table>,
}

/// Feed (Atom) generation configuration.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct FeedConfig {
    /// Whether to generate an Atom feed (default: true).
    pub enabled: bool,
}

impl Default for FeedConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Sitemap generation configuration.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct SitemapConfig {
    /// Whether to generate a sitemap.xml (default: true).
    pub enabled: bool,
}

impl Default for SitemapConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Navigation configuration.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct NavConfig {
    /// Whether to display nested navigation (default: false).
    pub nested: bool,
    /// Enable table of contents (anchor nav) globally (default: false).
    /// Can be overridden per-page via frontmatter toc field.
    pub toc: bool,
}

/// Path configuration with sensible defaults.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct PathsConfig {
    /// Content directory (default: "content")
    pub content: PathBuf,
    /// Output directory (default: "public")
    pub output: PathBuf,
    /// Static assets directory (default: "static")
    #[serde(rename = "static")]
    pub static_dir: PathBuf,
    /// Templates directory (default: "templates")
    pub templates: PathBuf,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            content: PathBuf::from("content"),
            output: PathBuf::from("public"),
            static_dir: PathBuf::from("static"),
            templates: PathBuf::from("templates"),
        }
    }
}

impl SiteConfig {
    /// Load configuration from a TOML file.
    pub fn load(path: &Path) -> ParseResult<Self> {
        let content = fs::read_to_string(path).map_err(|e| ParseError::ReadFile {
            path: path.to_path_buf(),
            source: e,
        })?;

        toml::from_str(&content).map_err(|e| ParseError::Config {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config() {
        let toml = r#"
            title = "Test Site"
            author = "Test Author"
            base_url = "https://example.com/"
        "#;

        let config: SiteConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.title, "Test Site");
        assert_eq!(config.author, "Test Author");
        assert_eq!(config.base_url, "https://example.com/");
    }

    #[test]
    fn test_parse_config_extra() {
        let toml = r##"
            title = "Test Site"
            author = "Test Author"
            base_url = "https://example.com/"
            
            [extra]
            description = "Some description"
            branding_color = "#58a6ff"
        "##;

        let config: SiteConfig = toml::from_str(toml).unwrap();
        let extra = config.extra.expect("extra should be present");
        assert_eq!(
            extra.get("description").unwrap().as_str().unwrap(),
            "Some description"
        );
        assert_eq!(
            extra.get("branding_color").unwrap().as_str().unwrap(),
            "#58a6ff"
        );
    }

    #[test]
    fn test_paths_config_defaults() {
        let toml = r#"
            title = "Test"
            author = "Author"
            base_url = "https://example.com"
        "#;

        let config: SiteConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.paths.content, PathBuf::from("content"));
        assert_eq!(config.paths.output, PathBuf::from("public"));
        assert_eq!(config.paths.static_dir, PathBuf::from("static"));
        assert_eq!(config.paths.templates, PathBuf::from("templates"));
    }

    #[test]
    fn test_paths_config_custom() {
        let toml = r#"
            title = "Test"
            author = "Author"
            base_url = "https://example.com"

            [paths]
            content = "src/content"
            output = "dist"
            static = "assets"
            templates = "theme"
        "#;

        let config: SiteConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.paths.content, PathBuf::from("src/content"));
        assert_eq!(config.paths.output, PathBuf::from("dist"));
        assert_eq!(config.paths.static_dir, PathBuf::from("assets"));
        assert_eq!(config.paths.templates, PathBuf::from("theme"));
    }

    #[test]
    fn test_feed_sitemap_defaults() {
        let toml = r#"
            title = "Test"
            author = "Author"
            base_url = "https://example.com"
        "#;

        let config: SiteConfig = toml::from_str(toml).unwrap();
        assert!(config.feed.enabled, "feed should be enabled by default");
        assert!(
            config.sitemap.enabled,
            "sitemap should be enabled by default"
        );
    }

    #[test]
    fn test_feed_disabled() {
        let toml = r#"
            title = "Test"
            author = "Author"
            base_url = "https://example.com"

            [feed]
            enabled = false
        "#;

        let config: SiteConfig = toml::from_str(toml).unwrap();
        assert!(!config.feed.enabled);
        assert!(config.sitemap.enabled, "sitemap unaffected");
    }

    #[test]
    fn test_sitemap_disabled() {
        let toml = r#"
            title = "Test"
            author = "Author"
            base_url = "https://example.com"

            [sitemap]
            enabled = false
        "#;

        let config: SiteConfig = toml::from_str(toml).unwrap();
        assert!(config.feed.enabled, "feed unaffected");
        assert!(!config.sitemap.enabled);
    }
}
