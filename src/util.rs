pub fn sanitize_name(s: &str) -> Option<String> {
    if s.is_empty() || s.len() > 80 {
        return None;
    }
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        Some(s.to_string())
    } else {
        None
    }
}
