use regex_lite::Regex;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TemplateError {
    #[error("invalid placeholder syntax at position {0}")]
    InvalidPlaceholder(usize),
}

/// Render a simple template supporting positional placeholders `$1..$9` and
/// aggregate placeholders `$*` and `$ARGUMENTS`.
///
/// - Unset positional placeholders are replaced with an empty string.
/// - `$*` and `$ARGUMENTS` expand to all arguments joined by a single space.
/// - Escaping is not currently supported; to render a literal `$`, prefer `$$` in
///   the future when escaping is implemented.
pub fn render_template(template: &str, args: &[String]) -> Result<String, TemplateError> {
    // Fast path: no dollar sign → return as-is.
    if !template.contains('$') {
        return Ok(template.to_string());
    }

    // Replace aggregate placeholders first.
    let all = args.join(" ");
    let out = template.replace("$*", &all).replace("$ARGUMENTS", &all);

    // Replace positional placeholders using a regex.
    // Note: `$10` is treated as `$1` followed by `0` to match common shell semantics.
    let re = Regex::new(r"\$(?P<idx>[1-9])").unwrap();
    let mut last_end = 0;
    let mut rendered = String::with_capacity(out.len());
    for caps in re
        .captures_iter(&out)
        .map(|c| (c.get(0).unwrap(), c["idx"].parse::<usize>().unwrap()))
    {
        let (m, idx) = caps;
        let start = m.start();
        let end = m.end();
        // Copy preceding chunk
        if start > last_end {
            rendered.push_str(&out[last_end..start]);
        }
        // Insert arg if available
        let repl = args.get(idx - 1).map(|s| s.as_str()).unwrap_or("");
        rendered.push_str(repl);
        last_end = end;
    }
    rendered.push_str(&out[last_end..]);
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_positional_and_aggregate() {
        let tpl = "Hello $1, all: $* / $ARGUMENTS and $9.";
        let args = vec!["Alice".into(), "B".into(), "C".into()];
        let out = render_template(tpl, &args).unwrap();
        assert_eq!(out, "Hello Alice, all: Alice B C / Alice B C and .");
    }

    #[test]
    fn leaves_unrelated_text() {
        let tpl = "No placeholders here.";
        assert_eq!(render_template(tpl, &[]).unwrap(), tpl);
    }
}
