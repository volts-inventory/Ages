//! HTML rendering of the post-run report. Same digest input as
//! `render::markdown`; runs the markdown through `pulldown-cmark`
//! and wraps it in a minimal HTML shell with embedded CSS tuned
//! for the ASCII maps + Unicode sparklines (monospace, no line
//! wrapping in `<pre>` blocks, sane font fallbacks).
//!
//! No external assets: the stylesheet is embedded so the output
//! file works when opened directly from disk.

use crate::digest::Digest;
use crate::render::markdown;
use pulldown_cmark::{html as cm_html, Options, Parser};

const STYLE: &str = r#"
:root { color-scheme: light dark; }
body {
  max-width: 60rem;
  margin: 2rem auto;
  padding: 0 1rem;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
  line-height: 1.5;
}
h1, h2, h3 { line-height: 1.2; }
h1 { border-bottom: 2px solid currentColor; padding-bottom: 0.3em; }
h2 { border-bottom: 1px solid currentColor; padding-bottom: 0.2em; margin-top: 2em; }
h3 { margin-top: 1.5em; }
pre {
  font-family: "JetBrains Mono", Menlo, Consolas, "Courier New", monospace;
  font-size: 0.875rem;
  line-height: 1.15;
  padding: 0.75rem 1rem;
  background: #f5f5f5;
  border: 1px solid #ddd;
  border-radius: 4px;
  overflow-x: auto;
  white-space: pre;
}
@media (prefers-color-scheme: dark) {
  pre { background: #1a1a1a; border-color: #333; }
}
code {
  font-family: "JetBrains Mono", Menlo, Consolas, "Courier New", monospace;
  font-size: 0.9em;
  padding: 0.1em 0.3em;
  background: rgba(127, 127, 127, 0.15);
  border-radius: 3px;
}
pre > code { padding: 0; background: none; }
table {
  border-collapse: collapse;
  margin: 1em 0;
}
th, td {
  border: 1px solid #aaa;
  padding: 0.3em 0.7em;
}
th { background: rgba(127, 127, 127, 0.1); }
ul, ol { padding-left: 1.5rem; }
li { margin: 0.2em 0; }
"#;

/// Convert a digest to an HTML document. Renders markdown first
/// (single source of truth for content), then converts via
/// `pulldown-cmark` with table + footnote extensions, then wraps
/// in an HTML shell with the embedded stylesheet.
#[must_use]
pub fn html(d: &Digest) -> String {
    let md = markdown(d);
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(&md, opts);
    let mut body = String::new();
    cm_html::push_html(&mut body, parser);

    let title = format!("Ages — seed {}", d.seed);
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n  <title>{title}</title>\n  <style>{STYLE}</style>\n</head>\n<body>\n{body}\n</body>\n</html>\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::Digest;

    #[test]
    fn empty_digest_renders_html_shell() {
        let d = Digest::from_events(&[]);
        let out = html(&d);
        assert!(out.contains("<!doctype html>"));
        assert!(out.contains("<title>Ages"));
        assert!(out.contains("<style>"));
        assert!(out.contains("</body>"));
    }
}
