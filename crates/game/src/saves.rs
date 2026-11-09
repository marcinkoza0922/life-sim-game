//! Save slots on disk: `saves/<name>.ron`, plus a rotating autosave.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const DIR: &str = "saves";
pub const AUTOSAVE: &str = "autosave";
pub const QUICKSAVE: &str = "quicksave";

#[derive(Clone, Debug)]
pub struct Slot {
    pub name: String,
    pub path: PathBuf,
    pub modified: Option<SystemTime>,
}

/// Keeps letters, digits, space, `-` and `_`; trims; empty → `None`.
pub fn sanitize(name: &str) -> Option<String> {
    let s: String = name.chars().filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_')).collect();
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

pub fn path_for(name: &str) -> PathBuf {
    Path::new(DIR).join(format!("{name}.ron"))
}

/// All saves in `dir`, newest first.
pub fn list_in(dir: &Path) -> Vec<Slot> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<Slot> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            if path.extension()? != "ron" {
                return None;
            }
            let name = path.file_stem()?.to_string_lossy().into_owned();
            let modified = e.metadata().ok().and_then(|m| m.modified().ok());
            Some(Slot { name, path, modified })
        })
        .collect();
    v.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.name.cmp(&b.name)));
    v
}

pub fn list() -> Vec<Slot> {
    list_in(Path::new(DIR))
}

/// "3 min ago", "2 h ago"…
pub fn age_label(t: Option<SystemTime>) -> String {
    let Some(secs) = t.and_then(|t| SystemTime::now().duration_since(t).ok()).map(|d| d.as_secs()) else {
        return String::new();
    };
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86399 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_path_tricks() {
        assert_eq!(sanitize("../evil/name"), Some("evilname".into()));
        assert_eq!(sanitize("  My Save-2_ "), Some("My Save-2_".into()));
        assert_eq!(sanitize("///"), None);
    }

    #[test]
    fn lists_only_ron_files_newest_first() {
        let dir = std::env::temp_dir().join(format!("lifesim-saves-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.ron"), "x").unwrap();
        std::fs::write(dir.join("note.txt"), "x").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(dir.join("b.ron"), "x").unwrap();
        let names: Vec<String> = list_in(&dir).into_iter().map(|s| s.name).collect();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(names, ["b", "a"]);
    }
}
