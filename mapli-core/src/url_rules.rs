//! Rules that restrict which URLs MapLibre Native may fetch.
//!
//! MapLibre Native fetches every URL a style names, from whichever of its sources serves the
//! URL's scheme: HTTP(S), local files, MBTiles and PMTiles archives, and the custom schemes of
//! [`register_scheme_handler`](crate::register_scheme_handler). An application that renders
//! styles it does not fully control can call [`restrict_resource_urls`] to only allow the URLs
//! that start with one of a list of prefixes. Without it, every URL is allowed.
//!
//! The rules are checked by the scheme handlers' dispatcher, which MapLibre Native asks about
//! every request before any of its own sources. It claims the URLs that the rules deny and
//! answers them with an error, so they never reach the source that would fetch them. This
//! includes the archive reads that MapLibre Native's PMTiles support makes for `pmtiles://`
//! URLs.
//!
//! URLs are compared in a canonical form, so that a URL that starts with an allowed prefix
//! cannot reach outside of it:
//!
//! - schemes are compared case-insensitively;
//! - `pmtiles://` URLs are checked by the URL they wrap, e.g. `pmtiles://https://a/b.pmtiles`
//!   by `https://a/b.pmtiles`, and `mbtiles://` URLs as the `file://` URL of the same path;
//! - local file URLs are percent-decoded, as MapLibre Native does before opening the file, and
//!   denied if their path has `.` or `..` segments, or a malformed `%` sequence;
//! - other URLs are parsed and normalized (e.g. dot segments are resolved and host names
//!   lowercased), and denied if they cannot be parsed, have credentials, or contain whitespace,
//!   control characters or backslashes, which URL parsers disagree about;
//! - a prefix only matches up to a path boundary: `https://example.org/a` allows
//!   `https://example.org/a/b` but not `https://example.org/ab`.
//!
//! The rules can only be set once per process, before the first
//! [`RenderPool`](crate::RenderPool) is created, so that they cannot be loosened while styles are
//! being rendered.

use std::borrow::Cow;
use std::sync::{Mutex, OnceLock};

use url::Url;

use crate::error::{MapliError, Result};

/// Only allow MapLibre Native to fetch the URLs that start with one of `allowed_url_prefixes`,
/// process-wide.
///
/// A prefix is either a URL, e.g. `https://fonts.example.org/`, or a scheme followed by `://`,
/// e.g. `store://`, which allows every URL of the scheme. Local files are denied unless a
/// `file://` prefix allows them. An empty list denies every URL.
///
/// Requests for denied URLs fail with [`ErrorReason::Other`](crate::ErrorReason::Other) and the
/// message `"<url>: access denied by URL rules"`. This is also the case for URLs of a scheme
/// that has a handler registered.
///
/// Returns [`MapliError::InvalidInput`] if a prefix is not a valid URL prefix, and
/// [`MapliError::UrlRulesClosed`] if a render pool has already been created, or if the rules
/// have already been set.
pub fn restrict_resource_urls<S: AsRef<str>>(allowed_url_prefixes: &[S]) -> Result<()> {
    let rules = UrlRules::new(allowed_url_prefixes)?;
    let pool_created = POOL_CREATED.lock().unwrap_or_else(|p| p.into_inner());
    if *pool_created {
        return Err(MapliError::UrlRulesClosed(
            "a render pool has already been created".to_string(),
        ));
    }
    RULES
        .set(rules)
        .map_err(|_| MapliError::UrlRulesClosed("they have already been set".to_string()))
}

/// Close the rules for the rest of the process, as a render pool is being created.
pub(crate) fn close_rules() {
    *POOL_CREATED.lock().unwrap_or_else(|p| p.into_inner()) = true;
}

/// Whether the rules deny `url`. No rules means that every URL is allowed.
pub(crate) fn denies(url: &str) -> bool {
    RULES.get().is_some_and(|rules| !rules.allows(url))
}

/// Whether a render pool has been created, which closes the rules. Setting the rules holds the
/// lock, so that they cannot be set while a pool is being created.
static POOL_CREATED: Mutex<bool> = Mutex::new(false);
static RULES: OnceLock<UrlRules> = OnceLock::new();

/// The allowed URL prefixes.
#[derive(Debug)]
struct UrlRules {
    allowed: Vec<Prefix>,
}

impl UrlRules {
    fn new<S: AsRef<str>>(allowed_url_prefixes: &[S]) -> Result<Self> {
        let allowed = allowed_url_prefixes
            .iter()
            .map(|prefix| Prefix::parse(prefix.as_ref()))
            .collect::<Result<_>>()?;
        Ok(Self { allowed })
    }

    fn allows(&self, url: &str) -> bool {
        let Some(url) = canonicalize_url(&unwrap_archive(url)) else {
            return false;
        };
        self.allowed.iter().any(|prefix| prefix.matches(&url))
    }
}

/// An allowed URL prefix.
#[derive(Debug, PartialEq, Eq)]
enum Prefix {
    /// Every URL of a scheme, lowercased
    Scheme(String),
    /// The URLs that start with this one, up to a path boundary
    Url(CanonicalUrl),
}

impl Prefix {
    fn parse(prefix: &str) -> Result<Self> {
        let unwrapped = unwrap_archive(prefix);
        let scheme_only = unwrapped
            .strip_suffix("://")
            .or_else(|| unwrapped.strip_suffix(':'))
            .filter(|scheme| is_scheme(scheme));
        if let Some(scheme) = scheme_only {
            return Ok(Self::Scheme(scheme.to_ascii_lowercase()));
        }
        canonicalize_url(&unwrapped).map(Self::Url).ok_or_else(|| {
            MapliError::InvalidInput(format!(
                "{prefix:?} is not a valid URL prefix: it must be a URL, or a scheme followed by \
                 \"://\""
            ))
        })
    }

    fn matches(&self, url: &CanonicalUrl) -> bool {
        match self {
            Self::Scheme(scheme) => url.scheme == *scheme,
            Self::Url(prefix) => {
                let Some(rest) = url.text.strip_prefix(prefix.text.as_str()) else {
                    return false;
                };
                // in local file URLs, '?' and '#' are part of the file name
                let boundaries: &[char] = if url.scheme == "file" {
                    &['/']
                } else {
                    &['/', '?', '#']
                };
                prefix.text.ends_with('/') || rest.is_empty() || rest.starts_with(boundaries)
            }
        }
    }
}

/// A URL in the form that the rules compare.
#[derive(Debug, PartialEq, Eq)]
struct CanonicalUrl {
    /// Lowercased
    scheme: String,
    text: String,
}

/// The URL that a `pmtiles://` URL wraps, and the `file://` URL of the path of an `mbtiles://`
/// URL, which are what MapLibre Native reads.
fn unwrap_archive(url: &str) -> Cow<'_, str> {
    let mut url = url;
    while let Some(inner) = strip_prefix_ignore_case(url, "pmtiles://") {
        url = inner;
    }
    match strip_prefix_ignore_case(url, "mbtiles://") {
        Some(path) => Cow::Owned(format!("file://{path}")),
        None => Cow::Borrowed(url),
    }
}

/// `url` in canonical form, or `None` if it must be denied whatever the rules.
fn canonicalize_url(url: &str) -> Option<CanonicalUrl> {
    let (scheme, rest) = url.split_once(':')?;
    if !is_scheme(scheme) {
        return None;
    }
    let scheme = scheme.to_ascii_lowercase();

    if scheme == "file" {
        // MapLibre Native opens everything after `file://`, percent-decoded, as the path
        let path = percent_decode(rest.strip_prefix("//")?)?;
        if path.chars().any(char::is_control) || path.split('/').any(|s| s == "." || s == "..") {
            return None;
        }
        return Some(CanonicalUrl {
            text: format!("file://{path}"),
            scheme,
        });
    }

    if url
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
    {
        return None;
    }
    let parsed = Url::parse(url).ok()?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }
    Some(CanonicalUrl {
        text: parsed.into(),
        scheme,
    })
}

/// Whether `scheme` is a valid URI scheme (RFC 3986, section 3.1).
fn is_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn strip_prefix_ignore_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// Decode the `%XX` sequences of `s`. Returns `None` if a `%` is not followed by two hex digits,
/// which MapLibre Native decodes in ways of its own, or if the result is not UTF-8.
fn percent_decode(s: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(s.len());
    let mut rest = s.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'%' {
            let hex = tail
                .get(..2)
                .and_then(|hex| std::str::from_utf8(hex).ok())?;
            if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            rest = &tail[2..];
        } else {
            bytes.push(byte);
            rest = tail;
        }
    }
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_rules(prefixes: &[&str]) -> UrlRules {
        UrlRules::new(prefixes).unwrap()
    }

    fn assert_urls_allowed(rules: &UrlRules, urls: &[&str]) {
        for url in urls {
            assert!(rules.allows(url), "{url:?} was denied by {rules:?}");
        }
    }

    fn assert_urls_denied(rules: &UrlRules, urls: &[&str]) {
        for url in urls {
            assert!(!rules.allows(url), "{url:?} was allowed by {rules:?}");
        }
    }

    #[test]
    fn no_prefixes_deny_everything() {
        assert_urls_denied(
            &build_rules(&[]),
            &["https://example.org/", "file:///tmp/a", "store://a"],
        );
    }

    #[test]
    fn scheme_prefixes_allow_the_whole_scheme() {
        let rules = build_rules(&["store://", "HTTPS:"]);
        assert_urls_allowed(
            &rules,
            &["store://a/b", "STORE://a", "https://example.org/x"],
        );
        assert_urls_denied(
            &rules,
            &["http://example.org/", "file:///tmp/a", "stores://a"],
        );
    }

    #[test]
    fn url_prefixes_match_at_path_boundaries() {
        let rules = build_rules(&["https://example.org/fonts", "store://tiles/"]);
        assert_urls_allowed(
            &rules,
            &[
                "https://example.org/fonts",
                "https://example.org/fonts/a.pbf",
                "https://example.org/fonts?key=1",
                "https://EXAMPLE.org:443/fonts/a",
                "HTTPS://example.org/fonts/a",
                "store://tiles/a.pmtiles",
                "Store://tiles/a.pmtiles",
            ],
        );
        assert_urls_denied(
            &rules,
            &[
                "https://example.org/fontsevil",
                "https://example.org/other",
                "http://example.org/fonts/a",
                "store://tilesevil/a",
                "store://other/a",
            ],
        );
    }

    #[test]
    fn host_prefixes_do_not_match_other_hosts() {
        let rules = build_rules(&["https://example.org"]);
        assert_urls_allowed(&rules, &["https://example.org/a"]);
        assert_urls_denied(
            &rules,
            &[
                "https://example.org.evil.com/a",
                "https://example.org@evil.com/a",
                "https://example.org:8443/a",
            ],
        );
    }

    #[test]
    fn remote_urls_that_parsers_disagree_about_are_denied() {
        let rules = build_rules(&["https://"]);
        assert_urls_denied(
            &rules,
            &[
                "https://user:pass@example.org/",
                "https://example.org\\@evil.com/",
                "https://exam\tple.org/",
                "https://example.org/a b",
                "https:///",
            ],
        );
    }

    #[test]
    fn dot_segments_are_resolved_in_remote_urls() {
        let rules = build_rules(&["https://example.org/public/", "store://tiles/public/"]);
        assert_urls_allowed(&rules, &["https://example.org/public/a/../b"]);
        assert_urls_denied(
            &rules,
            &[
                "https://example.org/public/../private",
                "https://example.org/public/%2e%2e/private",
                "store://tiles/public/../private/a",
            ],
        );
    }

    #[test]
    fn local_files_are_denied_unless_allowed() {
        assert_urls_denied(
            &build_rules(&["https://", "store://"]),
            &[
                "file:///etc/passwd",
                "FILE:///etc/passwd",
                "mbtiles:///data/a.mbtiles",
                "pmtiles://file:///data/a.pmtiles",
            ],
        );
    }

    #[test]
    fn local_file_prefixes_match_decoded_paths() {
        let rules = build_rules(&["file:///data/"]);
        assert_urls_allowed(
            &rules,
            &[
                "file:///data/a.json",
                "FILE:///data/a.json",
                "file:///data/my%20style.json",
                "file:///data/a?b.json",
            ],
        );
        assert_urls_denied(
            &rules,
            &[
                "file:///etc/passwd",
                "file:///data/../etc/passwd",
                "file:///data/./a.json",
                "file:///data/%2e%2e/etc/passwd",
                "file:///data/x%2f..%2f..%2fetc/passwd",
                "file:///data/%zz",
                "file:///data/a%",
                "file:///data/a%00.json",
                "file:///data/%ff",
                "file://data/a.json",
                "file:/data/a.json",
            ],
        );
    }

    #[test]
    fn local_file_prefixes_match_at_path_boundaries() {
        let rules = build_rules(&["file:///data"]);
        assert_urls_allowed(&rules, &["file:///data/a.json"]);
        assert_urls_denied(&rules, &["file:///data2/a.json", "file:///data?/a.json"]);
    }

    #[test]
    fn archive_urls_are_checked_by_what_they_read() {
        let rules = build_rules(&["file:///data/", "https://tiles.example.org/"]);
        assert_urls_allowed(
            &rules,
            &[
                "mbtiles:///data/a.mbtiles",
                "MBTiles:///data/a.mbtiles",
                "pmtiles://file:///data/a.pmtiles",
                "pmtiles://https://tiles.example.org/a.pmtiles",
                "PMTiles://https://tiles.example.org/a.pmtiles",
            ],
        );
        assert_urls_denied(
            &rules,
            &[
                "mbtiles:///etc/a.mbtiles",
                "mbtiles:///data/../etc/a.mbtiles",
                "pmtiles://https://evil.com/a.pmtiles",
                "pmtiles://file:///etc/a.pmtiles",
                "pmtiles://",
            ],
        );
    }

    #[test]
    fn archive_prefixes_are_unwrapped() {
        assert_eq!(
            Prefix::parse("mbtiles://").unwrap(),
            Prefix::Scheme("file".into())
        );
        assert_eq!(
            Prefix::parse("pmtiles://https://").unwrap(),
            Prefix::Scheme("https".into())
        );
        let rules = build_rules(&["mbtiles:///data/"]);
        assert_urls_allowed(&rules, &["file:///data/a.json"]);
    }

    #[test]
    fn invalid_prefixes_are_refused() {
        for prefix in [
            "",
            "no-scheme",
            "1store://",
            "pmtiles://",
            "file:/",
            "https://a b/",
        ] {
            let result = Prefix::parse(prefix);
            assert!(
                matches!(result, Err(MapliError::InvalidInput(_))),
                "{prefix:?} was accepted"
            );
        }
    }
}
