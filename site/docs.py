"""Render docs/*.md into the site's documentation pages (target/site/docs/).

Features:
- Open grouped sidebar showing all sections and SDKs directly.
- Inline synchronized multi-language code tabs (`<!-- tabs --> ... <!-- /tabs -->`).
- GitHub-style callouts (`> [!NOTE]`, `> [!TIP]`, `> [!IMPORTANT]`, `> [!WARNING]`).
- Code block headers + copy button, overview cards, and prev/next page navigation.
- Preserves clean Markdown alongside HTML (`docs/**/*.md`, `llms.txt`, `llms-full.txt`).
"""
import hashlib
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

# Sidebar sections: (Section Title, [(Page Title, Source relative to docs/, Short Description)])
SECTIONS = [
    ("Start", [
        ("Getting started", "getting-started.md", "Install gravenc, create a lockfile, and generate your first bindings."),
        ("Coming from JSON", "coming-from-json.md", "Translate JSON into a .graven schema and compare code across languages."),
        ("Good practice", "good-practice.md", "Schema evolution habits, collections, and reading untrusted input."),
    ]),
    ("Language & Format", [
        ("Schema language", "graven-language.md", "Syntax, types, IDs, imports, names, and lockfile compatibility rules."),
        ("Validation", "validation.md", "Value rules (range, len, finite, known, check) and evolution checks."),
        ("Services", "services.md", "RPC service declarations, method shapes, typed errors, and timeouts."),
        ("Format v1", "spec/graven-format-v1.md", "Normative wire format: layout, presence, verification, and envelopes."),
        ("RPC frames v1", "spec/graven-rpc-v1.md", "Normative RPC wire framing, reply sequences, and protocol faults."),
        ("Binary layout", "binary-layout.md", "Every wire value byte by byte, inspected from real encoded messages."),
        ("Conformance vectors", "conformance-vectors.md", "Frozen format and RPC frame vectors to test any implementation."),
    ]),
    ("SDKs", [
        ("Implementations", "implementations.md", "Supported languages, capability matrix, and how to add a new SDK."),
        ("Rust", "sdk/rust.md", "Reference runtime (no_std), build.rs codegen, validation, and RPC edge."),
        ("JavaScript / TypeScript", "sdk/js.md", "Zero-dep ES2022 .js + .d.ts bindings for browsers, Node, and Deno."),
    ]),
    ("Project & Tools", [
        ("gravenc CLI", "tools.md", "check, update, gen, diff, inspect, JSON conversion, scan, and editor support."),
        ("Benchmarks", "benchmarks.md", "Size, allocations, and latency vs FlatBuffers and Cap'n Proto."),
        ("Testing", "testing.md", "Rust suite, fuzzing, Miri, and the JS/TS conformance & interop suite."),
        ("The future", "future.md", "Roadmap and ecosystems where Graven fits best."),
        ("For AI assistants", "for-ai.md", "Compact single-page reference plus llms.txt and llms-full.txt."),
        ("History", "history/index.md", "Design notes, plans, and benchmark records (non-normative)."),
        ("Changelog", "changelog.md", "Release history."),
    ]),
]

PAGES = [p for _, items in SECTIONS for p in items]

# Legacy paths (without .html) -> target relative to `docs/`
REDIRECTS = {
    "javascript": "sdk/js.html",
    "typescript": "sdk/js.html",
    "rust/index": "sdk/rust.html",
    "rust/read-write": "sdk/rust.html#reading-and-writing",
    "rust/validation": "sdk/rust.html#validation",
    "rust/services": "sdk/rust.html#services",
    "rust/api": "sdk/rust.html#api-reference",
    "js/index": "sdk/js.html",
    "js/read-write": "sdk/js.html#reading-and-writing",
    "js/validation": "sdk/js.html#validation",
    "js/services": "sdk/js.html#services",
    "js/api": "sdk/js.html#api-reference-and-verification",
}


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
            (r"(enum|struct|table|union|service)(\s+)(\w+)", bygroups(Keyword.Declaration, Whitespace, Name.Class)),
            (r"#\[[^\]]*\]", Name.Decorator),
            (r"\breserved\b", Keyword),
            (r"\b(stream|Result)\b", Keyword.Type),
            (r"@\d+", Name.Label),
            (words(("u8", "u16", "u32", "u64", "u128", "i8", "i16", "i32", "i64", "i128", "f32", "f64", "bool", "string", "bytes"), suffix=r"\b"), Keyword.Type),
            (r"\b(true|false)\b", Keyword.Constant),
            (r"-?0x[0-9A-Fa-f_]+", Number.Hex),
            (r"-?\d[\d_]*(\.[\d_]+)?([eE][+-]?\d+)?", Number),
            (r"\b[A-Z]\w*", Name.Class),
            (r"\w+", Name),
            (r"[?=]|->", Operator),
            (r"[{}\[\]:;,<>()]", Punctuation),
            (r".", Text),
        ],
        "comment": [(r"[^*]+", Comment.Multiline), (r"\*/", Comment.Multiline, "#pop"), (r"\*", Comment.Multiline)],
    }


FORMATTER = HtmlFormatter(nowrap=True)


def highlight_code(code, lang, _attrs):
    lang_norm = (lang or "").strip().split()[0] if lang else ""
    lexer = None
    if lang_norm == "graven":
        lexer = GravenLexer()
    elif lang_norm and lang_norm not in ("text", "txt"):
        try:
            lexer = get_lexer_by_name({"sh": "bash", "shell": "bash"}.get(lang_norm, lang_norm))
        except ClassNotFound:
            lexer = None
    body = highlight(code, lexer, FORMATTER) if lexer else html.escape(code)
    cls = f' class="language-{html.escape(lang_norm)}"' if lang_norm else ""
    attr = f' data-lang="{html.escape(lang_norm)}"' if lang_norm else ""
    return f'<pre class="code"{attr}><code{cls}>{body}</code></pre>\n'


MD = MarkdownIt("commonmark", {"html": True, "highlight": highlight_code}).enable("table")

INCLUDE = re.compile(r"\{\{#include ([^}:]+)(?::([\w-]+))?(?::(\d))?\}\}")
TABS_BLOCK = re.compile(r"<!-- tabs -->\n(.*?)<!-- /tabs -->", re.DOTALL)


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
        shift = int(m.group(3) or 0)
        if shift:
            out, fence = [], False
            for line in src.split("\n"):
                if line.startswith("```"):
                    fence = not fence
                elif not fence and re.match(r"#{1,5} ", line):
                    line = "#" * shift + line
                out.append(line)
            src = "\n".join(out)
        return src.strip("\n")

    return INCLUDE.sub(repl, text)


def slugify(text):
    s = text.strip().lower()
    s = re.sub(r"[^\w\- ]", "", s)
    return s.replace(" ", "-")


def tab_key(label):
    low = label.strip().lower()
    if low == "rust":
        return "rust"
    if "javascript" in low or "typescript" in low or low in ("js", "ts", "js / ts"):
        return "js"
    return slugify(label)


def heading_ids(tokens):
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
    "testvectors/rpc-v1/README.md": "{root}docs/conformance-vectors.html#rpc-frames-v1",
}


def rewrite_links(body, src_rel, root):
    here = os.path.dirname(os.path.join("docs", src_rel))

    def repl(m):
        href = m.group(1)
        if re.match(r"^[a-z]+:", href) or href.startswith("#"):
            return m.group(0)
        path, _, frag = href.partition("#")
        frag = f"#{frag}" if frag else ""
        target = os.path.normpath(os.path.join(here, path))
        if path.startswith("docs/") and path.endswith(".md"):
            return f'href="{root}{path[:-3]}.html{frag}"'
        if target in SPECIAL_LINKS:
            return f'href="{SPECIAL_LINKS[target].format(root=root)}{frag}"'
        if path.endswith(".md"):
            return f'href="{path[:-3]}.html{frag}"'
        return m.group(0)

    return re.sub(r'href="([^"]+)"', repl, body)


CALLOUT_RE = re.compile(
    r"<blockquote>\s*<p>\[!(NOTE|TIP|IMPORTANT|WARNING)\]\s*(.*?)</p>(.*?)</blockquote>",
    re.DOTALL,
)


def transform_callouts(body):
    def repl(m):
        kind = m.group(1).lower()
        title = m.group(1).capitalize()
        first = m.group(2).strip()
        rest = m.group(3).strip()
        first_p = f"<p>{first}</p>" if first else ""
        return (
            f'<aside class="callout callout-{kind}">'
            f'<p class="callout-title">{title}</p>{first_p}{rest}</aside>'
        )

    return CALLOUT_RE.sub(repl, body)


def render_tabs_block(inner_md, src_rel, root):
    panels = []
    cur_label = None
    cur_lines = []
    fence = False
    for line in inner_md.splitlines():
        if line.strip().startswith("```"):
            fence = not fence
        if not fence and line.startswith("#### "):
            if cur_label is not None:
                panels.append((cur_label, "\n".join(cur_lines).strip()))
            cur_label = line[5:].strip()
            cur_lines = []
        else:
            cur_lines.append(line)
    if cur_label is not None:
        panels.append((cur_label, "\n".join(cur_lines).strip()))

    if not panels:
        return rewrite_links(transform_callouts(MD.render(inner_md)), src_rel, root)

    btns = []
    divs = []
    for i, (label, pmd) in enumerate(panels):
        key = tab_key(label)
        sel = "true" if i == 0 else "false"
        hid = "" if i == 0 else " hidden"
        phtml = rewrite_links(transform_callouts(MD.render(pmd)), src_rel, root)
        btns.append(
            f'<button type="button" role="tab" data-tab="{html.escape(key)}" '
            f'aria-selected="{sel}">{html.escape(label)}</button>'
        )
        divs.append(
            f'<div class="code-panel" role="tabpanel" data-tab="{html.escape(key)}"{hid}>'
            f'<p class="tab-label">{html.escape(label)}</p>{phtml}</div>'
        )
    return (
        f'<div class="code-tabs"><div class="code-tabbar" role="tablist" hidden>'
        f'{"".join(btns)}</div>{"".join(divs)}</div>'
    )


def render_markdown_with_tabs(text, src_rel, root):
    placeholders = {}

    def sub_tab(m):
        tok = f"GRAVEN_TABS_PLACEHOLDER_{len(placeholders)}"
        placeholders[tok] = render_tabs_block(m.group(1), src_rel, root)
        return f"\n\n{tok}\n\n"

    stripped = TABS_BLOCK.sub(sub_tab, text)
    tokens = MD.parse(stripped)
    toc = heading_ids(tokens)
    body = MD.renderer.render(tokens, MD.options, {})
    body = rewrite_links(body, src_rel, root)
    body = transform_callouts(body)
    body = re.sub(r"<p>\[(vectors?): ", r'<p class="ref">[\1: ', body)
    body = body.replace("<table>", '<div class="table-scroll"><table>').replace("</table>", "</table></div>")
    for tok, tab_html in placeholders.items():
        body = body.replace(f"<p>{tok}</p>", tab_html).replace(tok, tab_html)
    return body, toc


def nav_html(root, current):
    def link(src, title):
        cur = ' aria-current="page"' if src == current else ""
        return f'<li><a href="{root}docs/{src[:-3]}.html"{cur}>{html.escape(title)}</a></li>'

    cur = ' aria-current="page"' if current is None else ""
    groups = [
        f'<ul class="nav-top"><li><a href="{root}docs/index.html"{cur}>Overview</a></li></ul>'
    ]
    for section, pages in SECTIONS:
        links = "".join(link(src, title) for title, src, _ in pages)
        groups.append(
            f'<div class="nav-group"><p class="nav-heading">{html.escape(section)}</p>'
            f'<ul>{links}</ul></div>'
        )
    return "\n".join(groups)


def toc_html(toc):
    if len(toc) < 3:
        return ""
    items = "\n".join(
        f'<li class="l{lvl}"><a href="#{hid}">{html.escape(text)}</a></li>' for lvl, hid, text in toc if lvl == 2
    )
    return f'<nav class="toc" aria-label="On this page"><p class="toc-title">On this page</p><ul>{items}</ul></nav>'


def pager_html(src_rel, root):
    idx = next((i for i, (_, s, _) in enumerate(PAGES) if s == src_rel), None)
    if idx is None:
        return ""
    prev_link = ""
    next_link = ""
    if idx > 0:
        pt, ps, _ = PAGES[idx - 1]
        prev_link = (
            f'<a class="pager-prev" href="{root}docs/{ps[:-3]}.html">'
            f'<span>Previous</span><strong>{html.escape(pt)}</strong></a>'
        )
    if idx + 1 < len(PAGES):
        nt, ns, _ = PAGES[idx + 1]
        next_link = (
            f'<a class="pager-next" href="{root}docs/{ns[:-3]}.html">'
            f'<span>Next</span><strong>{html.escape(nt)}</strong></a>'
        )
    return f'<nav class="doc-pager" aria-label="Page navigation">{prev_link}{next_link}</nav>'


# The published site (Cloudflare Pages serves `x.html` at `/x`, so canonical URLs drop `.html`).
SITE = "https://graven.pages.dev"
OG_IMAGE = f"{SITE}/assets/og.png"
SITEMAP = []  # canonical URLs of every content page, for sitemap.xml


def canonical(path):
    """Canonical URL of a page written at `path` (relative to the site root)."""
    if path.endswith("index.html"):
        path = path[: -len("index.html")]
    elif path.endswith(".html"):
        path = path[: -len(".html")]
    return f"{SITE}/{path}"


PAGE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} — Graven</title>
<meta name="description" content="{desc}">
<link rel="canonical" href="{canonical}">
<meta property="og:type" content="article">
<meta property="og:site_name" content="Graven">
<meta property="og:title" content="{title} — Graven">
<meta property="og:description" content="{desc}">
<meta property="og:url" content="{canonical}">
<meta property="og:image" content="{og_image}">
<meta property="og:image:width" content="1200">
<meta property="og:image:height" content="630">
<meta name="twitter:card" content="summary_large_image">
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
      <summary>Menu</summary>
      {nav}
    </details>
  </aside>
  <main id="doc" class="doc">
{body}
{pager}
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
      <a href="{root}docs/tools.html">gravenc</a>
      <a href="{root}docs/conformance-vectors.html">Conformance vectors</a>
      <a href="{root}api/graven/index.html">Rust API</a>
      <a href="{root}docs/changelog.html">Changelog</a>
      <a href="https://github.com/lpj145/graven">GitHub</a>
    </nav>
  </div>
</footer>
<script>
  (function () {{
    const KEY = "graven-lang";
    const get = () => {{ try {{ return localStorage.getItem(KEY); }} catch (e) {{ return null; }} }};
    const set = (v) => {{ try {{ localStorage.setItem(KEY, v); }} catch (e) {{}} }};

    const groups = [...document.querySelectorAll(".code-tabs")];
    const activateAll = (lang) => {{
      groups.forEach((g) => {{
        const tabs = [...g.querySelectorAll(".code-tabbar [role=tab]")];
        const target = tabs.find((t) => t.dataset.tab === lang) || tabs[0];
        if (!target) return;
        const active = target.dataset.tab;
        tabs.forEach((t) => t.setAttribute("aria-selected", String(t.dataset.tab === active)));
        g.querySelectorAll(".code-panel").forEach((p) => (p.hidden = p.dataset.tab !== active));
      }});
    }};
    groups.forEach((g) => {{
      g.classList.add("js");
      const bar = g.querySelector(".code-tabbar");
      if (bar) bar.hidden = false;
      g.querySelectorAll("[role=tab]").forEach((btn) =>
        btn.addEventListener("click", () => {{
          set(btn.dataset.tab);
          activateAll(btn.dataset.tab);
        }})
      );
    }});
    if (groups.length) activateAll(get());

    document.querySelectorAll("#doc pre.code").forEach((pre) => {{
      const wrap = document.createElement("div");
      wrap.className = "code-wrap";
      pre.parentNode.insertBefore(wrap, pre);
      wrap.appendChild(pre);
      const bar = document.createElement("div");
      bar.className = "code-meta";
      const lang = pre.dataset.lang || "";
      const tag = document.createElement("span");
      tag.className = "code-lang";
      tag.textContent = lang;
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "code-copy";
      btn.textContent = "Copy";
      btn.addEventListener("click", async () => {{
        try {{
          await navigator.clipboard.writeText(pre.innerText);
          btn.textContent = "Copied";
          btn.dataset.copied = "true";
          setTimeout(() => {{ btn.textContent = "Copy"; delete btn.dataset.copied; }}, 1500);
        }} catch (e) {{}}
      }});
      bar.appendChild(tag);
      bar.appendChild(btn);
      wrap.insertBefore(bar, pre);
    }});
  }})();
  if (window.matchMedia("(max-width: 960px)").matches) {{
    document.querySelectorAll(".docs-menu").forEach((d) => d.removeAttribute("open"));
  }}
  document.querySelectorAll(".docs-menu > summary").forEach((s) =>
    s.addEventListener("click", (e) => {{
      if (!window.matchMedia("(max-width: 960px)").matches) e.preventDefault();
    }})
  );
</script>
</body>
</html>
"""

CSS_V = hashlib.sha256(open(os.path.join(ROOT, "site", "assets", "site.css"), "rb").read()).hexdigest()[:10]
PAGE = PAGE.replace("assets/site.css", f"assets/site.css?v={CSS_V}")


def render_page(src_rel, out_dir, title, desc):
    raw = open(os.path.join(DOCS, src_rel), encoding="utf8").read()
    text = expand_includes(raw, os.path.dirname(os.path.join(DOCS, src_rel)))
    # Emit clean Markdown copy (tab markers removed so plain Markdown readers see clean #### subheadings)
    md_clean = text.replace("<!-- tabs -->\n", "").replace("<!-- /tabs -->\n", "").replace("<!-- /tabs -->", "")
    md_out = os.path.join(out_dir, "docs", src_rel)
    os.makedirs(os.path.dirname(md_out), exist_ok=True)
    open(md_out, "w", encoding="utf8").write(md_clean)

    depth = src_rel.count("/") + 1
    root = "../" * depth
    body, toc = render_markdown_with_tabs(text, src_rel, root)
    out_rel = "docs/" + src_rel[:-3] + ".html"
    SITEMAP.append(canonical(out_rel))
    page = PAGE.format(
        title=html.escape(title),
        desc=html.escape(desc),
        canonical=canonical(out_rel),
        og_image=OG_IMAGE,
        root=root,
        nav=nav_html(root, src_rel),
        body=body,
        pager=pager_html(src_rel, root),
        toc=toc_html(toc),
    )
    out = os.path.join(out_dir, "docs", src_rel[:-3] + ".html")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    open(out, "w", encoding="utf8").write(page)


def index_page(out_dir):
    featured = [
        ("Getting started", "getting-started.html", "Install gravenc, create your first schema + lockfile, and generate bindings."),
        ("Schema language", "graven-language.html", "Tables, structs, unions, enums, imports, and lockfile compatibility rules."),
        ("Rust SDK", "sdk/rust.html", "Zero-copy runtime (no_std), build.rs codegen, validation, and server edge."),
        ("JavaScript / TypeScript SDK", "sdk/js.html", "Zero-dependency ES2022 .js + .d.ts modules for browsers, Node, and Deno."),
    ]
    cards = "\n".join(
        f'<a class="hero-card" href="{href}"><strong>{html.escape(t)}</strong><span>{html.escape(d)}</span></a>'
        for t, href, d in featured
    )

    sections_html = []
    for section, pages in SECTIONS:
        rows = "\n".join(
            f'<a class="doc-card" href="{src[:-3]}.html"><strong>{html.escape(t)}</strong><span>{html.escape(d)}</span></a>'
            for t, src, d in pages
        )
        sections_html.append(
            f'<h2 id="{slugify(section)}">{html.escape(section)}</h2><div class="doc-cards">{rows}</div>'
        )

    body = (
        '<h1 id="documentation">Documentation</h1>'
        '<p class="lede-doc">One language-neutral binary format and schema compiler, paired with native '
        'SDKs that read and write identical bytes.</p>'
        f'<div class="hero-cards">{cards}</div>'
        f'{"".join(sections_html)}'
    )
    SITEMAP.append(canonical("docs/index.html"))
    page = PAGE.format(
        title="Documentation",
        desc="Graven documentation: the schema language, the format and RPC specifications, conformance vectors, and the Rust and JavaScript SDKs.",
        canonical=canonical("docs/index.html"),
        og_image=OG_IMAGE,
        root="../",
        nav=nav_html("../", None),
        body=body,
        pager="",
        toc="",
    )
    open(os.path.join(out_dir, "docs", "index.html"), "w", encoding="utf8").write(page)


def redirect_pages(out_dir):
    docs_dir = os.path.join(out_dir, "docs")
    for old, new in REDIRECTS.items():
        out_path = os.path.join(docs_dir, old + ".html")
        os.makedirs(os.path.dirname(out_path), exist_ok=True)
        rel_dir = os.path.dirname(out_path)
        new_file, _, frag = new.partition("#")
        rel_target = os.path.relpath(os.path.join(docs_dir, new_file), rel_dir)
        target = f"{rel_target}#{frag}" if frag else rel_target
        absolute = canonical("docs/" + new_file)  # search engines ignore fragments
        page = (
            f'<!doctype html><meta charset="utf-8"><title>Moved — Graven</title>'
            f'<meta name="robots" content="noindex">'
            f'<meta http-equiv="refresh" content="0; url={target}"><link rel="canonical" href="{absolute}">'
            f'<p>This page moved to <a href="{target}">{target}</a>.</p>'
        )
        open(out_path, "w", encoding="utf8").write(page)


def build(out_dir):
    for title, src, desc in PAGES:
        render_page(src, out_dir, title, desc)
    for name in sorted(os.listdir(os.path.join(DOCS, "proposals"))):
        if name.endswith(".md"):
            first = open(os.path.join(DOCS, "proposals", name), encoding="utf8").readline().lstrip("# ").strip()
            render_page(f"proposals/{name}", out_dir, first, first)
    for sub in ("design", "plans", "benchmarks"):
        for name in sorted(os.listdir(os.path.join(DOCS, "history", sub))):
            if name.endswith(".md"):
                first = open(os.path.join(DOCS, "history", sub, name), encoding="utf8").readline().lstrip("# ").strip()
                render_page(f"history/{sub}/{name}", out_dir, first, first)
    index_page(out_dir)
    redirect_pages(out_dir)
    llms_files(out_dir)
    return [src for _, src, _ in PAGES]


REPO = "https://github.com/lpj145/graven"

LLMS_SUMMARY = (
    "Graven is an open specification for a zero-copy binary serialization format and its schema "
    "language (.graven), with a compiler (gravenc), lockfiles that refuse incompatible schema changes, "
    "frozen conformance vectors, a reference implementation in Rust and an implementation for JavaScript "
    "and TypeScript."
)


def llms_files(out_dir):
    lines = [
        "# Graven",
        "",
        f"> {LLMS_SUMMARY}",
        "",
        f"Source code: {REPO}. Start with the guide for AI assistants: it has the schema syntax, the rules gravenc enforces, "
        "the commands and the Rust and JavaScript APIs in one page. Every page below is Markdown.",
        "",
        "## Docs",
        "",
    ]
    for title, src, desc in sorted(PAGES, key=lambda p: p[1] != "for-ai.md"):
        if src.startswith("history/"):
            continue
        lines.append(f"- [{title}](docs/{src}): {desc}")
    lines += [
        "",
        "## Optional",
        "",
        "- [History](docs/history/index.md): how the format was designed and benchmarked (not normative).",
        "- [Proposal 0001: imports](docs/proposals/0001-imports.md): the design of imports between schema files.",
        "- [Proposal 0002: validation](docs/proposals/0002-validation.md): built-in rules and named checks.",
        "- [Proposal 0003: services](docs/proposals/0003-services.md): RPC framing, typed errors, client and server edge.",
        "- [Proposal 0004: JavaScript / TypeScript](docs/proposals/0004-typescript.md): JS runtime and .d.ts codegen.",
        "- [Complete documentation in one file](llms-full.txt)",
        "",
    ]
    open(os.path.join(out_dir, "llms.txt"), "w", encoding="utf8").write("\n".join(lines))

    order = ["for-ai.md"] + [src for _, src, _ in PAGES if src not in ("for-ai.md", "history/index.md", "changelog.md")]
    parts = [f"# Graven: complete documentation\n\n> {LLMS_SUMMARY}\n"]
    for src in order:
        text = open(os.path.join(out_dir, "docs", src), encoding="utf8").read()
        parts.append(f"\n\n---\n\n<!-- source: docs/{src} -->\n\n{text.strip()}\n")
    open(os.path.join(out_dir, "llms-full.txt"), "w", encoding="utf8").write("".join(parts))
