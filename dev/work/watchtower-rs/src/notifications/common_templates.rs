#![forbid(unsafe_code)]

/// The legacy provider templates bundled with Watchtower.
pub const COMMON_TEMPLATES: &[(&str, &str)] = &[
    (
        "default-legacy",
        "{{range .}}{{.Message}}{{println}}{{end}}",
    ),
    (
        "default",
        r#"
{{- if .Report -}}
  {{- with .Report -}}
    {{- if ( or .Updated .Failed ) -}}
{{len .Scanned}} Scanned, {{len .Updated}} Updated, {{len .Failed}} Failed
      {{- range .Updated}}
- {{.Name}} ({{.ImageName}}): {{.CurrentImageID.ShortID}} updated to {{.LatestImageID.ShortID}}
      {{- end -}}
      {{- range .Fresh}}
- {{.Name}} ({{.ImageName}}): {{.State}}
	  {{- end -}}
	  {{- range .Skipped}}
- {{.Name}} ({{.ImageName}}): {{.State}}: {{.Error}}
	  {{- end -}}
	  {{- range .Failed}}
- {{.Name}} ({{.ImageName}}): {{.State}}: {{.Error}}
	  {{- end -}}
    {{- end -}}
  {{- end -}}
{{- else -}}
  {{range .Entries -}}{{.Message}}{{"\n"}}{{- end -}}
{{- end -}}"#,
    ),
    (
        "porcelain.v1.summary-no-log",
        r#"
{{- if .Report -}}
  {{- range .Report.All }}
    {{- .Name}} ({{.ImageName}}): {{.State -}}
    {{- with .Error}} Error: {{.}}{{end}}{{ println }}
  {{- else -}}
    no containers matched filter
  {{- end -}}
{{- end -}}"#,
    ),
    ("json.v1", "{{ . | ToJSON }}"),
];

/// Resolve a common template by name.
pub fn common_template(name: &str) -> Option<&'static str> {
    COMMON_TEMPLATES
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, template)| *template)
}

/// Return the built-in template selected by the `legacy` flag.
pub fn default_template(legacy: bool) -> &'static str {
    if legacy {
        common_template("default-legacy").expect("default-legacy template exists")
    } else {
        common_template("default").expect("default template exists")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_templates_cover_the_legacy_names() {
        assert_eq!(
            common_template("default-legacy"),
            Some("{{range .}}{{.Message}}{{println}}{{end}}")
        );
        assert_eq!(
            default_template(true),
            "{{range .}}{{.Message}}{{println}}{{end}}"
        );
        assert!(common_template("missing").is_none());
    }
}
