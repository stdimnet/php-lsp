//! A port of js-beautify (HTML, CSS and JavaScript), used to format the
//! HTML of PHP templates exactly like Intelephense does.

pub(crate) mod core;
pub(crate) mod css;
pub(crate) mod html;
pub(crate) mod javascript;

/// Formats `text` as HTML with Intelephense's js-beautify settings
/// (`<script>` and `<style>` contents included).
pub fn html(text: &str, indent_size: usize, use_tabs: bool) -> String {
    let options = html::HtmlOptions::intelephense(indent_size, use_tabs);
    html::Beautifier::new(text, &options, Some(javascript::beautify), Some(css::beautify)).beautify()
}

/// Like [`html`] without formatting `<script>` / `<style>` contents.
pub fn html_only(text: &str, indent_size: usize, use_tabs: bool) -> String {
    let options = html::HtmlOptions::intelephense(indent_size, use_tabs);
    html::Beautifier::new(text, &options, None, None).beautify()
}

/// Formats JavaScript as a `<script>` of a template.
pub fn javascript(text: &str, indent_size: usize, use_tabs: bool) -> String {
    javascript::beautify(text, &html::ChildOptions { indent_size, use_tabs, wrap_line_length: 0 })
}

/// Formats CSS as a `<style>` of a template.
pub fn css(text: &str, indent_size: usize, use_tabs: bool) -> String {
    css::beautify(text, &html::ChildOptions { indent_size, use_tabs, wrap_line_length: 0 })
}
