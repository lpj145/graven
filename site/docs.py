"""Render docs/*.md into the site's documentation pages (target/site/docs/).

The pages share the landing page's header, typography and colors. Markdown is rendered
with markdown-it-py and code is highlighted with Pygments (see site/requirements.txt).
Headings get GitHub-style ids, so links like `spec/graven-format-v1.md#9-schema-compatibility`
work both on GitHub and on the site.
"""
import html
import os
import re

from markdown_it import MarkdownIt
from pygments import highlight
from pygments.formatters import HtmlFormatter
from pygments.lexer import RegexLexer, bygroups, words
from pygments.lexers import get_lexer_by_name
from pygments.token import Comment, Keyword, Name, Number, Operator, Punctuation, String, Text, Whitespace
from pygments.util import ClassNotFound

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DOCS = os.path.join(ROOT, "docs")

# (title in the sidebar, source under docs/, one-line description for the index)
PAGES = [
    ("Getting started", "getting-started.md", "Install gravenc, create a schema and lockfile, and wire up code generation (including Rust build.rs)."),
    ("Coming from JSON", "coming-from-json.md", "Translate JSON data into a Graven schema, convert existing files, and choose how to store arrays."),
    ("Good practice", "good-practice.md", "Advice for designing schemas, storing collections and reading untrusted data, with the reason behind each tip."),
    ("Specification", "spec/graven-format-v1.md", "The normative wire format: layout, presence, verification, envelope, compatibility, conformance."),
    ("RPC frames", "spec/graven-rpc-v1.md", "The normative bytes of service calls and replies: frame header, kinds, reply sequences, protocol faults."),
    ("Binary layout", "binary-layout.md", "Every kind of value on the wire, byte by byte, with real messages encoded by the reference implementation."),
    ("Schema language", "graven-language.md", "The .graven language, evolution rules, the compiler workflow and the generated API."),
    ("JavaScript and TypeScript", "javascript.md", "gravenc gen --lang js: JavaScript with .d.ts declarations, for browsers, Node and Deno; validation and RPC clients."),
    ("Validation", "validation.md", "Built-in rules (range, len, finite, known), custom checks, evolution and generated validation API."),
    ("Tools", "tools.md", "gravenc: check, update, diff, inspect, JSON conversion and log scanning; editor support."),
    ("Conformance vectors", "conformance-vectors.md", "The 42 frozen vectors and how to test an implementation against them."),
    ("Benchmarks", "benchmarks.md", "The reference implementation against FlatBuffers and Cap'n Proto: size, allocations and time on one machine."),
    ("Testing", "testing.md", "How the reference implementation is tested: suite, fuzzing and Miri."),
    ("The future", "future.md", "Where Graven can be applied, what already works for each area, and what is still to be built."),
    ("For AI assistants", "for-ai.md", "A compact guide for language models, plus llms.txt and llms-full.txt for tools that load documentation."),
    ("History", "history/index.md", "How we got here: the designs, plans and benchmarks that led to the format, in order (not normative)."),
    ("Changelog", "changelog.md", "Release history."),
]


class GravenLexer(RegexLexer):
    name = "Graven"
    aliases = ["graven"]
    filenames = ["*.graven"]
    tokens = {
        "root": [
            (r"\s+", Whitespace),
            (r"///(?!/).*?$", String.Doc),
            (r"//.*?$", Comment.Single),
            (r"/\*", Comment.Multiline, "comment"),
            (r"(namespace)(\s+)([\w.]+)", bygroups(Keyword.Namespace, Whitespace, Name.Namespace)),
            (r"(import)(\s+)([\w.]+)", bygroups(Keyword.Namespace, Whitespace, Name.Namespace)),
            (r"\bas\b", Keyword.Namespace),
            (r"(enum|struct|table|union)(\s+)(\w+)", bygroups(Keyword.Declaration, Whitespace, Name.Class)),
            (r"#\[[^\]]*\]", Name.Decorator),
            (r"\breserved\b", Keyword),
            (r"@\d+", Name.Label),
            (words(("u8", "u16", "u32", "u64", "u128", "i8", "i16", "i32", "i64", "i128", "f32", "f64", "bool", "string", "bytes"), suffix=r"\b"), Keyword.Type),
            (r"\b(true|false)\b", Keyword.Constant),
            (r"-?0x[0-9A-Fa-f_]+", Number.Hex),
            (r"-?\d[\d_]*(\.[\d_]+)?([eE][+-]?\d+)?", Number),
            (r"\b[A-Z]\w*", Name.Class),
            (r"\w+", Name),
            (r"[?=]", Operator),
            (r"[{}\[\]:;,]", Punctuation),
            (r".", Text),
        ],
        "comment": [(r"[^*]+", Comment.Multiline), (r"\*/", Comment.Multiline, "#pop"), (r"\*", Comment.Multiline)],
    }


FORMATTER = HtmlFormatter(nowrap=True)


def highlight_code(code, lang, _attrs):
    lang = (lang or "").strip().split()[0] if lang else ""
    lexer = None
    if lang == "graven":
        lexer = GravenLexer()
    elif lang and lang not in ("text", "txt"):
        try:
            lexer = get_lexer_by_name({"sh": "bash", "shell": "bash"}.get(lang, lang))
        except ClassNotFound:
            lexer = None
    body = highlight(code, lexer, FORMATTER) if lexer else html.escape(code)
    cls = f' class="language-{html.escape(lang)}"' if lang else ""
    return f'<pre class="code"><code{cls}>{body}</code></pre>\n'


MD = MarkdownIt("commonmark", {"html": True, "highlight": highlight_code}).enable("table")

INCLUDE = re.compile(r"\{\{#include ([^}:]+)(?::([\w-]+))?\}\}")


def expand_includes(text, base):
    def repl(m):
        path = os.path.normpath(os.path.join(base, m.group(1).strip()))
        src = open(path, encoding="utf8").read()
        anchor = m.group(2)
        if anchor:
            start = src.index(f"<!-- ANCHOR: {anchor} -->") + len(f"<!-- ANCHOR: {anchor} -->")
            end_tag = f"<!-- ANCHOR_END: {anchor} -->"
            end = src.index(end_tag) if end_tag in src else len(src)
            src = src[start:end]
        return src.strip("\n")

    return INCLUDE.sub(repl, text)


def slugify(text):
    s = text.strip().lower()
    s = re.sub(r"[^\w\- ]", "", s)
    return s.replace(" ", "-")


def heading_ids(tokens):
    """GitHub-style ids on every heading; returns [(level, id, text)] for h2/h3."""
    seen = {}
    toc = []
    for i, t in enumerate(tokens):
        if t.type != "heading_open":
            continue
        inline = tokens[i + 1]
        text = "".join(c.content for c in inline.children or [] if c.type in ("text", "code_inline"))
        base = slugify(text)
        n = seen.get(base, 0)
        seen[base] = n + 1
        hid = base if n == 0 else f"{base}-{n}"
        t.attrSet("id", hid)
        if t.tag in ("h2", "h3"):
            toc.append((int(t.tag[1]), hid, text))
    return toc


SPECIAL_LINKS = {
    "README.md": "{root}index.html",
    "CHANGELOG.md": "{root}docs/changelog.html",
    "testvectors/v1/README.md": "{root}docs/conformance-vectors.html",
}


def rewrite_links(body, src_rel, root):
    """Links between docs .md files become .html; links to repository files outside docs/
    that have a page get that page."""
    here = os.path.dirname(os.path.join("docs", src_rel))

    def repl(m):
        href = m.group(1)
        if re.match(r"^[a-z]+:", href) or href.startswith("#"):
            return m.group(0)
        path, _, frag = href.partition("#")
        frag = f"#{frag}" if frag else ""
        target = os.path.normpath(os.path.join(here, path))
        # Content included from the repository root (README, CHANGELOG) links to `docs/…`.
        if path.startswith("docs/") and path.endswith(".md"):
            return f'href="{root}{path[:-3]}.html{frag}"'
        if target in SPECIAL_LINKS:
            return f'href="{SPECIAL_LINKS[target].format(root=root)}{frag}"'
        if path.endswith(".md"):
            return f'href="{path[:-3]}.html{frag}"'
        return m.group(0)

    return re.sub(r'href="([^"]+)"', repl, body)


def nav_html(root, current):
    cur = ' aria-current="page"' if current is None else ""
    items = [f'<li><a href="{root}docs/index.html"{cur}>Overview</a></li>']
    for title, src, _ in PAGES:
        href = root + "docs/" + src[:-3] + ".html"
        cur = ' aria-current="page"' if src == current else ""
        items.append(f'<li><a href="{href}"{cur}>{html.escape(title)}</a></li>')
    return "\n".join(items)


def toc_html(toc):
    if len(toc) < 3:
        return ""
    items = "\n".join(
        f'<li class="l{lvl}"><a href="#{hid}">{html.escape(text)}</a></li>' for lvl, hid, text in toc if lvl == 2
    )
    return f'<nav class="toc" aria-label="On this page"><p class="toc-title">On this page</p><ul>{items}</ul></nav>'


PAGE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} — Graven</title>
<meta name="description" content="{desc}">
<link rel="icon" href="{root}assets/favicon.svg" type="image/svg+xml">
<link rel="icon" href="{root}assets/favicon.ico" sizes="48x48">
<link rel="apple-touch-icon" href="{root}assets/apple-touch-icon.png">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500&family=IBM+Plex+Sans:ital,wght@0,400;0,500;0,600;1,400&display=swap" rel="stylesheet">
<link rel="stylesheet" href="{root}assets/site.css">
</head>
<body class="docs-page">
<a class="skip" href="#doc">Skip to content</a>

<header class="top">
  <div class="wrap top-inner">
    <a class="wordmark" href="{root}index.html" aria-label="Graven home"><svg class="mark" viewBox="0 0 32 32" aria-hidden="true"><rect class="mk-bg" width="32" height="32" rx="7"/><rect class="mk-l" x="7" y="8.5" width="18" height="3.5" rx="1.75"/><rect class="mk-l" x="7" y="14.25" width="13" height="3.5" rx="1.75"/><rect class="mk-a" x="7" y="20" width="8" height="3.5" rx="1.75"/></svg>Graven</a>
    <nav aria-label="Main">
      <a href="{root}docs/index.html" aria-current="page">Docs</a>
      <a href="https://github.com/lpj145/graven">GitHub</a>
    </nav>
  </div>
</header>

<div class="wrap docs-grid">
  <aside class="docs-nav">
    <details class="docs-menu" open>
      <summary>Documentation</summary>
      <ul>
{nav}
      </ul>
    </details>
  </aside>
  <main id="doc" class="doc">
{body}
  </main>
  <aside class="docs-aside">
{toc}
  </aside>
</div>

<footer class="foot">
  <div class="wrap foot-inner">
    <p>Graven · format v1 · MIT license</p>
    <nav aria-label="Footer">
      <a href="{root}docs/spec/graven-format-v1.html">Specification</a>
      <a href="{root}docs/graven-language.html">Schema language</a>
      <a href="{root}docs/tools.html">Tools</a>
      <a href="{root}docs/conformance-vectors.html">Conformance vectors</a>
      <a href="{root}api/graven/index.html">Rust API</a>
      <a href="{root}docs/changelog.html">Changelog</a>
      <a href="https://github.com/lpj145/graven">GitHub</a>
    </nav>
  </div>
</footer>
<script>
  // On narrow screens the documentation menu starts closed.
  if (window.matchMedia("(max-width: 960px)").matches) {{
    document.querySelectorAll(".docs-menu").forEach((d) => d.removeAttribute("open"));
  }}
  // On wide screens the menu is always shown: its title is a label, not a toggle.
  document.querySelectorAll(".docs-menu summary").forEach((s) =>
    s.addEventListener("click", (e) => {{
      if (!window.matchMedia("(max-width: 960px)").matches) e.preventDefault();
    }})
  );
</script>
</body>
</html>
"""


def render_page(src_rel, out_dir, title, desc):
    text = open(os.path.join(DOCS, src_rel), encoding="utf8").read()
    text = expand_includes(text, os.path.dirname(os.path.join(DOCS, src_rel)))
    # Every page is also published as Markdown next to its HTML, for tools and models.
    md_out = os.path.join(out_dir, "docs", src_rel)
    os.makedirs(os.path.dirname(md_out), exist_ok=True)
    open(md_out, "w", encoding="utf8").write(text)
    tokens = MD.parse(text)
    toc = heading_ids(tokens)
    body = MD.renderer.render(tokens, MD.options, {})
    depth = src_rel.count("/") + 1
    root = "../" * depth
    body = rewrite_links(body, src_rel, root)
    # Spec references to the conformance vectors read as quiet margin notes.
    body = re.sub(r"<p>\[(vectors?): ", r'<p class="ref">[\1: ', body)
    # Tables scroll on narrow screens instead of widening the page.
    body = body.replace("<table>", '<div class="table-scroll"><table>').replace("</table>", "</table></div>")
    page = PAGE.format(
        title=html.escape(title), desc=html.escape(desc), root=root, nav=nav_html(root, src_rel), body=body, toc=toc_html(toc)
    )
    out = os.path.join(out_dir, "docs", src_rel[:-3] + ".html")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    open(out, "w", encoding="utf8").write(page)


def index_page(out_dir):
    items = "\n".join(
        f'<div><dt><a href="{src[:-3]}.html">{html.escape(t)}</a></dt><dd>{html.escape(d)}</dd></div>' for t, src, d in PAGES
    )
    body = f"""<h1 id="documentation">Documentation</h1>
<p class="lede-doc">The specification is the normative text; everything else explains how to use it and how the
reference implementation is built and tested.</p>
<dl class="doc-index">
{items}
</dl>"""
    page = PAGE.format(
        title="Documentation", desc="Graven documentation", root="../", nav=nav_html("../", None), body=body, toc="",
    )
    open(os.path.join(out_dir, "docs", "index.html"), "w", encoding="utf8").write(page)


def build(out_dir):
    for title, src, desc in PAGES:
        render_page(src, out_dir, title, desc)
    # Design proposals: published, linked from the pages that cite them, not in the sidebar.
    for name in sorted(os.listdir(os.path.join(DOCS, "proposals"))):
        if name.endswith(".md"):
            first = open(os.path.join(DOCS, "proposals", name), encoding="utf8").readline().lstrip("# ").strip()
            render_page(f"proposals/{name}", out_dir, first, first)
    # History documents: published, linked from history/index.md, not in the sidebar.
    for sub in ("design", "plans", "benchmarks"):
        for name in sorted(os.listdir(os.path.join(DOCS, "history", sub))):
            if name.endswith(".md"):
                first = open(os.path.join(DOCS, "history", sub, name), encoding="utf8").readline().lstrip("# ").strip()
                render_page(f"history/{sub}/{name}", out_dir, first, first)
    index_page(out_dir)
    llms_files(out_dir)
    return [src for _, src, _ in PAGES]


REPO = "https://github.com/lpj145/graven"

LLMS_SUMMARY = (
    "Graven is an open specification for a zero-copy binary serialization format and its schema "
    "language (.graven), with a compiler (gravenc), lockfiles that refuse incompatible schema changes, "
    "frozen conformance vectors and a reference implementation in Rust."
)


def llms_files(out_dir):
    """llms.txt (an index, https://llmstxt.org) and llms-full.txt (every page in one file)."""
    lines = ["# Graven", "", f"> {LLMS_SUMMARY}", "",
             f"Source code: {REPO}. Start with the guide for AI assistants: it has the schema syntax, the rules gravenc enforces, "
             "the commands and the Rust API in one page. Every page below is Markdown.", "", "## Docs", ""]
    for title, src, desc in sorted(PAGES, key=lambda p: p[1] != "for-ai.md"):
        if src.startswith("history/"):
            continue
        lines.append(f"- [{title}](docs/{src}): {desc}")
    lines += ["", "## Optional", "",
              "- [History](docs/history/index.md): how the format was designed and benchmarked (not normative).",
              "- [Proposal 0001: imports](docs/proposals/0001-imports.md): the design of imports between schema files.",
              "- [Complete documentation in one file](llms-full.txt)", ""]
    open(os.path.join(out_dir, "llms.txt"), "w", encoding="utf8").write("\n".join(lines))

    order = ["for-ai.md"] + [src for _, src, _ in PAGES if src not in ("for-ai.md", "history/index.md", "changelog.md")]
    parts = [f"# Graven: complete documentation\n\n> {LLMS_SUMMARY}\n"]
    for src in order:
        text = open(os.path.join(out_dir, "docs", src), encoding="utf8").read()
        parts.append(f"\n\n---\n\n<!-- source: docs/{src} -->\n\n{text.strip()}\n")
    open(os.path.join(out_dir, "llms-full.txt"), "w", encoding="utf8").write("".join(parts))
