//! sukr - Minimal static site compiler.
//!
//! Suckless, Rust, zero JS. Transforms markdown into static HTML.

mod config;
mod content;
mod css;
mod error;
mod escape;
mod feed;
mod highlight;
mod math;
mod mermaid;
mod render;
mod sitemap;
mod template_engine;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::content::{Content, NavItem, OUTPUT_INDEX};
use crate::error::{CompileError, CompileResult, ParseError, Result};
use crate::template_engine::{ContentContext, TemplateEngine};

/// Output filename for the Atom feed.
const OUTPUT_FEED: &str = "feed.xml";
/// Output filename for the XML sitemap.
const OUTPUT_SITEMAP: &str = "sitemap.xml";
/// Output filename for the 404 error page.
const OUTPUT_404: &str = "404.html";

const USAGE: &str = "\
sukr - Minimal static site compiler

USAGE:
    sukr [OPTIONS]

OPTIONS:
    -c, --config <FILE>  Path to site.toml config file (default: ./site.toml)
    -h, --help           Print this help message
";

fn main() {
    match parse_args() {
        Ok(Some(config_path)) => {
            if let Err(e) = run(&config_path) {
                eprintln!("error: {e}");
                // Print full error chain
                let mut source = std::error::Error::source(&e);
                while let Some(cause) = source {
                    eprintln!("  caused by: {cause}");
                    source = std::error::Error::source(cause);
                }
                std::process::exit(1);
            }
        },
        Ok(None) => {}, // --help was printed
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        },
    }
}

/// Parse command-line arguments. Returns None if --help was requested.
fn parse_args() -> std::result::Result<Option<PathBuf>, String> {
    let args: Vec<_> = std::env::args().collect();
    let mut config_path = PathBuf::from("site.toml");
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(None);
            },
            "-c" | "--config" => {
                if i + 1 >= args.len() {
                    return Err("--config requires an argument".to_string());
                }
                config_path = PathBuf::from(&args[i + 1]);
                i += 2;
            },
            arg => {
                return Err(format!("unknown argument: {arg}"));
            },
        }
    }

    Ok(Some(config_path))
}

fn run(config_path: &Path) -> Result<()> {
    // ── Bootstrap ─────────────────────────────────────────────────────
    // Load configuration and resolve paths.
    let config = config::SiteConfig::load(config_path)?;

    let base_dir = config_path.parent().unwrap_or(Path::new("."));
    let content_dir = base_dir.join(&config.paths.content);
    let output_dir = base_dir.join(&config.paths.output);
    let static_dir = base_dir.join(&config.paths.static_dir);
    let template_dir = base_dir.join(&config.paths.templates);

    if !content_dir.exists() {
        return Err(ParseError::ContentDirNotFound(content_dir.to_path_buf()).into());
    }

    let engine = TemplateEngine::new(&template_dir)?;

    // ── Parse phase (S → C) ──────────────────────────────────────────
    // Discover site structure and parse all content in a single pass.
    let manifest = content::SiteManifest::discover(&content_dir)?;

    // ── Compile phase (C → O) ────────────────────────────────────────
    // Render content blocks, apply templates, and write output.

    // 0. Copy static assets
    copy_static_assets(&static_dir, &output_dir)?;

    // 1. Process all sections
    for section in &manifest.sections {
        eprintln!("processing section: {}", section.name);

        // Items are pre-sorted at construction in discover_sections
        let items = &section.items;

        // Render individual content pages for all sections
        for item in items {
            eprintln!("  processing: {}", item.slug);
            let page_path = format!("/{}", item.output_path.display());
            let render_prose = |text: &str| {
                engine.render_prose_string(text, &page_path, &config, &manifest.nav, false)
            };
            let (html_body, anchors) =
                render::render_blocks(&item.blocks, render_prose).map_err(CompileError::Render)?;
            let html = engine.render_content(
                item,
                &html_body,
                &page_path,
                &config,
                &manifest.nav,
                &anchors,
            )?;
            write_output(&output_dir, item, html)?;
        }

        // Render section index
        let page_path = format!("/{}/index.html", section.name);
        let item_contexts: Vec<_> = items
            .iter()
            .map(|c| ContentContext::from_content(c, &config))
            .collect();
        let html = engine.render_section(
            &section.index,
            &section.section_type.to_string(),
            &item_contexts,
            &page_path,
            &config,
            &manifest.nav,
        );

        let out_path = output_dir.join(&section.name).join(OUTPUT_INDEX);
        fs::create_dir_all(out_path.parent().unwrap()).map_err(|e| CompileError::CreateDir {
            path: out_path.parent().unwrap().to_path_buf(),
            source: e,
        })?;
        fs::write(&out_path, html?).map_err(|e| CompileError::WriteFile {
            path: out_path.clone(),
            source: e,
        })?;
        eprintln!("  → {}", out_path.display());
    }

    // 2. Generate Atom feed (blog posts only, if enabled)
    if config.feed.enabled && !manifest.posts.is_empty() {
        generate_feed(&output_dir, &manifest, &config)?;
    }

    // 3. Process standalone pages
    for page in &manifest.pages {
        eprintln!("processing: {}", page.slug);
        let page_path = format!("/{}", page.output_path.display());
        let render_prose = |text: &str| {
            engine.render_prose_string(text, &page_path, &config, &manifest.nav, false)
        };
        let (html_body, anchors) =
            render::render_blocks(&page.blocks, render_prose).map_err(CompileError::Render)?;
        let html = engine.render_page(
            page,
            &html_body,
            &page_path,
            &config,
            &manifest.nav,
            &anchors,
            false,
        )?;
        write_output(&output_dir, page, html)?;
    }

    // 4. Generate homepage
    generate_homepage(&manifest, &output_dir, &config, &engine)?;

    // 5. Generate 404 page (if _404.md exists)
    if let Some(ref page_404) = manifest.page_404 {
        generate_404(page_404, &output_dir, &config, &manifest.nav, &engine)?;
    }

    // 6. Collect tags and generate tag listing pages
    let tags = collect_tags(&manifest.sections, &manifest.pages, &config);
    if !tags.is_empty() {
        write_tag_pages(&output_dir, &tags, &config, &manifest.nav, &engine)?;
    }

    // 7. Generate sitemap (if enabled)
    let tag_names: Vec<String> = tags.keys().cloned().collect();
    if config.sitemap.enabled {
        generate_sitemap_file(&output_dir, &manifest, &config, &tag_names)?;
    }

    // 8. Generate alias redirects
    generate_aliases(&output_dir, &manifest, &config)?;

    eprintln!("done!");
    Ok(())
}

/// Generate the Atom feed
fn generate_feed(
    output_dir: &Path,
    manifest: &content::SiteManifest,
    config: &config::SiteConfig,
) -> CompileResult<()> {
    let out_path = output_dir.join(OUTPUT_FEED);
    eprintln!("generating: {}", out_path.display());

    let feed_xml = feed::generate_atom_feed(manifest, config);

    fs::write(&out_path, feed_xml).map_err(|e| CompileError::WriteFile {
        path: out_path.clone(),
        source: e,
    })?;

    eprintln!("  → {}", out_path.display());
    Ok(())
}

/// Generate the XML sitemap
fn generate_sitemap_file(
    output_dir: &Path,
    manifest: &content::SiteManifest,
    config: &config::SiteConfig,
    tag_names: &[String],
) -> CompileResult<()> {
    let out_path = output_dir.join(OUTPUT_SITEMAP);
    eprintln!("generating: {}", out_path.display());

    let sitemap_xml = sitemap::generate_sitemap(manifest, config, tag_names);

    fs::write(&out_path, sitemap_xml).map_err(|e| CompileError::WriteFile {
        path: out_path.clone(),
        source: e,
    })?;

    eprintln!("  → {}", out_path.display());
    Ok(())
}

/// Generate the homepage from manifest.homepage
fn generate_homepage(
    manifest: &content::SiteManifest,
    output_dir: &Path,
    config: &config::SiteConfig,
    engine: &TemplateEngine,
) -> CompileResult<()> {
    eprintln!("generating: homepage");

    let page_path = format!("/{OUTPUT_INDEX}");
    let render_prose =
        |text: &str| engine.render_prose_string(text, &page_path, config, &manifest.nav, false);
    let (html_body, anchors) = render::render_blocks(&manifest.homepage.blocks, render_prose)
        .map_err(CompileError::Render)?;
    let html = engine.render_page(
        &manifest.homepage,
        &html_body,
        &page_path,
        config,
        &manifest.nav,
        &anchors,
        false,
    )?;

    let out_path = output_dir.join(OUTPUT_INDEX);

    fs::create_dir_all(output_dir).map_err(|e| CompileError::CreateDir {
        path: output_dir.to_path_buf(),
        source: e,
    })?;

    fs::write(&out_path, html).map_err(|e| CompileError::WriteFile {
        path: out_path.clone(),
        source: e,
    })?;

    eprintln!("  → {}", out_path.display());
    Ok(())
}

/// Generate the 404 error page from manifest.page_404
fn generate_404(
    page_404: &Content,
    output_dir: &Path,
    config: &config::SiteConfig,
    nav: &[NavItem],
    engine: &TemplateEngine,
) -> CompileResult<()> {
    eprintln!("generating: 404 page");

    let page_path = format!("/{OUTPUT_404}");
    let render_prose = |text: &str| engine.render_prose_string(text, &page_path, config, nav, true);
    let (html_body, anchors) =
        render::render_blocks(&page_404.blocks, render_prose).map_err(CompileError::Render)?;
    let html = engine.render_page(
        page_404, &html_body, &page_path, config, nav, &anchors,
        true, // 404 needs absolute paths — serving location is unknown
    )?;

    let out_path = output_dir.join(OUTPUT_404);
    fs::write(&out_path, html).map_err(|e| CompileError::WriteFile {
        path: out_path.clone(),
        source: e,
    })?;

    eprintln!("  → {}", out_path.display());
    Ok(())
}

/// Collect all unique tags across section items and standalone pages.
///
/// Returns a sorted map of tag → tagged items for deterministic output.
fn collect_tags(
    sections: &[content::Section],
    pages: &[Content],
    config: &config::SiteConfig,
) -> BTreeMap<String, Vec<ContentContext>> {
    let mut tags: BTreeMap<String, Vec<ContentContext>> = BTreeMap::new();

    // Collect from section items
    for section in sections {
        for item in &section.items {
            for tag in &item.frontmatter.tags {
                tags.entry(tag.to_string())
                    .or_default()
                    .push(ContentContext::from_content(item, config));
            }
        }
    }

    // Collect from standalone pages
    for page in pages {
        for tag in &page.frontmatter.tags {
            tags.entry(tag.to_string())
                .or_default()
                .push(ContentContext::from_content(page, config));
        }
    }

    tags
}

/// Write tag listing pages from pre-collected tag data.
fn write_tag_pages(
    output_dir: &Path,
    tags: &BTreeMap<String, Vec<ContentContext>>,
    config: &config::SiteConfig,
    nav: &[NavItem],
    engine: &TemplateEngine,
) -> CompileResult<()> {
    let tags_dir = output_dir.join("tags");
    fs::create_dir_all(&tags_dir).map_err(|e| CompileError::CreateDir {
        path: tags_dir.clone(),
        source: e,
    })?;

    for (tag, items) in tags {
        let page_path = format!("/tags/{}.html", tag);
        let html = engine.render_tag_page(tag, items, &page_path, config, nav)?;

        let out_path = tags_dir.join(format!("{}.html", tag));
        fs::write(&out_path, html).map_err(|e| CompileError::WriteFile {
            path: out_path.clone(),
            source: e,
        })?;

        eprintln!(
            "  tag: {} ({} items) → {}",
            tag,
            items.len(),
            out_path.display()
        );
    }

    Ok(())
}

/// Write a content item to its output path
fn write_output(output_dir: &Path, content: &Content, html: String) -> CompileResult<()> {
    let out_path = output_dir.join(&content.output_path);
    let out_dir = out_path.parent().unwrap();

    fs::create_dir_all(out_dir).map_err(|e| CompileError::CreateDir {
        path: out_dir.to_path_buf(),
        source: e,
    })?;

    fs::write(&out_path, html).map_err(|e| CompileError::WriteFile {
        path: out_path.clone(),
        source: e,
    })?;

    eprintln!("  → {}", out_path.display());
    Ok(())
}

/// Generate HTML redirect stubs for alias paths.
///
/// For each content item with `aliases = ["/old/path", ...]` in frontmatter,
/// writes a minimal HTML file at the alias path that redirects to the canonical URL.
fn generate_aliases(
    output_dir: &Path,
    manifest: &content::SiteManifest,
    config: &config::SiteConfig,
) -> CompileResult<()> {
    let base_url = config.base_url.trim_end_matches('/');

    // Process section items
    for section in &manifest.sections {
        for item in &section.items {
            write_aliases(output_dir, item, base_url)?;
        }
    }

    // Process standalone pages
    for page in &manifest.pages {
        write_aliases(output_dir, page, base_url)?;
    }

    Ok(())
}

/// Write redirect stubs for a single content item's aliases.
fn write_aliases(output_dir: &Path, content: &Content, base_url: &str) -> CompileResult<()> {
    if content.frontmatter.aliases.is_empty() {
        return Ok(());
    }

    let canonical_path = &content.output_path;
    let canonical_url = format!("{}/{}", base_url, canonical_path.display());

    for alias in &content.frontmatter.aliases {
        let alias_path = alias.trim_start_matches('/');
        // Append index.html if alias ends with / or has no extension
        let alias_file = if alias_path.ends_with('/') || !alias_path.contains('.') {
            format!("{}/index.html", alias_path.trim_end_matches('/'))
        } else {
            alias_path.to_string()
        };

        let out_path = output_dir.join(&alias_file);
        let out_dir = out_path.parent().unwrap();

        fs::create_dir_all(out_dir).map_err(|e| CompileError::CreateDir {
            path: out_dir.to_path_buf(),
            source: e,
        })?;

        let html = redirect_html(&canonical_url);
        fs::write(&out_path, html).map_err(|e| CompileError::WriteFile {
            path: out_path.clone(),
            source: e,
        })?;

        eprintln!("  alias: {} → {}", alias, canonical_url);
    }

    Ok(())
}

/// Generate minimal HTML for a redirect page.
fn redirect_html(target_url: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta http-equiv="refresh" content="0; url={url}">
  <link rel="canonical" href="{url}">
</head>
<body>
  <p>Redirecting to <a href="{url}">{url}</a></p>
</body>
</html>
"#,
        url = target_url,
    )
}

/// Copy static assets (CSS, images, etc.) to output directory.
/// CSS files are minified before writing.
fn copy_static_assets(static_dir: &Path, output_dir: &Path) -> CompileResult<()> {
    use crate::css::bundle_css;

    if !static_dir.exists() {
        return Ok(()); // No static dir is fine
    }

    fs::create_dir_all(output_dir).map_err(|e| CompileError::CreateDir {
        path: output_dir.to_path_buf(),
        source: e,
    })?;

    for src in walk_dir(static_dir)? {
        let relative = src.strip_prefix(static_dir).unwrap();
        let dest = output_dir.join(relative);

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| CompileError::CreateDir {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }

        // Bundle CSS files (resolves @imports), copy others directly
        if src.extension().is_some_and(|ext| ext == "css") {
            let original_size = fs::metadata(&src).map(|m| m.len()).unwrap_or(0);
            let bundled = bundle_css(&src)?;
            fs::write(&dest, &bundled).map_err(|e| CompileError::WriteFile {
                path: dest.clone(),
                source: e,
            })?;
            eprintln!(
                "bundling: {} → {} ({} → {} bytes)",
                src.display(),
                dest.display(),
                original_size,
                bundled.len()
            );
        } else {
            fs::copy(&src, &dest).map_err(|e| CompileError::WriteFile {
                path: dest.clone(),
                source: e,
            })?;
            eprintln!("copying: {} → {}", src.display(), dest.display());
        }
    }

    Ok(())
}

/// Recursively walk a directory, returning all file paths.
fn walk_dir(dir: &Path) -> CompileResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    walk_dir_inner(dir, &mut files)?;
    Ok(files)
}

fn walk_dir_inner(dir: &Path, files: &mut Vec<PathBuf>) -> CompileResult<()> {
    let entries = fs::read_dir(dir).map_err(|e| CompileError::ReadDir {
        path: dir.to_path_buf(),
        source: e,
    })?;

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            walk_dir_inner(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redirect_html_contains_meta_refresh() {
        let html = redirect_html("https://example.com/blog/new-post.html");
        assert!(html.contains(r#"http-equiv="refresh""#));
        assert!(html.contains(r#"content="0; url=https://example.com/blog/new-post.html""#));
        assert!(html.contains(r#"rel="canonical""#));
        assert!(html.contains(r#"href="https://example.com/blog/new-post.html""#));
    }

    #[test]
    fn test_redirect_html_is_valid_document() {
        let html = redirect_html("https://example.com/target.html");
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<html>"));
        assert!(html.contains("</html>"));
        assert!(html.contains("<head>"));
        assert!(html.contains("</head>"));
    }

    #[test]
    fn test_collect_tags_groups_items() {
        let dir = tempfile::tempdir().unwrap();
        let content_dir = dir.path();

        // Create a section with tagged items
        let section_dir = content_dir.join("blog");
        std::fs::create_dir_all(&section_dir).unwrap();
        std::fs::write(
            section_dir.join("_index.md"),
            "+++\ntitle = \"Blog\"\nsection_type = \"blog\"\n+++\n",
        )
        .unwrap();
        std::fs::write(
            section_dir.join("post1.md"),
            "+++\ntitle = \"Post 1\"\ntags = [\"rust\", \"web\"]\n+++\n\nBody.",
        )
        .unwrap();
        std::fs::write(
            section_dir.join("post2.md"),
            "+++\ntitle = \"Post 2\"\ntags = [\"rust\"]\n+++\n\nBody.",
        )
        .unwrap();

        let sections = content::discover_sections(content_dir).unwrap();
        let pages: Vec<Content> = vec![];
        let config = config::SiteConfig {
            title: String::new(),
            author: String::new(),
            base_url: "https://example.com".into(),
            paths: Default::default(),
            nav: Default::default(),
            feed: Default::default(),
            sitemap: Default::default(),
            extra: None,
        };

        let tags = collect_tags(&sections, &pages, &config);
        assert_eq!(tags.len(), 2, "should have 2 unique tags");
        assert_eq!(tags["rust"].len(), 2, "rust tag should have 2 items");
        assert_eq!(tags["web"].len(), 1, "web tag should have 1 item");
    }

    #[test]
    fn test_collect_tags_empty_when_no_tags() {
        let dir = tempfile::tempdir().unwrap();
        let content_dir = dir.path();

        // Create a section with untagged items
        let section_dir = content_dir.join("docs");
        std::fs::create_dir_all(&section_dir).unwrap();
        std::fs::write(
            section_dir.join("_index.md"),
            "+++\ntitle = \"Docs\"\n+++\n",
        )
        .unwrap();
        std::fs::write(
            section_dir.join("page.md"),
            "+++\ntitle = \"A Page\"\n+++\n\nBody.",
        )
        .unwrap();

        let sections = content::discover_sections(content_dir).unwrap();
        let pages: Vec<Content> = vec![];
        let config = config::SiteConfig {
            title: String::new(),
            author: String::new(),
            base_url: "https://example.com".into(),
            paths: Default::default(),
            nav: Default::default(),
            feed: Default::default(),
            sitemap: Default::default(),
            extra: None,
        };

        let tags = collect_tags(&sections, &pages, &config);
        assert!(tags.is_empty(), "should have no tags");
    }
}
