use super::{read_skill_summary, CodexSkillsInventory};
use std::fs;
use std::path::Path;

pub(super) fn append_agents_skills(agents_root: &Path, inventory: &mut CodexSkillsInventory) {
    match fs::symlink_metadata(agents_root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            inventory
                .warnings
                .push(format!("unable to inspect ~/.agents/skills: {error}"));
            return;
        }
        Ok(metadata) => {
            // This is an additional read-only source, never a new mutation target.
            // Reject redirected parents as well as redirected skill directories.
            let parent_is_safe = agents_root
                .parent()
                .and_then(|parent| fs::symlink_metadata(parent).ok())
                .is_some_and(|parent| is_regular_directory_without_reparse(&parent));
            if !is_regular_directory_without_reparse(&metadata) || !parent_is_safe {
                inventory
                    .warnings
                    .push("ignored unsafe ~/.agents/skills directory".to_string());
                return;
            }
        }
    }
    let entries = match fs::read_dir(agents_root) {
        Ok(entries) => entries,
        Err(error) => {
            inventory
                .warnings
                .push(format!("unable to read ~/.agents/skills: {error}"));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                inventory.warnings.push(format!(
                    "skipped unreadable ~/.agents/skills entry: {error}"
                ));
                continue;
            }
        };
        let directory_name = entry.file_name().to_string_lossy().to_string();
        if directory_name.starts_with('.') {
            continue;
        }
        let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if !is_regular_directory_without_reparse(&metadata) {
            continue;
        }
        inventory.items.push(read_skill_summary(
            &entry.path(),
            format!("~/.agents/skills/{directory_name}"),
            "user",
            false,
        ));
    }
}

fn is_regular_directory_without_reparse(metadata: &fs::Metadata) -> bool {
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return false;
        }
    }
    true
}
