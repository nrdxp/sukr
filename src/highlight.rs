//! Syntax highlighting via tree-house (Helix's tree-sitter integration).
//!
//! Uses curated queries from Helix for comprehensive syntax highlighting
//! with support for language injections (e.g., bash in Nix strings).

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

use ropey::RopeSlice;
use tree_house::highlighter::{Highlight, HighlightEvent, Highlighter};
use tree_house::{
    InjectionLanguageMarker, Language as THLanguage, LanguageConfig, LanguageLoader, Syntax,
};
use tree_house_bindings::Grammar;

use crate::escape::{code_escape, code_escape_into};

/// Supported languages for syntax highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Bash,
    C,
    Css,
    Go,
    Html,
    JavaScript,
    Json,
    Markdown,
    Nix,
    Python,
    Rust,
    Toml,
    TypeScript,
    Yaml,
}

impl Language {
    /// Parse a language identifier from a code fence.
    pub fn from_fence(lang: &str) -> Option<Self> {
        match lang.to_lowercase().as_str() {
            "bash" | "sh" | "shell" | "zsh" => Some(Language::Bash),
            "c" => Some(Language::C),
            "css" => Some(Language::Css),
            "go" | "golang" => Some(Language::Go),
            "html" => Some(Language::Html),
            "javascript" | "js" => Some(Language::JavaScript),
            "json" => Some(Language::Json),
            "markdown" | "md" => Some(Language::Markdown),
            "nix" => Some(Language::Nix),
            "python" | "py" => Some(Language::Python),
            "rust" | "rs" => Some(Language::Rust),
            "toml" => Some(Language::Toml),
            "typescript" | "ts" | "tsx" => Some(Language::TypeScript),
            "yaml" | "yml" => Some(Language::Yaml),
            _ => None,
        }
    }

    /// Convert to tree-house Language index.
    fn to_th_language(self) -> THLanguage {
        THLanguage::new(self as u32)
    }

    /// Convert from tree-house Language index.
    fn from_th_language(lang: THLanguage) -> Option<Self> {
        match lang.0 {
            0 => Some(Language::Bash),
            1 => Some(Language::C),
            2 => Some(Language::Css),
            3 => Some(Language::Go),
            4 => Some(Language::Html),
            5 => Some(Language::JavaScript),
            6 => Some(Language::Json),
            7 => Some(Language::Markdown),
            8 => Some(Language::Nix),
            9 => Some(Language::Python),
            10 => Some(Language::Rust),
            11 => Some(Language::Toml),
            12 => Some(Language::TypeScript),
            13 => Some(Language::Yaml),
            _ => None,
        }
    }
}

/// Create a LanguageConfig for a language with embedded queries.
fn make_config(
    grammar: Grammar,
    highlights: &str,
    injections: &str,
    locals: &str,
) -> Option<LanguageConfig> {
    LanguageConfig::new(grammar, highlights, injections, locals).ok()
}

/// Scope-to-highlight mapping with hierarchical fallback.
/// Returns a HashMap of scope name -> Highlight index.
fn build_scope_map() -> HashMap<&'static str, Highlight> {
    // Comprehensive list of scopes from Helix queries.
    static SCOPES: &[&str] = &[
        // Keywords
        "keyword",
        "keyword.control",
        "keyword.control.conditional",
        "keyword.control.repeat",
        "keyword.control.import",
        "keyword.control.return",
        "keyword.control.exception",
        "keyword.operator",
        "keyword.directive",
        "keyword.function",
        "keyword.return",
        "keyword.storage",
        "keyword.storage.type",
        "keyword.storage.modifier",
        "keyword.storage.modifier.mut",
        "keyword.storage.modifier.ref",
        "keyword.special",
        // Functions
        "function",
        "function.builtin",
        "function.call",
        "function.macro",
        "function.method",
        // Types
        "type",
        "type.builtin",
        "type.parameter",
        "type.enum.variant",
        "type.enum.variant.builtin",
        // Constants
        "constant",
        "constant.builtin",
        "constant.builtin.boolean",
        "constant.character",
        "constant.character.escape",
        "constant.macro",
        "constant.numeric",
        "constant.numeric.integer",
        "constant.numeric.float",
        // Strings
        "string",
        "string.regexp",
        "string.special",
        "string.special.path",
        "string.special.symbol",
        // Variables
        "variable",
        "variable.builtin",
        "variable.parameter",
        "variable.other",
        "variable.other.member",
        // Comments
        "comment",
        "comment.line",
        "comment.block",
        "comment.block.documentation",
        "comment.line.documentation",
        "comment.unused",
        // Punctuation
        "punctuation",
        "punctuation.bracket",
        "punctuation.delimiter",
        "punctuation.special",
        // Operators
        "operator",
        // Other
        "attribute",
        "label",
        "namespace",
        "constructor",
        "special",
        "tag",
        "tag.attribute",
        "tag.delimiter",
        // Markup
        "markup.bold",
        "markup.italic",
        "markup.strikethrough",
        "markup.heading",
        "markup.link.text",
        "markup.link.url",
        "markup.list",
        "markup.quote",
        "markup.raw",
    ];

    SCOPES
        .iter()
        .enumerate()
        .map(|(i, &scope)| (scope, Highlight::new(i as u32)))
        .collect()
}

/// Static scope map for highlight resolution.
static SCOPE_MAP: LazyLock<HashMap<&'static str, Highlight>> = LazyLock::new(build_scope_map);

/// Static CSS class names for each scope.
static SCOPE_CLASSES: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    vec![
        "hl-keyword",
        "hl-keyword-control",
        "hl-keyword-control-conditional",
        "hl-keyword-control-repeat",
        "hl-keyword-control-import",
        "hl-keyword-control-return",
        "hl-keyword-control-exception",
        "hl-keyword-operator",
        "hl-keyword-directive",
        "hl-keyword-function",
        "hl-keyword-return",
        "hl-keyword-storage",
        "hl-keyword-storage-type",
        "hl-keyword-storage-modifier",
        "hl-keyword-storage-modifier-mut",
        "hl-keyword-storage-modifier-ref",
        "hl-keyword-special",
        "hl-function",
        "hl-function-builtin",
        "hl-function-call",
        "hl-function-macro",
        "hl-function-method",
        "hl-type",
        "hl-type-builtin",
        "hl-type-parameter",
        "hl-type-enum-variant",
        "hl-type-enum-variant-builtin",
        "hl-constant",
        "hl-constant-builtin",
        "hl-constant-builtin-boolean",
        "hl-constant-character",
        "hl-constant-character-escape",
        "hl-constant-macro",
        "hl-constant-numeric",
        "hl-constant-numeric-integer",
        "hl-constant-numeric-float",
        "hl-string",
        "hl-string-regexp",
        "hl-string-special",
        "hl-string-special-path",
        "hl-string-special-symbol",
        "hl-variable",
        "hl-variable-builtin",
        "hl-variable-parameter",
        "hl-variable-other",
        "hl-variable-other-member",
        "hl-comment",
        "hl-comment-line",
        "hl-comment-block",
        "hl-comment-block-documentation",
        "hl-comment-line-documentation",
        "hl-comment-unused",
        "hl-punctuation",
        "hl-punctuation-bracket",
        "hl-punctuation-delimiter",
        "hl-punctuation-special",
        "hl-operator",
        "hl-attribute",
        "hl-label",
        "hl-namespace",
        "hl-constructor",
        "hl-special",
        "hl-tag",
        "hl-tag-attribute",
        "hl-tag-delimiter",
        "hl-markup-bold",
        "hl-markup-italic",
        "hl-markup-strikethrough",
        "hl-markup-heading",
        "hl-markup-link-text",
        "hl-markup-link-url",
        "hl-markup-list",
        "hl-markup-quote",
        "hl-markup-raw",
    ]
});

/// Resolve a scope name to a Highlight, with hierarchical fallback.
/// E.g., "keyword.control.conditional" falls back to "keyword.control" then "keyword".
fn resolve_scope(scope: &str) -> Option<Highlight> {
    let mut s = scope;
    loop {
        if let Some(&highlight) = SCOPE_MAP.get(s) {
            return Some(highlight);
        }
        // Try parent scope
        match s.rfind('.') {
            Some(idx) => s = &s[..idx],
            None => return None,
        }
    }
}

/// Convert a Highlight to a CSS class name.
fn scope_to_class(highlight: Highlight) -> &'static str {
    SCOPE_CLASSES
        .get(highlight.idx())
        .copied()
        .unwrap_or("hl-unknown")
}

/// Language loader for sukr.
struct SukrLoader {
    configs: HashMap<Language, LanguageConfig>,
    name_to_lang: HashMap<String, Language>,
}

impl SukrLoader {
    fn new() -> Self {
        let mut configs = HashMap::new();
        let mut name_to_lang = HashMap::new();

        // Register all language names
        for (names, lang) in [
            (vec!["bash", "sh", "shell", "zsh"], Language::Bash),
            (vec!["c"], Language::C),
            (vec!["css"], Language::Css),
            (vec!["go", "golang"], Language::Go),
            (vec!["html"], Language::Html),
            (vec!["javascript", "js"], Language::JavaScript),
            (vec!["json"], Language::Json),
            (vec!["markdown", "md"], Language::Markdown),
            (vec!["nix"], Language::Nix),
            (vec!["python", "py"], Language::Python),
            (vec!["rust", "rs"], Language::Rust),
            (vec!["toml"], Language::Toml),
            (vec!["typescript", "ts", "tsx"], Language::TypeScript),
            (vec!["yaml", "yml"], Language::Yaml),
        ] {
            for name in names {
                name_to_lang.insert(name.to_string(), lang);
            }
        }

        // Create configs for each language
        // Each grammar is converted using TryFrom<LanguageFn> for Grammar
        if let Ok(grammar) = Grammar::try_from(tree_sitter_bash::LANGUAGE)
            && let Some(config) = make_config(
                grammar,
                include_str!("../queries/bash/highlights.scm"),
                include_str!("../queries/bash/injections.scm"),
                "",
            )
        {
            config.configure(resolve_scope);
            configs.insert(Language::Bash, config);
        }

        if let Ok(grammar) = Grammar::try_from(tree_sitter_c::LANGUAGE)
            && let Some(config) = make_config(
                grammar,
                include_str!("../queries/c/highlights.scm"),
                include_str!("../queries/c/injections.scm"),
                include_str!("../queries/c/locals.scm"),
            )
        {
            config.configure(resolve_scope);
            configs.insert(Language::C, config);
        }

        if let Ok(grammar) = Grammar::try_from(tree_sitter_css::LANGUAGE)
            && let Some(config) = make_config(
                grammar,
                include_str!("../queries/css/highlights.scm"),
                include_str!("../queries/css/injections.scm"),
                "",
            )
        {
            config.configure(resolve_scope);
            configs.insert(Language::Css, config);
        }

        if let Ok(grammar) = Grammar::try_from(tree_sitter_go::LANGUAGE)
            && let Some(config) = make_config(
                grammar,
                include_str!("../queries/go/highlights.scm"),
                include_str!("../queries/go/injections.scm"),
                include_str!("../queries/go/locals.scm"),
            )
        {
            config.configure(resolve_scope);
            configs.insert(Language::Go, config);
        }

        if let Ok(grammar) = Grammar::try_from(tree_sitter_html::LANGUAGE)
            && let Some(config) = make_config(
                grammar,
                include_str!("../queries/html/highlights.scm"),
                include_str!("../queries/html/injections.scm"),
                "",
            )
        {
            config.configure(resolve_scope);
            configs.insert(Language::Html, config);
        }

        // JavaScript needs combined queries from ecma + _javascript
        let js_highlights = [
            include_str!("../queries/ecma/highlights.scm"),
            include_str!("../queries/_javascript/highlights.scm"),
        ]
        .join("\n");
        let js_locals = [
            include_str!("../queries/ecma/locals.scm"),
            include_str!("../queries/_javascript/locals.scm"),
        ]
        .join("\n");

        if let Ok(grammar) = Grammar::try_from(tree_sitter_javascript::LANGUAGE)
            && let Some(config) = make_config(
                grammar,
                &js_highlights,
                include_str!("../queries/ecma/injections.scm"),
                &js_locals,
            )
        {
            config.configure(resolve_scope);
            configs.insert(Language::JavaScript, config);
        }

        if let Ok(grammar) = Grammar::try_from(tree_sitter_json::LANGUAGE)
            && let Some(config) = make_config(
                grammar,
                include_str!("../queries/json/highlights.scm"),
                "",
                "",
            )
        {
            config.configure(resolve_scope);
            configs.insert(Language::Json, config);
        }

        if let Ok(grammar) = Grammar::try_from(tree_sitter_md::LANGUAGE)
            && let Some(config) = make_config(
                grammar,
                include_str!("../queries/markdown/highlights.scm"),
                include_str!("../queries/markdown/injections.scm"),
                "",
            )
        {
            config.configure(resolve_scope);
            configs.insert(Language::Markdown, config);
        }

        if let Ok(grammar) = Grammar::try_from(tree_sitter_nix::LANGUAGE)
            && let Some(config) = make_config(
                grammar,
                include_str!("../queries/nix/highlights.scm"),
                include_str!("../queries/nix/injections.scm"),
                "",
            )
        {
            config.configure(resolve_scope);
            configs.insert(Language::Nix, config);
        }

        if let Ok(grammar) = Grammar::try_from(tree_sitter_python::LANGUAGE)
            && let Some(config) = make_config(
                grammar,
                include_str!("../queries/python/highlights.scm"),
                include_str!("../queries/python/injections.scm"),
                include_str!("../queries/python/locals.scm"),
            )
        {
            config.configure(resolve_scope);
            configs.insert(Language::Python, config);
        }

        if let Ok(grammar) = Grammar::try_from(tree_sitter_rust::LANGUAGE)
            && let Some(config) = make_config(
                grammar,
                include_str!("../queries/rust/highlights.scm"),
                include_str!("../queries/rust/injections.scm"),
                include_str!("../queries/rust/locals.scm"),
            )
        {
            config.configure(resolve_scope);
            configs.insert(Language::Rust, config);
        }

        if let Ok(grammar) = Grammar::try_from(tree_sitter_toml_ng::LANGUAGE)
            && let Some(config) = make_config(
                grammar,
                include_str!("../queries/toml/highlights.scm"),
                include_str!("../queries/toml/injections.scm"),
                "",
            )
        {
            config.configure(resolve_scope);
            configs.insert(Language::Toml, config);
        }

        // TypeScript needs combined queries from ecma + _typescript
        let ts_highlights = [
            include_str!("../queries/ecma/highlights.scm"),
            include_str!("../queries/_typescript/highlights.scm"),
        ]
        .join("\n");
        let ts_locals = [
            include_str!("../queries/ecma/locals.scm"),
            include_str!("../queries/_typescript/locals.scm"),
        ]
        .join("\n");

        if let Ok(grammar) = Grammar::try_from(tree_sitter_typescript::LANGUAGE_TYPESCRIPT)
            && let Some(config) = make_config(
                grammar,
                &ts_highlights,
                include_str!("../queries/ecma/injections.scm"),
                &ts_locals,
            )
        {
            config.configure(resolve_scope);
            configs.insert(Language::TypeScript, config);
        }

        if let Ok(grammar) = Grammar::try_from(tree_sitter_yaml::LANGUAGE)
            && let Some(config) = make_config(
                grammar,
                include_str!("../queries/yaml/highlights.scm"),
                include_str!("../queries/yaml/injections.scm"),
                "",
            )
        {
            config.configure(resolve_scope);
            configs.insert(Language::Yaml, config);
        }

        Self {
            configs,
            name_to_lang,
        }
    }
}

impl LanguageLoader for SukrLoader {
    fn language_for_marker(&self, marker: InjectionLanguageMarker) -> Option<THLanguage> {
        let name: Cow<'_, str> = match marker {
            InjectionLanguageMarker::Name(name) => name.into(),
            InjectionLanguageMarker::Match(text) => text.into(),
            InjectionLanguageMarker::Filename(_) | InjectionLanguageMarker::Shebang(_) => {
                return None;
            },
        };
        self.name_to_lang
            .get(name.to_lowercase().as_str())
            .map(|lang| lang.to_th_language())
    }

    fn get_config(&self, lang: THLanguage) -> Option<&LanguageConfig> {
        Language::from_th_language(lang).and_then(|l| self.configs.get(&l))
    }
}

/// Global loader instance.
static LOADER: LazyLock<SukrLoader> = LazyLock::new(SukrLoader::new);

/// Highlight source code and return HTML with span elements.
///
/// Uses tree-house with injection support for embedded languages
/// in Nix, HTML, JavaScript, and Markdown code blocks.
pub fn highlight_code(lang: Language, source: &str) -> String {
    let loader = &*LOADER;

    // Check if we have a config for this language
    if !loader.configs.contains_key(&lang) {
        return code_escape(source);
    }

    // Parse the syntax tree
    let rope = RopeSlice::from(source);
    let syntax = match Syntax::new(rope, lang.to_th_language(), Duration::from_secs(5), loader) {
        Ok(s) => s,
        Err(_) => return code_escape(source),
    };

    // Create highlighter and render
    let highlighter = Highlighter::new(&syntax, rope, loader, ..);
    render_html(source, highlighter)
}

/// Render highlighted source to HTML.
fn render_html<'a>(source: &str, mut highlighter: Highlighter<'a, 'a, SukrLoader>) -> String {
    let mut html = String::with_capacity(source.len() * 2);
    let mut pos = 0u32;
    let source_len = source.len() as u32;

    loop {
        let next_pos = highlighter.next_event_offset().min(source_len);

        // Output text between current position and next event
        if next_pos > pos {
            let start = pos as usize;
            let end = next_pos as usize;
            if start < source.len() {
                let text = &source[start..end.min(source.len())];
                code_escape_into(&mut html, text);
            }
        }

        if next_pos >= source_len {
            break;
        }

        pos = next_pos;
        let (event, highlights) = highlighter.advance();

        // Handle highlight events
        match event {
            HighlightEvent::Refresh | HighlightEvent::Push => {
                // Open spans for active highlights (use the most specific one)
                if highlights.len() > 0
                    && let Some(highlight) = highlights.into_iter().next_back()
                {
                    let class = scope_to_class(highlight);
                    html.push_str("<span class=\"");
                    html.push_str(class);
                    html.push_str("\">");
                }
            },
        }
    }

    html
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_from_fence() {
        assert_eq!(Language::from_fence("rust"), Some(Language::Rust));
        assert_eq!(Language::from_fence("rs"), Some(Language::Rust));
        assert_eq!(Language::from_fence("bash"), Some(Language::Bash));
        assert_eq!(Language::from_fence("sh"), Some(Language::Bash));
        assert_eq!(Language::from_fence("json"), Some(Language::Json));
        assert_eq!(Language::from_fence("nix"), Some(Language::Nix));
        assert_eq!(Language::from_fence("python"), Some(Language::Python));
        assert_eq!(Language::from_fence("py"), Some(Language::Python));
        assert_eq!(
            Language::from_fence("javascript"),
            Some(Language::JavaScript)
        );
        assert_eq!(Language::from_fence("js"), Some(Language::JavaScript));
        assert_eq!(
            Language::from_fence("typescript"),
            Some(Language::TypeScript)
        );
        assert_eq!(Language::from_fence("ts"), Some(Language::TypeScript));
        assert_eq!(Language::from_fence("tsx"), Some(Language::TypeScript));
        assert_eq!(Language::from_fence("go"), Some(Language::Go));
        assert_eq!(Language::from_fence("golang"), Some(Language::Go));
        assert_eq!(Language::from_fence("c"), Some(Language::C));
        assert_eq!(Language::from_fence("yaml"), Some(Language::Yaml));
        assert_eq!(Language::from_fence("yml"), Some(Language::Yaml));
        assert_eq!(Language::from_fence("css"), Some(Language::Css));
        assert_eq!(Language::from_fence("html"), Some(Language::Html));
        assert_eq!(Language::from_fence("unknown"), None);
    }

    #[test]
    fn test_scope_resolution() {
        // Exact match
        assert!(resolve_scope("keyword").is_some());
        // Hierarchical fallback
        assert!(resolve_scope("keyword.control.conditional").is_some());
        // Unknown scope
        assert!(resolve_scope("totally.unknown.scope").is_none());
    }

    #[test]
    fn test_html_escape() {
        let escaped = code_escape("<script>alert('xss')</script>");
        assert!(!escaped.contains('<'));
        assert!(escaped.contains("&lt;"));
    }

    #[test]
    fn test_highlight_rust_code() {
        let code = "fn main() { println!(\"hello\"); }";
        let html = highlight_code(Language::Rust, code);

        // Should contain some content
        assert!(html.contains("fn") || html.contains("main"));
    }

    #[test]
    fn test_highlight_nix_code() {
        let code = "{ pkgs, ... }: { environment.systemPackages = [ pkgs.vim ]; }";
        let html = highlight_code(Language::Nix, code);

        assert!(html.contains("pkgs"));
    }

    // === Hierarchical Scope Tests ===

    #[test]
    fn test_scope_to_class_generates_hierarchical_names() {
        // Verify the SCOPE_CLASSES static contains hierarchical class names
        let classes = SCOPE_CLASSES.clone();

        // Should have keyword hierarchy
        assert!(classes.contains(&"hl-keyword"));
        assert!(classes.contains(&"hl-keyword-control"));
        assert!(classes.contains(&"hl-keyword-control-return"));

        // Should have function hierarchy
        assert!(classes.contains(&"hl-function"));
        assert!(classes.contains(&"hl-function-builtin"));

        // Should have comment hierarchy
        assert!(classes.contains(&"hl-comment"));
        assert!(classes.contains(&"hl-comment-block-documentation"));
    }

    #[test]
    fn test_hierarchical_scope_resolution_fallback() {
        // Direct match should work
        let kw = resolve_scope("keyword");
        assert!(kw.is_some());

        // Hierarchical match should find parent
        let kw_ctrl = resolve_scope("keyword.control");
        assert!(kw_ctrl.is_some());

        // Deeper hierarchy should find closest ancestor
        let kw_ctrl_ret = resolve_scope("keyword.control.return");
        assert!(kw_ctrl_ret.is_some());

        // Completely unknown should return None
        assert!(resolve_scope("nonexistent.scope.here").is_none());
    }

    #[test]
    fn test_highlight_generates_hl_prefixed_classes() {
        // Rust code that should produce keyword highlighting
        let code = "fn main() { return 42; }";
        let html = highlight_code(Language::Rust, code);

        // Should contain hl-prefixed span classes
        assert!(
            html.contains("hl-"),
            "Expected hl-prefixed classes in: {html}"
        );
    }

    #[test]
    fn test_highlight_rust_keywords() {
        let code = "pub fn foo() -> Result<(), Error> { Ok(()) }";
        let html = highlight_code(Language::Rust, code);

        // Should contain span elements
        assert!(html.contains("<span"));
        // Should have keyword classes (fn, pub, etc.)
        assert!(html.contains("hl-keyword") || html.contains("class="));
    }

    #[test]
    fn test_highlight_python_function_definition() {
        let code = "def greet(name: str) -> str:\n    return f'Hello, {name}'";
        let html = highlight_code(Language::Python, code);

        // Should contain the function name
        assert!(html.contains("greet"));
        // Should have span highlighting
        assert!(html.contains("<span"));
    }

    // === Injection Tests ===

    #[test]
    fn test_injection_nix_with_bash() {
        // Nix code with bash in multi-line strings (common pattern)
        let code = r#"
stdenv.mkDerivation {
  buildPhase = ''
    echo "Building..."
    make -j$NIX_BUILD_CORES
  '';
}
"#;
        let html = highlight_code(Language::Nix, code);

        // Should produce HTML output with the nix content
        assert!(html.contains("stdenv"));
        assert!(html.contains("mkDerivation"));
        // The injection should be handled (even if bash isn't fully highlighted, should not error)
        assert!(html.contains("echo"));
    }

    #[test]
    fn test_injection_markdown_with_fenced_code() {
        // Markdown with a fenced code block
        let code = r#"# Header

Here is some code:

```rust
fn main() {}
```
"#;
        let html = highlight_code(Language::Markdown, code);

        // Should handle the markdown content
        assert!(html.contains("Header"));
        // Fenced code block should be present (may have spans inside)
        assert!(html.contains("fn") && html.contains("main"));
    }

    #[test]
    fn test_injection_html_with_script() {
        // HTML with embedded JavaScript
        let code = r#"
<!DOCTYPE html>
<html>
<head>
  <script>
    const x = 42;
    console.log(x);
  </script>
</head>
</html>
"#;
        let html = highlight_code(Language::Html, code);

        // Should contain HTML structure
        assert!(html.contains("html"));
        assert!(html.contains("script"));
        // JavaScript content should be present
        assert!(html.contains("const"));
    }

    #[test]
    fn test_injection_html_with_style() {
        // HTML with embedded CSS
        let code = r#"
<html>
<head>
  <style>
    .container { display: flex; }
  </style>
</head>
</html>
"#;
        let html = highlight_code(Language::Html, code);

        // Should handle CSS injection
        assert!(html.contains("style"));
        assert!(html.contains("container"));
        assert!(html.contains("flex"));
    }

    // === Edge Cases ===

    #[test]
    fn test_empty_input() {
        let html = highlight_code(Language::Rust, "");
        // Empty input should produce minimal output
        assert!(html.is_empty() || html.len() < 10);
    }

    #[test]
    fn test_whitespace_only_input() {
        let html = highlight_code(Language::Rust, "   \n\t\n  ");
        // Whitespace should be preserved
        assert!(html.contains(' ') || html.contains('\n'));
    }

    #[test]
    fn test_special_characters_escaped() {
        let code = r#"let x = "<script>alert('xss')</script>";"#;
        let html = highlight_code(Language::Rust, code);

        // HTML special chars should be escaped
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;") || html.contains("script"));
    }
}
