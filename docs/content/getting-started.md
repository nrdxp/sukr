+++
title = "Getting Started"
description = "Install sukr and build your first site"
weight = 0
+++

This guide walks you through installing sukr and creating your first static site.

## Installation

### From source (recommended)

```bash
git clone https://github.com/nrdxp/sukr
cd sukr
cargo install --path .
```

### With Nix

Run directly:

```bash
nix run github:nrdxp/sukr -- --help
```

Or install with Nix:

```bash
nix profile install github:nrdxp/sukr
```

## Create Your First Site

### 1. Create directory structure

```bash
mkdir my-site && cd my-site
mkdir -p content templates static
```

### 2. Create configuration

Create `site.toml`:

```toml
title    = "My Site"
author   = "Your Name"
base_url = "https://example.com"
```

### 3. Create homepage

Create `content/_index.md`:

```markdown
+++
title = "Welcome"
description = "My awesome site"
+++

# Hello, World!

This is my site built with sukr.
```

### 4. Create templates

Create `templates/base.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>{{ title }} | {{ config.title }}</title>
    <link rel="stylesheet" href="{{ prefix }}/style.css" />
  </head>
  <body>
    <main>{% block content %}{% endblock content %}</main>
  </body>
</html>
```

Create `templates/page.html`:

```html
{% extends "base.html" %} {% block content %}
<article>
  <h1>{{ page.title }}</h1>
  {{ content | safe }}
</article>
{% endblock content %}
```

Create `templates/content/default.html`:

```html
{% extends "base.html" %} {% block content %}
<article>
  <h1>{{ page.title }}</h1>
  {{ content | safe }}
</article>
{% endblock content %}
```

Your templates directory should look like this:

```text
templates/
├── base.html
├── page.html
└── content/
    └── default.html
```

### 5. Build

```bash
sukr
```

### 6. View your site

Open `public/index.html` in your browser. You should see your "Hello, World!" page rendered with the template you created.

## Next Steps

- [Deployment](deployment.html) — put your site on the web
- [Configuration](configuration.html) — customize `site.toml` options (paths, navigation, base URL)
- [Content Organization](content-organization.html) — learn how directories map to site sections
- [Features](features/index.html) — syntax highlighting, math, diagrams, and more
