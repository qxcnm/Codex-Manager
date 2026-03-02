use serde_json::Value;

pub(super) fn retain_fields_with_allowlist(
    obj: &mut serde_json::Map<String, Value>,
    allow: fn(&str) -> bool,
) -> Vec<String> {
    let dropped = obj
        .keys()
        .filter(|key| !allow(key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if dropped.is_empty() {
        return dropped;
    }
    obj.retain(|key, _| allow(key.as_str()));
    dropped
}

pub(super) fn normalize_path(path: &str) -> &str {
    path.split('?').next().unwrap_or(path)
}

pub(super) fn path_matches_template(path: &str, template: &str) -> bool {
    let normalized_path = normalize_path(path);
    let mut path_segments = normalized_path
        .trim_end_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty());
    let mut template_segments = template
        .trim_end_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty());

    loop {
        match (template_segments.next(), path_segments.next()) {
            (None, None) => return true,
            (Some(_), None) | (None, Some(_)) => return false,
            (Some(template_segment), Some(path_segment)) => {
                if template_segment.starts_with('{') && template_segment.ends_with('}') {
                    if path_segment.is_empty() {
                        return false;
                    }
                    continue;
                }
                if template_segment != path_segment {
                    return false;
                }
            }
        }
    }
}

pub(super) struct TemplateAllowlist {
    pub(super) template: &'static str,
    pub(super) allow: fn(&str) -> bool,
}

pub(super) fn retain_fields_by_templates(
    path: &str,
    obj: &mut serde_json::Map<String, Value>,
    templates: &[TemplateAllowlist],
) -> Vec<String> {
    for template in templates {
        if path_matches_template(path, template.template) {
            return retain_fields_with_allowlist(obj, template.allow);
        }
    }
    Vec::new()
}
