//! Process enumeration from /proc.
use std::collections::HashSet;

/// Lowercased executable names of every running process (e.g. "resolve").
pub fn running_processes() -> HashSet<String> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return HashSet::new();
    };
    dir.flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(process_name)
        .collect()
}

/// Executable file name of `pid`, lowercased. Falls back to `comm` (which the
/// kernel truncates to 15 bytes) when `exe` is unreadable, e.g. for processes
/// of other users. Kernel threads (empty cmdline) are skipped.
pub fn process_name(pid: u32) -> Option<String> {
    let from_exe = std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .and_then(|path| {
            let name = path.file_name()?.to_str()?;
            Some(
                name.strip_suffix(" (deleted)")
                    .unwrap_or(name)
                    .to_lowercase(),
            )
        });
    from_exe
        .or_else(|| {
            let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
            if cmdline.is_empty() {
                return None;
            }
            let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
            Some(comm.trim_end().to_lowercase())
        })
        .filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_self() {
        let me = process_name(std::process::id()).unwrap();
        assert!(running_processes().contains(&me));
    }
}
