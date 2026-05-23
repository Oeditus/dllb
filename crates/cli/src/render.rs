//! Terminal rendering via [`marcli`].
//!
//! Wraps every user-facing string in Markdown so that `marcli::render`
//! produces ANSI-coloured, syntax-highlighted output.
//!
//! When colour is disabled ([`Renderer::plain`]), output is still
//! pretty-printed but contains no ANSI escape sequences.

use std::sync::LazyLock;

use marcli::RenderOptions;

static OPTS: LazyLock<RenderOptions> = LazyLock::new(RenderOptions::default);
static PLAIN_OPTS: LazyLock<RenderOptions> = LazyLock::new(|| RenderOptions {
    escape_sequences: false,
    ..RenderOptions::default()
});

/// Holds the colour mode for the session.
#[derive(Debug, Clone, Copy)]
pub struct Renderer {
    color: bool,
}

impl Renderer {
    /// Default renderer with ANSI colours enabled.
    pub fn colored() -> Self {
        Self { color: true }
    }

    /// Plain renderer -- no ANSI escape sequences.
    pub fn plain() -> Self {
        Self { color: false }
    }

    fn opts(&self) -> &'static RenderOptions {
        if self.color { &OPTS } else { &PLAIN_OPTS }
    }

    /// Render a Markdown string to the terminal.
    pub fn md(&self, markdown: &str) -> String {
        marcli::render(markdown, self.opts())
    }

    /// Render a fenced code block with the given language tag.
    pub fn code(&self, raw: &str, lang: &str) -> String {
        let content = if lang == "json" { prettify_json(raw) } else { raw.to_string() };
        let block = format!("```{lang}\n{content}\n```");
        marcli::render(&block, self.opts())
    }

    /// Render an error (bold heading + highlighted code block).
    pub fn error(&self, raw: &str, lang: &str) -> String {
        let content = if lang == "json" { prettify_json(raw) } else { raw.to_string() };
        let block = format!("**error**\n\n```{lang}\n{content}\n```");
        marcli::render(&block, self.opts())
    }
}

/// Best-effort pretty-print; falls back to the original string.
fn prettify_json(raw: &str) -> String {
    serde_json::from_str::<serde_json::Value>(raw)
        .and_then(|v| serde_json::to_string_pretty(&v))
        .unwrap_or_else(|_| raw.to_string())
}
