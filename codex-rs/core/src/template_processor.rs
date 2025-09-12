use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TemplateError {
    #[error("invalid placeholder syntax at position {0}")]
    InvalidPlaceholder(usize),
    #[error("rendered output exceeds limit of {0} bytes")]
    OutputTooLarge(usize),
}

/// Render a simple template supporting positional placeholders `$1..$9` and
/// aggregate placeholders `$*` and `$ARGUMENTS`.
///
/// - Unset positional placeholders are replaced with an empty string.
/// - `$*` and `$ARGUMENTS` expand to all arguments joined by a single space.
/// - Escaping is not currently supported; to render a literal `$`, prefer `$$` in
///   the future when escaping is implemented.
pub fn render_template(template: &str, args: &[String]) -> Result<String, TemplateError> {
    const MAX_BYTES: usize = 64 * 1024;
    if !template.contains('$') {
        return Ok(template.to_string());
    }
    let all = args.join(" ");

    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '$' {
            out.push(ch);
            continue;
        }
        // $$ escape → $
        if matches!(chars.peek(), Some('$')) {
            chars.next();
            out.push('$');
            continue;
        }
        // $* / $ARGUMENTS
        if matches!(chars.peek(), Some('*')) {
            chars.next();
            out.push_str(&all);
            continue;
        }
        if str_peek_consume(&mut chars, "ARGUMENTS") {
            out.push_str(&all);
            continue;
        }
        // ${N} (multi-digit index)
        if matches!(chars.peek(), Some('{')) {
            chars.next(); // consume '{'
            let mut num = String::new();
            while let Some(c) = chars.peek().copied() {
                if c.is_ascii_digit() {
                    num.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            if matches!(chars.peek(), Some('}')) {
                chars.next();
                if let Ok(idx) = num.parse::<usize>()
                    && idx >= 1 {
                        if let Some(val) = args.get(idx - 1) {
                            out.push_str(val);
                        }
                        continue;
                    }
            }
            // Fallback: literal
            out.push_str("${");
            out.push_str(&num);
            if matches!(chars.peek(), Some('}')) {
                out.push('}');
                let _ = chars.next();
            }
            continue;
        }
        // $1..$9 (note: $10 => $1 + '0')
        if let Some(d) = chars.peek().copied()
            && d.is_ascii_digit() && d != '0' {
                let idx = d as usize - '0' as usize;
                chars.next();
                if let Some(val) = args.get(idx - 1) {
                    out.push_str(val);
                }
                continue;
            }
        // Unknown placeholder: keep as-is ($X → $X)
        out.push('$');
        if let Some(nc) = chars.peek().copied() {
            out.push(nc);
            chars.next();
        }
        if out.len() > MAX_BYTES {
            return Err(TemplateError::OutputTooLarge(MAX_BYTES));
        }
    }
    if out.len() > MAX_BYTES {
        return Err(TemplateError::OutputTooLarge(MAX_BYTES));
    }
    Ok(out)
}

fn str_peek_consume(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, key: &str) -> bool {
    let mut clone = chars.clone();
    for kch in key.chars() {
        match clone.next() {
            Some(c) if c == kch => {}
            _ => return false,
        }
    }
    // commit
    for _ in 0..key.len() {
        let _ = chars.next();
    }
    true
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

    #[test]
    fn supports_dollar_escape_and_multi_digit() {
        let tpl = "Cost: $$5; tenth=${10}; plain=$10";
        let args = vec![
            "one".into(),
            "two".into(),
            "three".into(),
            "four".into(),
            "five".into(),
            "six".into(),
            "seven".into(),
            "eight".into(),
            "nine".into(),
            "TEN".into(),
        ];
        let out = render_template(tpl, &args).unwrap();
        assert_eq!(out, "Cost: $5; tenth=TEN; plain=one0");
    }

    #[test]
    fn output_cap_errors() {
        let tpl = "$*";
        let big = vec!["x".repeat(70 * 1024)];
        let err = render_template(tpl, &big).unwrap_err();
        assert!(matches!(err, TemplateError::OutputTooLarge(_)));
    }
}
