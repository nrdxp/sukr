+++
title = "Footnotes"
description = "Sequential footnotes with backlinks and screen-reader accessibility"
weight = 6
+++

sukr supports Markdown footnotes with automated sequential numbering, bidirectional navigation backlinks, support for math blocks, and screen-reader optimizations.

## Footnote Syntax

Define a footnote reference inline using `[^label]`, and specify the footnote's content anywhere else in the document with `[^label]: content`:

```markdown
Here is a footnote reference[^1], and another with a descriptive name[^named].

[^1]: This is the first footnote definition.
[^named]: This footnote has a non-numeric label but will be numbered sequentially.
```

## How It Works

1. **Sequential Numbering**: Regardless of the label used (e.g., `[^named]`), footnotes are numbered sequentially (`[1]`, `[2]`, etc.) in the order they are referenced in the document text.
2. **Footnote Layout**: Footnote definitions are collected and appended to the bottom of the rendered page, wrapped in a semantic HTML `<aside class="footnotes">` container.
3. **Bidirectional Backlinks**: Each footnote reference generates a link to its definition. The definition includes a backlink (`↩`) allowing readers to jump directly back to the reference in the text.

## Accessibility Features

`sukr` optimizes footnotes for screen readers and assistive technologies to ensure a clean Text-to-Speech (TTS) experience:

- **TTS Footnote Number Suppression**: Inline footnote numbers are rendered using the CSS `data-footnote` attribute rather than raw text nodes. This prevents TTS engines from reading out the footnote numbers mid-sentence (e.g., reading "quadratic formula one" instead of "quadratic formula").
- **Aria Labels**: Reference links include an `aria-label="Footnote N"` attribute to cleanly inform screen-reader users of a footnote reference.
- **Aside Element**: The footnote section is wrapped in a semantic `<aside>` container to mark it as auxiliary content.

To support the TTS suppression, ensure your site's stylesheet contains the following CSS rule for displaying footnote numbers:

```css
.footnote-ref a[data-footnote]::before {
  content: attr(data-footnote);
}
```

## Math Blocks in Footnotes

You can include inline or display LaTeX math expressions directly inside footnote definitions:

```markdown
Here is a footnote with math[^math-fn].

[^math-fn]: This footnote contains inline math $x^2$ and display math:

    $$
    E = mc^2
    $$
```

These math expressions are eagerly rendered to MathML at compile time, preserving the footnote structure.
