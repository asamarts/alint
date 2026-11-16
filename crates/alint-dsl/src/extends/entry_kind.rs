//! How an `extends:` entry loads, by its scheme.

/// How an `extends:` entry loads, by its scheme. A URL scheme matches in any
/// case (RFC 3986), so `HTTPS://` is a remote entry: SRI-pinned, fetched, and
/// shown without its credentials and query, never read as a local path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryKind<'a> {
    /// `http://`, which is refused.
    PlainHttp,
    /// `https://`, fetched and checked against its `#sha256-` pin.
    Https,
    /// `alint://bundled/<name>@<rev>`, with the `<name>@<rev>` part.
    Bundled(&'a str),
    /// Any other `<scheme>://` (`ftp://`, `file://`, `ssh://`,
    /// `git+https://`, an `alint://` that is not `alint://bundled/`), with
    /// the scheme as written: a load error, never a local path.
    Unsupported(&'a str),
    /// Anything else, a path relative to the extending config.
    Local,
}

impl<'a> EntryKind<'a> {
    const BUNDLED_PREFIX: &'static str = "alint://bundled/";

    pub(crate) fn of(entry: &'a str) -> Self {
        let has_prefix = |prefix: &str| {
            entry
                .get(..prefix.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        };
        if has_prefix("http://") {
            Self::PlainHttp
        } else if has_prefix("https://") {
            Self::Https
        } else if has_prefix(Self::BUNDLED_PREFIX) {
            Self::Bundled(&entry[Self::BUNDLED_PREFIX.len()..])
        } else if let Some(scheme) = url_scheme(entry) {
            Self::Unsupported(scheme)
        } else {
            Self::Local
        }
    }

    /// Whether the entry names a URL (alint shows one without its
    /// credentials, query and fragment).
    pub(crate) fn is_url(self) -> bool {
        !matches!(self, Self::Local)
    }
}

/// The scheme of `entry` when it starts as a URL does, `<scheme>://` with an
/// RFC 3986 scheme (a letter, then letters, digits, `+`, `-` or `.`). A
/// one-letter scheme is a Windows drive (`C://dir`), never a URL.
fn url_scheme(entry: &str) -> Option<&str> {
    let (scheme, _) = entry.split_once("://")?;
    let mut chars = scheme.chars();
    let valid = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    (valid && scheme.len() > 1).then_some(scheme)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_is_read_by_its_scheme_in_any_case() {
        assert_eq!(EntryKind::of("HTTPS://h/x.yml"), EntryKind::Https);
        assert_eq!(EntryKind::of("Http://h/x.yml"), EntryKind::PlainHttp);
        assert_eq!(
            EntryKind::of("ALINT://Bundled/oss@v1"),
            EntryKind::Bundled("oss@v1")
        );
        for (entry, scheme) in [
            ("ftp://h/x.yml", "ftp"),
            ("FILE:///etc/x.yml", "FILE"),
            ("ssh://git@h/x.yml", "ssh"),
            ("git+https://h/x.yml", "git+https"),
            ("alint://elsewhere/x", "alint"),
        ] {
            assert_eq!(EntryKind::of(entry), EntryKind::Unsupported(scheme));
        }
        for local in [
            "rules.yml",
            "./a://b.yml",
            "C://rules.yml",
            "1ab://x.yml",
            "dir/ftp://x",
        ] {
            assert_eq!(EntryKind::of(local), EntryKind::Local, "{local}");
        }
    }
}
