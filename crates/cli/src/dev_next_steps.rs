//! Pure helper for rendering the `owasidea dev` next-steps block (DL-07).

/// Render the "next steps" block printed at the end of `owasidea dev`.
///
/// `stale_bundle` indicates the UI bundle build id differs from the host build
/// id (RF-08); when true a clear line is included instead of a loose warning.
pub fn dev_next_steps(
    ui_url: Option<&str>,
    host_url: &str,
    plugins: &[String],
    stale_bundle: bool,
) -> String {
    let mut out = String::new();
    out.push_str("Next steps:\n");
    if let Some(u) = ui_url {
        out.push_str(&format!("  UI:   {u}\n"));
    }
    out.push_str(&format!("  Host: {host_url}\n"));
    if stale_bundle {
        out.push_str("  Bundle: STALE (UI build id != host build id) — run `owasidea build`\n");
    } else {
        out.push_str("  Bundle: up to date\n");
    }
    let example = plugins
        .first()
        .cloned()
        .unwrap_or_else(|| "plugin".to_string());
    out.push_str(&format!(
        "  Try:  owasidea call {example}.greet \"hello\"\n"
    ));
    out.push_str("        owasidea logs <plugin> --follow\n");
    out.push_str("        owasidea status\n");
    out.push_str("  Docs: docs/CLI.md\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_block() {
        let s = dev_next_steps(
            Some("http://127.0.0.1:5173"),
            "http://127.0.0.1:8787",
            &["notes".to_string()],
            false,
        );
        assert!(s.contains("UI:   http://127.0.0.1:5173"));
        assert!(s.contains("Host: http://127.0.0.1:8787"));
        assert!(s.contains("owasidea call notes.greet"));
        assert!(s.contains("owasidea logs <plugin> --follow"));
        assert!(s.contains("owasidea status"));
    }

    #[test]
    fn stale_line() {
        let s = dev_next_steps(None, "http://127.0.0.1:8787", &[], true);
        assert!(s.contains("STALE"));
    }
}
