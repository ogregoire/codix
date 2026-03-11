use std::path::{Path, PathBuf};
use anyhow::Result;

pub fn config_path(root: &Path) -> PathBuf {
    root.join(".codix/config")
}

pub fn read_value(root: &Path, section: &str, key: &str) -> Result<Option<String>> {
    let path = config_path(root);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let target_header = format!("[{}]", section);
    let mut in_section = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed == target_header;
            continue;
        }
        if in_section {
            if let Some((k, v)) = parse_kv(trimmed) {
                if k == key {
                    return Ok(Some(v.to_string()));
                }
            }
        }
    }
    Ok(None)
}

pub fn write_value(root: &Path, section: &str, key: &str, value: &str) -> Result<()> {
    let path = config_path(root);
    let target_header = format!("[{}]", section);
    let new_line = format!("{} = {}", key, value);

    if !path.exists() {
        std::fs::write(&path, format!("{}\n{}\n", target_header, new_line))?;
        return Ok(());
    }

    let content = std::fs::read_to_string(&path)?;
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut in_section = false;
    let mut replaced = false;

    for (i, line) in lines.iter_mut().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_section && !replaced {
                // Section ended without finding the key — insert before this line
                lines.insert(i, new_line.clone());
                replaced = true;
                break;
            }
            in_section = trimmed == target_header;
            continue;
        }
        if in_section && !replaced {
            if let Some((k, _)) = parse_kv(trimmed) {
                if k == key {
                    *line = new_line.clone();
                    replaced = true;
                }
            }
        }
    }

    if !replaced {
        if in_section {
            // Key not found but we're still in the right section at EOF
            lines.push(new_line);
        } else {
            // Section doesn't exist at all
            lines.push(String::new());
            lines.push(target_header);
            lines.push(new_line);
        }
    }

    let mut output = lines.join("\n");
    if !output.ends_with('\n') {
        output.push('\n');
    }
    std::fs::write(&path, output)?;
    Ok(())
}

pub fn remove_value(root: &Path, section: &str, key: &str) -> Result<()> {
    let path = config_path(root);
    if !path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(&path)?;
    let target_header = format!("[{}]", section);
    let mut in_section = false;
    let mut lines: Vec<&str> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed == target_header;
            lines.push(line);
            continue;
        }
        if in_section {
            if let Some((k, _)) = parse_kv(trimmed) {
                if k == key {
                    continue; // skip this line
                }
            }
        }
        lines.push(line);
    }

    let mut output = lines.join("\n");
    if !output.ends_with('\n') {
        output.push('\n');
    }
    std::fs::write(&path, output)?;
    Ok(())
}

pub fn read_all(root: &Path) -> Result<Vec<(String, String)>> {
    let path = config_path(root);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)?;
    let mut result = Vec::new();
    let mut current_section = String::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current_section = trimmed[1..trimmed.len()-1].to_string();
            continue;
        }
        if let Some((k, v)) = parse_kv(trimmed) {
            result.push((format!("{}.{}", current_section, k), v.to_string()));
        }
    }
    Ok(result)
}

pub fn configured_languages(root: &Path) -> Result<Option<Vec<String>>> {
    match read_value(root, "index", "languages")? {
        None => Ok(None),
        Some(val) => {
            let langs: Vec<String> = val.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            if langs.is_empty() {
                Ok(None)
            } else {
                Ok(Some(langs))
            }
        }
    }
}

fn parse_kv(line: &str) -> Option<(&str, &str)> {
    if line.starts_with('#') || line.is_empty() {
        return None;
    }
    let mut parts = line.splitn(2, '=');
    let k = parts.next()?.trim();
    let v = parts.next()?.trim();
    Some((k, v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir(root.join(".codix")).unwrap();
        (tmp, root)
    }

    #[test]
    fn test_read_nonexistent() {
        let (_tmp, root) = setup();
        assert_eq!(read_value(&root, "index", "languages").unwrap(), None);
    }

    #[test]
    fn test_write_creates_file() {
        let (_tmp, root) = setup();
        write_value(&root, "index", "languages", "java").unwrap();
        let content = std::fs::read_to_string(config_path(&root)).unwrap();
        assert_eq!(content, "[index]\nlanguages = java\n");
    }

    #[test]
    fn test_read_after_write() {
        let (_tmp, root) = setup();
        write_value(&root, "index", "languages", "java,go").unwrap();
        assert_eq!(read_value(&root, "index", "languages").unwrap(), Some("java,go".to_string()));
    }

    #[test]
    fn test_overwrite_in_place() {
        let (_tmp, root) = setup();
        write_value(&root, "index", "languages", "java").unwrap();
        write_value(&root, "index", "languages", "java,go").unwrap();
        let content = std::fs::read_to_string(config_path(&root)).unwrap();
        assert_eq!(content, "[index]\nlanguages = java,go\n");
    }

    #[test]
    fn test_preserves_other_keys() {
        let (_tmp, root) = setup();
        let path = config_path(&root);
        std::fs::write(&path, "[index]\nlanguages = java\nother = value\n").unwrap();
        write_value(&root, "index", "languages", "go").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("languages = go"));
        assert!(content.contains("other = value"));
    }

    #[test]
    fn test_preserves_other_sections() {
        let (_tmp, root) = setup();
        let path = config_path(&root);
        std::fs::write(&path, "[other]\nfoo = bar\n").unwrap();
        write_value(&root, "index", "languages", "java").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("[other]\nfoo = bar"));
        assert!(content.contains("[index]\nlanguages = java"));
    }

    #[test]
    fn test_add_key_to_existing_section() {
        let (_tmp, root) = setup();
        let path = config_path(&root);
        std::fs::write(&path, "[index]\nother = value\n").unwrap();
        write_value(&root, "index", "languages", "java").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("other = value"));
        assert!(content.contains("languages = java"));
    }

    #[test]
    fn test_configured_languages() {
        let (_tmp, root) = setup();
        assert_eq!(configured_languages(&root).unwrap(), None);
        write_value(&root, "index", "languages", "java,go").unwrap();
        assert_eq!(configured_languages(&root).unwrap(), Some(vec!["java".to_string(), "go".to_string()]));
    }
}
