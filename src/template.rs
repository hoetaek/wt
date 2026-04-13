use regex::Regex;
use std::collections::HashMap;

/// Replace `{{key}}` placeholders with values from the context map.
/// Unknown keys are left as-is.
pub fn render(template: &str, vars: &HashMap<String, String>) -> String {
    let re = Regex::new(r"\{\{(\w+)\}\}").unwrap();
    re.replace_all(template, |caps: &regex::Captures| {
        let key = &caps[1];
        match vars.get(key) {
            Some(val) => val.clone(),
            None => caps[0].to_string(),
        }
    })
    .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn substitutes_single_variable() {
        let result = render(
            "https://{{site_name}}.test",
            &vars(&[("site_name", "hapjeong-tech-680")]),
        );
        assert_eq!(result, "https://hapjeong-tech-680.test");
    }

    #[test]
    fn substitutes_multiple_variables() {
        let result = render(
            "{{repo}}-{{tech_id}}",
            &vars(&[("repo", "hapjeong"), ("tech_id", "tech-680")]),
        );
        assert_eq!(result, "hapjeong-tech-680");
    }

    #[test]
    fn leaves_unknown_variables_untouched() {
        let result = render("{{unknown}}", &vars(&[]));
        assert_eq!(result, "{{unknown}}");
    }

    #[test]
    fn handles_no_variables() {
        let result = render("plain text", &vars(&[("key", "val")]));
        assert_eq!(result, "plain text");
    }

    #[test]
    fn handles_adjacent_variables() {
        let result = render("{{a}}{{b}}", &vars(&[("a", "hello"), ("b", "world")]));
        assert_eq!(result, "helloworld");
    }

    #[test]
    fn handles_variable_with_spaces_in_braces() {
        let result = render("{{ spaced }}", &vars(&[("spaced", "val")]));
        assert_eq!(result, "{{ spaced }}");
    }
}
