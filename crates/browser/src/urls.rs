//! Petites primitives URL, sans dépendance externe.

/// Sépare host et chemin d'une URL.
/// `"https://user@a.b:8080/x/y?z"` → `("a.b", "x/y?z")`
pub fn host_and_path(url: &str) -> (&str, &str) {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, h)| h)
        .split(':')
        .next()
        .unwrap_or_default();
    (host, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraction_host_et_chemin() {
        assert_eq!(host_and_path("https://a.b/x/y?z"), ("a.b", "x/y?z"));
        assert_eq!(host_and_path("http://user@a.b:8080/x"), ("a.b", "x"));
        assert_eq!(host_and_path("a.b"), ("a.b", ""));
        assert_eq!(host_and_path("https://a.b"), ("a.b", ""));
    }
}
