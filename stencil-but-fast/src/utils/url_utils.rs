use regex::Regex;
use url::Url;

/// Strip domain from Set-Cookie headers for local dev
pub fn strip_domain_from_cookies(cookies: &[String]) -> Vec<String> {
    let domain_re = Regex::new(r"(?i)(?:;\s)?domain=(?:.+?)(;|$)").unwrap();
    let samesite_re = Regex::new(r"(?i); SameSite=none").unwrap();

    cookies
        .iter()
        .map(|c| {
            let stripped = domain_re.replace_all(c, "$1").to_string();
            samesite_re.replace_all(&stripped, "").to_string()
        })
        .collect()
}

/// Strip domain from redirect URL if it matches the store, otherwise leave it alone
pub fn normalize_redirect_url(redirect_url: &str, normal_store_url: &str, store_url: &str) -> String {
    if redirect_url.is_empty() || !redirect_url.starts_with("http") {
        return redirect_url.to_string();
    }

    let store_host = Url::parse(normal_store_url)
        .ok()
        .and_then(|u| u.host_str().map(String::from))
        .unwrap_or_default();
    let secure_store_host = Url::parse(store_url)
        .ok()
        .and_then(|u| u.host_str().map(String::from))
        .unwrap_or_default();

    if let Ok(redirect) = Url::parse(redirect_url) {
        let redirect_host = redirect.host_str().unwrap_or_default().to_string();
        if redirect_host == store_host || redirect_host == secure_store_host {
            // Strip to just path + query + fragment
            let path = redirect.path().to_string();
            let query = redirect.query().map(|q| format!("?{}", q)).unwrap_or_default();
            let fragment = redirect.fragment().map(|f| format!("#{}", f)).unwrap_or_default();
            return format!("{}{}{}", path, query, fragment);
        }
    }

    redirect_url.to_string()
}
