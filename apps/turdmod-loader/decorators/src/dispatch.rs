//! Pure-logic markup-attribute dispatch for the rich-text decorator DLL.
//!
//! Given a tag name + attribute map (as parsed by UE4's RichTextBlock
//! markup parser, surfaced through our detoured `CreateDecorator`), this
//! module decides what behaviour to invoke: URL fetch, DataTable lookup,
//! hyperlink, dismiss button, etc. The canonical tag dialect lives in
//! `examples/turdmod/welcome-screen/PAYLOAD-SCHEMA.md` ("Phase C: markup-
//! string output"); keep that doc and this module in sync.
//!
//! No UE4 dependencies — fully unit-testable in isolation.

use std::collections::HashMap;

use url::Url;

/// Maximum length (chars) for `alt` text and hyperlink `label` content
/// before truncation. Anything longer gets sliced and an ellipsis appended.
const MAX_TEXT_LEN: usize = 200;
/// Truncation suffix appended when label/alt overflows `MAX_TEXT_LEN`.
const TRUNCATE_SUFFIX: &str = "…";

/// One outcome per recognised markup tag. Variants are 1:1 with the
/// behaviours the detour body needs to perform; `Unknown` is the catch-all
/// for tags we don't handle (caller logs and falls back to engine-stock).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// `<img src="https://..."/>` — fetch URL via `crate::image_fetch`.
    FetchImage {
        src: String,
        alt: Option<String>,
        width: Option<u32>,
        height: Option<u32>,
    },
    /// `<img id="key"/>` — DataTable lookup by row key.
    LookupImage {
        id: String,
        alt: Option<String>,
        width: Option<u32>,
        height: Option<u32>,
    },
    /// `<banner ...>` — hoists a `FetchImage` / `LookupImage` to the
    /// panel's banner slot rather than rendering inline.
    HoistBanner(Box<Action>),
    /// `<a href="..." key="...">label</a>` — clickable hyperlink with
    /// optional keybind shortcut.
    Hyperlink {
        href: String,
        key: Option<String>,
        label: String,
    },
    /// `<dismiss key="Escape" label="Got it"/>` — close button + keybind.
    Dismiss {
        key: String,
        label: Option<String>,
    },
    /// `<title>content</title>` — hoist content to panel title.
    HoistTitle(String),
    /// `<section title="...">` — section grouping (rendering hint only).
    SectionStart(String),
    /// Tag we don't recognise (or required attr missing / URL invalid).
    /// Carries the original tag name for logging.
    Unknown(String),
}

/// Decide what to do for a single markup tag. Pure function — no I/O, no
/// engine calls. `content` is the inner text run for tags like
/// `<a>...</a>` or `<title>...</title>`; pass `None` for void tags.
pub fn dispatch(
    tag_name: &str,
    attrs: &HashMap<String, String>,
    content: Option<&str>,
) -> Action {
    let lower = tag_name.trim().to_ascii_lowercase();
    match lower.as_str() {
        "img" => image_action(tag_name, attrs),
        "banner" => match image_action(tag_name, attrs) {
            Action::Unknown(name) => Action::Unknown(name),
            inner => Action::HoistBanner(Box::new(inner)),
        },
        "a" => hyperlink_action(tag_name, attrs, content),
        "dismiss" => dismiss_action(tag_name, attrs),
        "title" => match content.map(str::trim).filter(|s| !s.is_empty()) {
            Some(c) => Action::HoistTitle(truncate(c, MAX_TEXT_LEN)),
            None => Action::Unknown(tag_name.to_string()),
        },
        "section" => match get_trimmed(attrs, "title") {
            Some(title) if !title.is_empty() => Action::SectionStart(truncate(&title, MAX_TEXT_LEN)),
            _ => Action::Unknown(tag_name.to_string()),
        },
        _ => Action::Unknown(tag_name.to_string()),
    }
}

/// Builds `FetchImage` (when `src=` is present and is a valid https URL)
/// or `LookupImage` (when `id=` is present). If both are present, `src`
/// wins. Returns `Unknown` when neither is present or when the URL fails
/// validation.
fn image_action(tag_name: &str, attrs: &HashMap<String, String>) -> Action {
    let alt = get_trimmed(attrs, "alt").map(|s| truncate(&s, MAX_TEXT_LEN));
    let width = get_trimmed(attrs, "width").and_then(|s| s.parse::<u32>().ok());
    let height = get_trimmed(attrs, "height").and_then(|s| s.parse::<u32>().ok());

    if let Some(src) = get_trimmed(attrs, "src") {
        return match validate_https_url(&src) {
            Some(valid) => Action::FetchImage { src: valid, alt, width, height },
            None => Action::Unknown(tag_name.to_string()),
        };
    }
    if let Some(id) = get_trimmed(attrs, "id") {
        if !id.is_empty() {
            return Action::LookupImage { id, alt, width, height };
        }
    }
    Action::Unknown(tag_name.to_string())
}

fn hyperlink_action(
    tag_name: &str,
    attrs: &HashMap<String, String>,
    content: Option<&str>,
) -> Action {
    let href = match get_trimmed(attrs, "href").and_then(|h| validate_https_url(&h)) {
        Some(h) => h,
        None => return Action::Unknown(tag_name.to_string()),
    };
    let key = get_trimmed(attrs, "key").filter(|s| !s.is_empty());
    let raw_label = content.map(str::trim).unwrap_or("");
    let label = if raw_label.is_empty() {
        // Fallback: show the URL itself if no link text was provided.
        href.clone()
    } else {
        raw_label.to_string()
    };
    Action::Hyperlink {
        href,
        key,
        label: truncate(&label, MAX_TEXT_LEN),
    }
}

fn dismiss_action(tag_name: &str, attrs: &HashMap<String, String>) -> Action {
    let key = match get_trimmed(attrs, "key").filter(|s| !s.is_empty()) {
        Some(k) => k,
        None => return Action::Unknown(tag_name.to_string()),
    };
    let label = get_trimmed(attrs, "label")
        .filter(|s| !s.is_empty())
        .map(|s| truncate(&s, MAX_TEXT_LEN));
    Action::Dismiss { key, label }
}

/// Look up an attribute case-sensitively (UE4 markup is case-sensitive on
/// attribute names) and return its trimmed value if non-empty after trim.
fn get_trimmed(attrs: &HashMap<String, String>, key: &str) -> Option<String> {
    attrs.get(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Reject anything that isn't a well-formed `https://` URL. Returns the
/// trimmed URL string when valid; `None` otherwise. We deliberately reject
/// `http://` (cleartext fetches are a security concern in a game-process
/// detour), `file://`, `javascript:`, etc.
fn validate_https_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parsed = Url::parse(trimmed).ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    if parsed.host_str().map(str::is_empty).unwrap_or(true) {
        return None;
    }
    Some(trimmed.to_string())
}

/// Truncate a string to `max` chars, appending an ellipsis if it had to
/// be cut. Counts Unicode scalar values, not bytes — safe with non-ASCII.
fn truncate(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    // Reserve room for the suffix. Saturating_sub guards against `max=0`.
    let keep = max.saturating_sub(TRUNCATE_SUFFIX.chars().count());
    let mut out: String = s.chars().take(keep).collect();
    out.push_str(TRUNCATE_SUFFIX);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn img_with_https_src_fetches() {
        let a = attrs(&[("src", "https://placehold.co/200x100"), ("alt", "demo"), ("width", "200"), ("height", "100")]);
        match dispatch("img", &a, None) {
            Action::FetchImage { src, alt, width, height } => {
                assert_eq!(src, "https://placehold.co/200x100");
                assert_eq!(alt.as_deref(), Some("demo"));
                assert_eq!(width, Some(200));
                assert_eq!(height, Some(100));
            }
            other => panic!("expected FetchImage, got {other:?}"),
        }
    }

    #[test]
    fn img_with_id_does_datatable_lookup() {
        let a = attrs(&[("id", "ServerLogo"), ("alt", "logo")]);
        match dispatch("img", &a, None) {
            Action::LookupImage { id, alt, .. } => {
                assert_eq!(id, "ServerLogo");
                assert_eq!(alt.as_deref(), Some("logo"));
            }
            other => panic!("expected LookupImage, got {other:?}"),
        }
    }

    #[test]
    fn img_src_wins_over_id_when_both_present() {
        let a = attrs(&[("src", "https://example.com/x.png"), ("id", "Fallback")]);
        assert!(matches!(dispatch("img", &a, None), Action::FetchImage { .. }));
    }

    #[test]
    fn img_without_src_or_id_is_unknown() {
        let a = attrs(&[("alt", "stranded")]);
        assert_eq!(dispatch("img", &a, None), Action::Unknown("img".to_string()));
    }

    #[test]
    fn img_http_scheme_rejected() {
        let a = attrs(&[("src", "http://insecure.example/x.png")]);
        assert_eq!(dispatch("img", &a, None), Action::Unknown("img".to_string()));
    }

    #[test]
    fn img_garbage_url_rejected() {
        let a = attrs(&[("src", "not a url at all !!")]);
        assert_eq!(dispatch("img", &a, None), Action::Unknown("img".to_string()));
        let a = attrs(&[("src", "javascript:alert(1)")]);
        assert_eq!(dispatch("img", &a, None), Action::Unknown("img".to_string()));
        let a = attrs(&[("src", "file:///c:/secret.txt")]);
        assert_eq!(dispatch("img", &a, None), Action::Unknown("img".to_string()));
    }

    #[test]
    fn banner_wraps_image_action() {
        let a = attrs(&[("src", "https://cdn.example.com/hero.jpg")]);
        match dispatch("banner", &a, None) {
            Action::HoistBanner(inner) => {
                assert!(matches!(*inner, Action::FetchImage { .. }));
            }
            other => panic!("expected HoistBanner, got {other:?}"),
        }
    }

    #[test]
    fn banner_with_id_wraps_lookup() {
        let a = attrs(&[("id", "WelcomeHero")]);
        match dispatch("banner", &a, None) {
            Action::HoistBanner(inner) => {
                assert!(matches!(*inner, Action::LookupImage { .. }));
            }
            other => panic!("expected HoistBanner, got {other:?}"),
        }
    }

    #[test]
    fn banner_without_attrs_is_unknown() {
        let a = attrs(&[]);
        assert_eq!(dispatch("banner", &a, None), Action::Unknown("banner".to_string()));
    }

    #[test]
    fn hyperlink_with_https_href_and_label() {
        let a = attrs(&[("href", "https://discord.gg/turdmod"), ("key", "F1")]);
        match dispatch("a", &a, Some("Join Discord")) {
            Action::Hyperlink { href, key, label } => {
                assert_eq!(href, "https://discord.gg/turdmod");
                assert_eq!(key.as_deref(), Some("F1"));
                assert_eq!(label, "Join Discord");
            }
            other => panic!("expected Hyperlink, got {other:?}"),
        }
    }

    #[test]
    fn hyperlink_without_label_falls_back_to_href() {
        let a = attrs(&[("href", "https://example.com")]);
        match dispatch("a", &a, None) {
            Action::Hyperlink { label, .. } => assert_eq!(label, "https://example.com"),
            other => panic!("expected Hyperlink, got {other:?}"),
        }
    }

    #[test]
    fn hyperlink_http_rejected() {
        let a = attrs(&[("href", "http://insecure.example/")]);
        assert_eq!(dispatch("a", &a, Some("x")), Action::Unknown("a".to_string()));
    }

    #[test]
    fn hyperlink_missing_href_is_unknown() {
        let a = attrs(&[("key", "F1")]);
        assert_eq!(dispatch("a", &a, Some("nope")), Action::Unknown("a".to_string()));
    }

    #[test]
    fn dismiss_with_key_and_label() {
        let a = attrs(&[("key", "Escape"), ("label", "Got it")]);
        match dispatch("dismiss", &a, None) {
            Action::Dismiss { key, label } => {
                assert_eq!(key, "Escape");
                assert_eq!(label.as_deref(), Some("Got it"));
            }
            other => panic!("expected Dismiss, got {other:?}"),
        }
    }

    #[test]
    fn dismiss_without_key_is_unknown() {
        let a = attrs(&[("label", "Got it")]);
        assert_eq!(dispatch("dismiss", &a, None), Action::Unknown("dismiss".to_string()));
    }

    #[test]
    fn dismiss_with_only_key_keeps_label_none() {
        let a = attrs(&[("key", "Escape")]);
        match dispatch("dismiss", &a, None) {
            Action::Dismiss { key, label } => {
                assert_eq!(key, "Escape");
                assert!(label.is_none());
            }
            other => panic!("expected Dismiss, got {other:?}"),
        }
    }

    #[test]
    fn title_uses_inner_content() {
        let a = attrs(&[]);
        assert_eq!(
            dispatch("title", &a, Some("Welcome to TurdMOD")),
            Action::HoistTitle("Welcome to TurdMOD".to_string()),
        );
    }

    #[test]
    fn title_without_content_is_unknown() {
        let a = attrs(&[]);
        assert_eq!(dispatch("title", &a, None), Action::Unknown("title".to_string()));
        assert_eq!(dispatch("title", &a, Some("   ")), Action::Unknown("title".to_string()));
    }

    #[test]
    fn section_with_title_attr() {
        let a = attrs(&[("title", "House Rules")]);
        assert_eq!(dispatch("section", &a, None), Action::SectionStart("House Rules".to_string()));
    }

    #[test]
    fn section_without_title_is_unknown() {
        let a = attrs(&[]);
        assert_eq!(dispatch("section", &a, None), Action::Unknown("section".to_string()));
    }

    #[test]
    fn unknown_tag_returns_unknown() {
        let a = attrs(&[("foo", "bar")]);
        assert_eq!(dispatch("blink", &a, None), Action::Unknown("blink".to_string()));
        // Style.* is handled upstream; if it leaks through we still return Unknown.
        assert_eq!(dispatch("Style.HeroTagline", &a, Some("x")), Action::Unknown("Style.HeroTagline".to_string()));
    }

    #[test]
    fn tag_name_is_case_insensitive() {
        let a = attrs(&[("src", "https://example.com/x.png")]);
        assert!(matches!(dispatch("IMG", &a, None), Action::FetchImage { .. }));
        assert!(matches!(dispatch("Img", &a, None), Action::FetchImage { .. }));
        let a = attrs(&[("key", "Escape")]);
        assert!(matches!(dispatch("DISMISS", &a, None), Action::Dismiss { .. }));
    }

    #[test]
    fn whitespace_in_attrs_is_trimmed() {
        let a = attrs(&[("src", "  https://example.com/x.png  "), ("alt", "  hello  ")]);
        match dispatch("img", &a, None) {
            Action::FetchImage { src, alt, .. } => {
                assert_eq!(src, "https://example.com/x.png");
                assert_eq!(alt.as_deref(), Some("hello"));
            }
            other => panic!("expected FetchImage, got {other:?}"),
        }
    }

    #[test]
    fn empty_or_whitespace_attr_treated_as_missing() {
        let a = attrs(&[("src", "   "), ("id", "")]);
        assert_eq!(dispatch("img", &a, None), Action::Unknown("img".to_string()));
    }

    #[test]
    fn long_label_is_truncated_with_ellipsis() {
        let long = "x".repeat(500);
        let a = attrs(&[("href", "https://example.com")]);
        match dispatch("a", &a, Some(&long)) {
            Action::Hyperlink { label, .. } => {
                assert_eq!(label.chars().count(), MAX_TEXT_LEN);
                assert!(label.ends_with(TRUNCATE_SUFFIX));
            }
            other => panic!("expected Hyperlink, got {other:?}"),
        }
    }

    #[test]
    fn long_alt_is_truncated() {
        let long_alt = "y".repeat(400);
        let a = attrs(&[("src", "https://example.com/x.png"), ("alt", long_alt.as_str())]);
        match dispatch("img", &a, None) {
            Action::FetchImage { alt, .. } => {
                let alt = alt.expect("alt should be set");
                assert_eq!(alt.chars().count(), MAX_TEXT_LEN);
                assert!(alt.ends_with(TRUNCATE_SUFFIX));
            }
            other => panic!("expected FetchImage, got {other:?}"),
        }
    }

    #[test]
    fn short_text_not_truncated() {
        let a = attrs(&[("href", "https://example.com")]);
        match dispatch("a", &a, Some("Short")) {
            Action::Hyperlink { label, .. } => {
                assert_eq!(label, "Short");
                assert!(!label.ends_with(TRUNCATE_SUFFIX));
            }
            other => panic!("expected Hyperlink, got {other:?}"),
        }
    }

    #[test]
    fn invalid_width_height_ignored_gracefully() {
        let a = attrs(&[("src", "https://example.com/x.png"), ("width", "not-a-number"), ("height", "  ")]);
        match dispatch("img", &a, None) {
            Action::FetchImage { width, height, .. } => {
                assert_eq!(width, None);
                assert_eq!(height, None);
            }
            other => panic!("expected FetchImage, got {other:?}"),
        }
    }
}
