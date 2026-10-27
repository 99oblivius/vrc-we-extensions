//! The game log an extension may read, if its manifest names prefixes.

/// The most prefixes in `log_prefixes`.
pub const LOG_PREFIXES_MAX: usize = 8;

/// The longest log prefix, in bytes.
pub const LOG_PREFIX_MAX: usize = 128;

/// The longest log line an extension receives, in bytes; a longer one is
/// cut at a character boundary.
pub const LOG_LINE_MAX: usize = 64 * 1024;

/// The longest start of `text` that fits `max` bytes, cut at a character
/// boundary.
pub fn cut_to(text: &str, max: usize) -> &str {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// A log prefix: some text of at most [`LOG_PREFIX_MAX`] bytes, without
/// control characters.
pub(crate) fn is_log_prefix(prefix: &str) -> bool {
    !prefix.is_empty() && prefix.len() <= LOG_PREFIX_MAX && !prefix.chars().any(char::is_control)
}
