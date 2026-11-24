use crate::collector::ProcessInfo;

pub fn matches(process: &ProcessInfo, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }

    if let Some((field, value)) = query.split_once(':') {
        let value = value.trim();
        return match field.trim().to_ascii_lowercase().as_str() {
            "pid" => process.pid.to_string().contains(value),
            "user" => contains_case_insensitive(process.user.as_deref().unwrap_or(""), value),
            "path" => process
                .exe_path
                .as_deref()
                .is_some_and(|path| contains_case_insensitive(&path.to_string_lossy(), value)),
            _ => {
                contains_case_insensitive(&process.name, query)
                    || contains_case_insensitive(&process.command, query)
            }
        };
    }

    contains_case_insensitive(&process.name, query)
        || contains_case_insensitive(&process.command, query)
}

fn contains_case_insensitive(value: &str, query: &str) -> bool {
    value.to_lowercase().contains(&query.to_lowercase())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::collector::ProcessInfo;

    use super::matches;

    fn process() -> ProcessInfo {
        ProcessInfo {
            pid: 42,
            name: "Editor".to_owned(),
            command: "editor --workspace /Users/demo/Code".to_owned(),
            exe_path: Some(PathBuf::from(
                "/Applications/Editor.app/Contents/MacOS/editor",
            )),
            user: Some("Alice".to_owned()),
            cpu_percent: None,
            mem_bytes: None,
            status: "Sleep".to_owned(),
        }
    }

    #[test]
    fn matches_name_and_command_without_case_sensitivity() {
        let process = process();
        assert!(matches(&process, "EDIT"));
        assert!(matches(&process, "workspace"));
        assert!(!matches(&process, "unrelated"));
    }

    #[test]
    fn matches_typed_filters() {
        let process = process();
        assert!(matches(&process, "pid:4"));
        assert!(matches(&process, "user:alice"));
        assert!(matches(&process, "path:Editor.app"));
        assert!(!matches(&process, "pid:41"));
        assert!(!matches(&process, "user:bob"));
        assert!(!matches(&process, "path:/Applications/Other"));
    }
}
