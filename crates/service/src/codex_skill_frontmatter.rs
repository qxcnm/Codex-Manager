use serde::Deserialize;

#[derive(Deserialize)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
}

pub(super) fn parse(content: &str) -> Result<(String, String), String> {
    let normalized = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut lines = normalized.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Err("SKILL.md is missing YAML frontmatter".to_string());
    }
    let mut frontmatter = Vec::new();
    let mut closed = false;
    for line in lines {
        if line.trim() == "---" {
            closed = true;
            break;
        }
        frontmatter.push(line);
    }
    if !closed {
        return Err("SKILL.md frontmatter is not closed".to_string());
    }
    let yaml = frontmatter.join("\n");
    let parsed: Frontmatter = serde_yaml::from_str(&yaml)
        .or_else(|original_error| {
            // Match Codex's bounded compatibility repair for prose containing `: `.
            // Parse the entire document so nested metadata never becomes a top-level field.
            match repair_scalar_fields(&yaml) {
                Some(repaired) => serde_yaml::from_str(&repaired).map_err(|_| original_error),
                None => Err(original_error),
            }
        })
        .map_err(|error| format!("SKILL.md has invalid YAML frontmatter: {error}"))?;
    let name = required_single_line(parsed.name, "name")?;
    if name.chars().count() > 64 {
        return Err("SKILL.md name exceeds the Codex limit of 64 characters".to_string());
    }
    let description = required_single_line(parsed.description, "description")?;
    Ok((name, description))
}

fn required_single_line(value: Option<String>, field: &str) -> Result<String, String> {
    value
        .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("SKILL.md frontmatter is missing {field}"))
}

fn repair_scalar_fields(frontmatter: &str) -> Option<String> {
    let mut changed = false;
    let mut block_indent = None;
    let mut repaired = Vec::new();
    for line in frontmatter.lines() {
        let indent = line
            .chars()
            .take_while(|character| *character == ' ')
            .count();
        if let Some(block_indent_value) = block_indent {
            if line.trim().is_empty() || indent > block_indent_value {
                repaired.push(line.to_string());
                continue;
            }
            block_indent = None;
        }
        let Some((key, value)) = line.split_once(':') else {
            repaired.push(line.to_string());
            continue;
        };
        if key.trim().is_empty() || !value.chars().next().is_none_or(char::is_whitespace) {
            repaired.push(line.to_string());
            continue;
        }
        let trimmed = value.trim_start();
        let leading = &value[..value.len() - trimmed.len()];
        let mut scalar = trimmed;
        let mut comment = "";
        for (index, character) in trimmed.char_indices() {
            if character == '#'
                && (index == 0
                    || trimmed[..index]
                        .chars()
                        .next_back()
                        .is_some_and(char::is_whitespace))
            {
                let comment_start = trimmed[..index].trim_end().len();
                scalar = &trimmed[..comment_start];
                comment = &trimmed[comment_start..];
                break;
            }
        }
        let scalar = scalar.trim_end();
        let Some(first) = scalar.chars().next() else {
            repaired.push(line.to_string());
            continue;
        };
        if matches!(first, '|' | '>') {
            block_indent = Some(indent);
            repaired.push(line.to_string());
            continue;
        }
        if matches!(first, '\'' | '"') {
            repaired.push(line.to_string());
            continue;
        }
        let colon_separator = scalar.char_indices().any(|(index, character)| {
            character == ':' && scalar[index + 1..].starts_with(char::is_whitespace)
        });
        let invalid_flow_scalar = matches!(first, '[' | '{' | '@' | '`')
            && serde_yaml::from_str::<serde_yaml::Value>(scalar).is_err();
        if !colon_separator && !invalid_flow_scalar {
            repaired.push(line.to_string());
            continue;
        }
        let quoted = format!("'{}'", scalar.replace('\'', "''"));
        repaired.push(format!("{key}:{leading}{quoted}{comment}"));
        changed = true;
    }
    changed.then(|| repaired.join("\n"))
}
