use aethershell_core::BindingValue;
use std::collections::HashMap;

/// Formats text templates containing `${source:format}` binding expressions (PRD Section 10.5).
pub struct BindingFormatter;

impl BindingFormatter {
    /// Evaluates a template string by substituting placeholders from the bindings map.
    pub fn format_template(
        template: &str,
        bindings: &HashMap<String, BindingValue>,
        fallback: &str,
    ) -> String {
        let mut result = String::with_capacity(template.len());
        let mut chars = template.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '$' && chars.peek() == Some(&'{') {
                chars.next(); // Consume '{'
                let mut expr = String::new();
                let mut closed = false;

                for inner in chars.by_ref() {
                    if inner == '}' {
                        closed = true;
                        break;
                    }
                    expr.push(inner);
                }

                if closed {
                    let (key, format_spec) = match expr.split_once(':') {
                        Some((k, fmt)) => (k.trim(), Some(fmt.trim())),
                        None => (expr.trim(), None),
                    };

                    let formatted = match bindings.get(key) {
                        Some(val) => val.format_with(format_spec, fallback),
                        None => fallback.to_string(),
                    };
                    result.push_str(&formatted);
                } else {
                    // Unclosed expression, write literal
                    result.push_str("${");
                    result.push_str(&expr);
                }
            } else {
                result.push(c);
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_template_substitution() {
        let mut bindings = HashMap::new();
        bindings.insert("metrics.cpu".into(), BindingValue::Percent(45.2));
        bindings.insert(
            "metrics.ram".into(),
            BindingValue::Bytes(6 * 1024 * 1024 * 1024),
        ); // 6 GiB

        let template = "CPU ${metrics.cpu:0.0}% | RAM ${metrics.ram:0.1} GiB";
        let rendered = BindingFormatter::format_template(template, &bindings, "—");
        assert_eq!(rendered, "CPU 45% | RAM 6.0 GiB");
    }

    #[test]
    fn test_format_template_missing_binding_fallback() {
        let bindings = HashMap::new();
        let template = "GPU ${metrics.gpu:0.0}%";
        let rendered = BindingFormatter::format_template(template, &bindings, "—");
        assert_eq!(rendered, "GPU —%");
    }
}
