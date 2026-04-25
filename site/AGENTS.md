# sparkid marketing site

Single-page marketing site for the sparkid library, built with Astro and deployed to Cloudflare Pages.

## Tech stack

- **Framework**: Astro (static output mode)
- **Hosting**: Cloudflare Pages (project: `sparkid-site`)
- **Domain**: sparkid.dev (configured in Cloudflare dashboard)
- **Fonts**: Bricolage Grotesque (display), DM Sans (body), JetBrains Mono (code) — loaded from Google Fonts
- **Design**: Dark theme (#09090b bg), amber accent (#f59e0b)

## Project structure

```
site/
  src/
    layouts/Layout.astro   # Global layout, CSS variables, base styles
    pages/index.astro      # Entire marketing page (single file)
  public/
    favicon.svg            # Amber spark icon
  astro.config.mjs         # Static output config
  package.json             # Scripts: dev, build, preview
```

## Development

```bash
cd site
npm install
npm run dev       # Local dev server (localhost:4321)
npm run build     # Build to dist/
npm run preview   # Preview built site locally
```

## Deployment (Wrangler CLI)

The site is deployed to Cloudflare Pages via the Wrangler CLI. Authentication is already configured.

```bash
cd site
npm run build
npx wrangler pages deploy dist --project-name sparkid-site
```

To check auth status:
```bash
npx wrangler whoami
```

Custom domain (sparkid.dev) is managed in the Cloudflare dashboard under Workers & Pages → sparkid-site → Custom domains.

## Page sections

The entire page lives in `src/pages/index.astro`:

1. **Nav** — Logo, section links, GitHub link
2. **Hero** — Headline, subtitle, install tabs (npm/pip/cargo)
3. **Anatomy** — Visual breakdown of a SparkID's 3 segments (timestamp, counter, random) with properties
4. **Comparison table** — sparkid vs UUID v4, UUID v7, nanoid, ULID
5. **Code examples** — Tabbed JS/Python/Rust snippets
6. **Features grid** — Key properties (monotonic, collision-resistant, human readable, thread-safe, zero deps, multi-language)
7. **Benchmarks** — Bar charts per language showing sparkid vs alternatives
8. **CTA** — Final call to action with install command
9. **Footer** — GitHub, npm, PyPI, crates.io links + author credit

## Code examples with curly braces

Astro parses `{}` in templates as expressions. Code snippets containing curly braces (e.g., Rust) must be defined as HTML strings in the frontmatter and rendered with `set:html`:

```astro
---
const codeRust = `<span class="kw">use</span> sparkid::SparkId;

<span class="kw">let</span> id = SparkId::new();`;
---
<pre><code set:html={codeRust} /></pre>
```

## Interactive scripts

The `<script is:inline>` tag at the bottom handles tab switching for install commands, code examples, and benchmark bar animations. Use `is:inline` (not regular `<script>`) to avoid Astro's esbuild processing, which chokes on certain patterns.

## Conventions

- **sparkid** (lowercase) = the library/package name
- **SparkID** (PascalCase) = a generated ID
- Do not deploy without explicit user approval
- Real SparkIDs are used throughout (generated from the JS package), not fake placeholders
- Benchmark data comes from `bench_compare.py` at the repo root — run once, medians parsed from output
