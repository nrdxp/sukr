+++
title = "Content Organization"
description = "How the filesystem maps to your site structure"
weight = 2
+++

sukr builds your site structure from your `content/` directory. No routing config needed — the filesystem _is_ the config.

## The Rule

```text
content/foo/bar.md  →  public/foo/bar.html
content/about.md    →  public/about.html
content/_index.md   →  public/index.html
```

That's it. Paths mirror exactly, with `.md` becoming `.html`.

## Directory Layout

```text
content/
├── _index.md           # Homepage (required)
├── _404.md             # → /404.html (custom error page, optional)
├── about.md            # → /about.html
├── contact.md          # → /contact.html
├── blog/               # Section directory
│   ├── _index.md       # → /blog/index.html (section index)
│   ├── first-post.md   # → /blog/first-post.html
│   └── second-post.md  # → /blog/second-post.html
└── projects/
    ├── _index.md       # → /projects/index.html
    └── my-app.md       # → /projects/my-app.html
```

## What Makes a Section

A section is any directory containing Markdown content files.

- **Explicit Sections**: Directories containing `_index.md` are treated as explicit sections. The `_index.md` file provides metadata (title, description), controls custom listing layouts, and displays in the navigation menus.
- **Virtual Sections**: If a subdirectory does not contain `_index.md` but contains other Markdown files, `sukr` automatically instantiates a virtual section for it. This virtual section uses default frontmatter settings, deriving its title from the directory name.

This recursive section discovery allows building deeply nested content hierarchies without the need to define boilerplate `_index.md` files at every intermediate level.

## Custom 404 Page

To add a custom "not found" page, create `content/_404.md`:

```markdown
+++
title = "Page Not Found"
+++

# Page Not Found

The page you're looking for doesn't exist or has been moved.

[Return to the homepage](/)
```

sukr renders this to `404.html` in the output root. The file is **optional** — if you don't create `_404.md`, no 404 page is generated.

Most static hosts (Cloudflare Pages, Netlify, GitHub Pages, Vercel) automatically serve `/404.html` when a visitor hits an unmatched route. No host-side configuration is needed beyond having the file present.

## Draft Mode

Set `draft = true` in frontmatter to exclude content from the build:

```toml
+++
title = "Work in Progress"
draft = true
+++
```

Drafts are filtered from **all output** — page rendering, navigation, section listings, feeds, and the sitemap. The file stays in your content directory but produces no HTML. Remove the field (or set `draft = false`) to publish.

## Aliases (Redirects)

Aliases let you redirect old URLs to a page's current location. Set `aliases` in frontmatter:

```toml
+++
title = "New Location"
aliases = ["/old/path", "/another/old/path"]
+++
```

For each alias, sukr generates an HTML redirect stub using `<meta http-equiv="refresh">`. Bare paths like `/old/path` produce `/old/path/index.html`; paths with extensions like `/old/page.html` are used as-is.

## Relative URL Prefixes

When organizing content in deep directory structures, linking to pages or assets at the root of the site (e.g., `/style.css` or `/images/logo.png`) can be error-prone when served from subdirectories.

To simplify referencing assets and links relative to the site root, `sukr` supports the `{{ prefix }}` (or `{{prefix}}`) placeholder directly in your Markdown content body. At build time, `sukr` automatically replaces this placeholder with the correct relative prefix (such as `.`, `../`, or `../../`) matching the depth of the current page.

For example, a Markdown file located at `content/blog/first-post.md` can reference root-relative links and images like so:

```markdown
[Return to Home]({{ prefix }}/index.html)
![Logo]({{ prefix }}/logo.png)
```

During build time, this gets resolved to:

```markdown
[Return to Home](../index.html)
![Logo](../logo.png)
```

## Section Discovery

sukr automatically discovers sections during the build:

1. Scans `content/` for directories containing `_index.md`
2. Collects all `.md` files in that directory (excluding `_index.md`)
3. Renders the section index template with the collected items
4. Renders individual content pages (for blog-type sections)

The **section type** determines which template renders the index. It resolves in order:

1. **Frontmatter override** — `section_type = "blog"` in the section's `_index.md`
2. **Directory name** — `content/blog/` becomes type `blog`

For the full section type reference (built-in types, frontmatter fields, and template dispatch), see [Sections](features/sections.html).

## Navigation Generation

Navigation builds automatically from:

- **Top-level `.md` files** (except `_index.md`) → page links
- **Directories with `_index.md`** → section links

Items sort by `weight` in frontmatter (lower first), then alphabetically.

```toml
+++
title = "Blog"
weight = 10  # Appears before items with weight > 10
+++
```

### Hierarchical Navigation

When `nav.nested = true` in your config, section children appear as nested sub-items:

```text
Features           ← Section link
  ├─ Templates     ← Child page
  ├─ Sections      ← Child page
  └─ Highlighting  ← Child page
Getting Started    ← Top-level page
```

Child pages inherit their parent section's position in the nav tree. Within a section, children sort by weight then alphabetically.

Without nested navigation (the default), only top-level items appear in the nav.

## URL Examples

| Source Path              | Output Path              | URL                |
| ------------------------ | ------------------------ | ------------------ |
| `content/_index.md`      | `public/index.html`      | `/`                |
| `content/_404.md`        | `public/404.html`        | `/404.html`        |
| `content/about.md`       | `public/about.html`      | `/about.html`      |
| `content/blog/_index.md` | `public/blog/index.html` | `/blog/`           |
| `content/blog/hello.md`  | `public/blog/hello.html` | `/blog/hello.html` |

## Key Points

- No config files for routing
- Directory names become URL segments
- `_index.md` = section index, not a regular page
- `_404.md` = custom error page, rendered to `404.html` at output root
- Flat output structure (no nested `index.html` per page)
