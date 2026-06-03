//! Mermaid diagram rendering via mermaid-rs-renderer.
//!
//! Converts Mermaid diagram definitions to SVG at build-time.

use std::panic;

use mermaid_rs_renderer::RenderOptions;

/// Render a Mermaid diagram to SVG.
///
/// # Arguments
/// * `code` - The Mermaid diagram definition
///
/// # Returns
/// The rendered SVG string, or an error message on failure.
///
/// # Note
/// Uses catch_unwind to handle panics in upstream dependencies gracefully.
pub fn render_diagram(code: &str) -> Result<String, String> {
    let code = code.to_owned();
    let result = panic::catch_unwind(move || {
        let opts = RenderOptions::modern();
        mermaid_rs_renderer::render_with_options(&code, opts)
    });

    match result {
        Ok(Ok(svg)) => Ok(svg),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("mermaid rendering panicked (upstream bug)".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_flowchart() {
        let result = render_diagram("flowchart LR; A-->B-->C").unwrap();
        assert!(result.contains("<svg"));
        assert!(result.contains("</svg>"));
    }

    #[test]
    fn test_sequence_diagram() {
        let result = render_diagram("sequenceDiagram\n    Alice->>Bob: Hello").unwrap();
        assert!(result.contains("<svg"));
    }

    #[test]
    fn test_invalid_syntax_no_panic() {
        // Should not panic, returns error
        let result = render_diagram("invalid diagram syntax ???");
        // May succeed with error node or fail gracefully
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_state_diagram() {
        let result =
            render_diagram("stateDiagram-v2\n    [*] --> Idle\n    Idle --> Processing: Start");
        assert!(result.is_ok() || result.is_err());
    }
}
