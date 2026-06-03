//! CSS processing via lightningcss.
//!
//! Provides CSS bundling and minification. The bundler resolves `@import`
//! rules at build time, inlining imported files into a single output.

use std::path::Path;

use lightningcss::bundler::{Bundler, FileProvider};
use lightningcss::stylesheet::{MinifyOptions, ParserOptions, PrinterOptions};

use crate::error::{CompileError, CompileResult};

/// Bundle and minify a CSS file, resolving all `@import` rules.
///
/// This function:
/// 1. Reads the CSS file at `path`
/// 2. Resolves and inlines all `@import` rules (relative to source file)
/// 3. Minifies the combined output
///
/// Returns minified CSS string on success.
pub fn bundle_css(path: &Path) -> CompileResult<String> {
    let fs = FileProvider::new();
    let mut bundler = Bundler::new(
        &fs,
        None,
        ParserOptions {
            error_recovery: true,
            ..ParserOptions::default()
        },
    );

    let mut stylesheet = bundler
        .bundle(path)
        .map_err(|e| CompileError::CssBundle(format!("bundle error: {e}")))?;

    stylesheet
        .minify(MinifyOptions::default())
        .map_err(|e| CompileError::CssBundle(format!("minify error: {e}")))?;

    let result = stylesheet
        .to_css(PrinterOptions {
            minify: true,
            ..Default::default()
        })
        .map_err(|e| CompileError::CssBundle(format!("print error: {e}")))?;

    Ok(result.code)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn test_bundle_minifies() {
        let dir = TempDir::new().unwrap();
        let css_path = dir.path().join("test.css");
        fs::write(
            &css_path,
            r#"
            .foo {
                color: red;
            }
        "#,
        )
        .unwrap();

        let output = bundle_css(&css_path).unwrap();

        // Should be minified (whitespace removed)
        assert!(output.contains(".foo"));
        assert!(output.contains("red"));
        assert!(!output.contains('\n'));
    }

    #[test]
    fn test_bundle_resolves_imports() {
        let dir = TempDir::new().unwrap();

        // Create imported file
        let imported_path = dir.path().join("colors.css");
        fs::write(
            &imported_path,
            r#"
            :root {
                --primary: blue;
            }
        "#,
        )
        .unwrap();

        // Create main file that imports colors.css
        let main_path = dir.path().join("main.css");
        fs::write(
            &main_path,
            r#"
            @import "colors.css";
            
            .btn {
                color: var(--primary);
            }
        "#,
        )
        .unwrap();

        let output = bundle_css(&main_path).unwrap();

        // Should contain content from both files
        assert!(output.contains("--primary"));
        assert!(output.contains("blue"));
        assert!(output.contains(".btn"));
        // Should NOT contain @import directive
        assert!(!output.contains("@import"));
    }

    #[test]
    fn test_bundle_removes_comments() {
        let dir = TempDir::new().unwrap();
        let css_path = dir.path().join("test.css");
        fs::write(
            &css_path,
            r#"
            /* This is a comment */
            .bar { background: blue; }
        "#,
        )
        .unwrap();

        let output = bundle_css(&css_path).unwrap();

        // Comment should be removed
        assert!(!output.contains("This is a comment"));
        // Rule should remain
        assert!(output.contains(".bar"));
    }
}
