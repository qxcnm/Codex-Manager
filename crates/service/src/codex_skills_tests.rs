use super::*;
use std::io::Cursor;
use std::sync::atomic::{AtomicU64, Ordering};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

struct TempTree {
    path: PathBuf,
}

impl TempTree {
    fn new(label: &str) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "codexmanager-skills-{label}-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create temp tree");
        Self { path }
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn skill_markdown(name: &str, description: &str) -> String {
    format!("---\nname: \"{name}\"\ndescription: \"{description}\"\n---\n\n# {name}\n")
}

fn write_skill(directory: &Path, name: &str, description: &str) {
    fs::create_dir_all(directory).expect("create skill directory");
    fs::write(
        directory.join(SKILL_FILE_NAME),
        skill_markdown(name, description),
    )
    .expect("write skill markdown");
}

fn zip_bytes(entries: &[(&str, &[u8], Option<u32>)]) -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    for (path, content, mode) in entries {
        let mut options =
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        if let Some(mode) = mode {
            options = options.unix_permissions(*mode);
        }
        writer.start_file(*path, options).expect("start zip file");
        writer.write_all(content).expect("write zip file");
    }
    writer.finish().expect("finish zip").into_inner()
}

#[test]
fn skill_frontmatter_supports_codex_yaml_scalars_without_nested_overrides() {
    for scalar in ["|", "|-", "|+", ">", ">-", ">+"] {
        let markdown = format!(
            "---\nname: yaml-skill\ndescription: {scalar}\n  Build for AWS: ECS\n  and deploy safely.\nmetadata:\n  name: nested-name\n  description: nested-description\n---\n"
        );
        let (name, description) =
            parse_skill_metadata_document(&markdown).expect("parse YAML block scalar");
        assert_eq!(name, "yaml-skill", "{scalar}");
        assert_eq!(
            description, "Build for AWS: ECS and deploy safely.",
            "{scalar}"
        );
    }

    let (name, description) = parse_skill_metadata_document(
        "---\nname: 'yaml-skill'\ndescription: Build for AWS: ECS # compatibility prose\n---\n",
    )
    .expect("retain Codex compatibility for unquoted prose");
    assert_eq!(name, "yaml-skill");
    assert_eq!(description, "Build for AWS: ECS");
}

#[test]
fn skill_frontmatter_rejects_metadata_codex_cannot_load() {
    for markdown in [
        "---\nname: test\nmetadata:\n  description: nested-only\n---\n".to_string(),
        "---\nname: test\ndescription: '  '\n---\n".to_string(),
        "---\nname: test\ndescription: [invalid, scalar]\n---\n".to_string(),
        "---\nname: first\nname: second\ndescription: duplicate keys\n---\n".to_string(),
        format!(
            "---\nname: {}\ndescription: Too long\n---\n",
            "x".repeat(65)
        ),
    ] {
        assert!(
            parse_skill_metadata_document(&markdown).is_err(),
            "{markdown}"
        );
    }
}

#[test]
fn zip_install_and_inventory_use_yaml_frontmatter() {
    let tree = TempTree::new("yaml-frontmatter");
    let skills_root = tree.path.join("skills");
    let markdown = b"---\nname: yaml-skill\ndescription: >-\n  Use this skill\n  for deployment.\nmetadata:\n  name: unrelated\n---\n";
    let archive = zip_bytes(&[("yaml-skill/SKILL.md", markdown, Some(0o100644))]);
    assert_eq!(
        install_zip_into_root(&skills_root, &archive).expect("install YAML skill"),
        "yaml-skill"
    );
    let inventory = list_from_root(&tree.path, &skills_root).expect("list YAML skill");
    assert_eq!(inventory.items.len(), 1);
    assert_eq!(inventory.items[0].name, "yaml-skill");
    assert_eq!(
        inventory.items[0].description,
        "Use this skill for deployment."
    );
    assert!(inventory.items[0].valid);
}

#[test]
fn list_scans_user_and_system_skills_without_item_paths() {
    let tree = TempTree::new("list");
    let skills_root = tree.path.join("skills");
    write_skill(
        &skills_root.join("user-skill"),
        "user-skill",
        "User description",
    );
    write_skill(
        &skills_root.join(SYSTEM_DIRECTORY_NAME).join("system-skill"),
        "system-skill",
        "System description",
    );
    write_skill(
        &skills_root.join(format!("{QUARANTINE_PREFIX}orphan")),
        "quarantined-skill",
        "Must stay hidden",
    );

    let inventory = list_from_root(&tree.path, &skills_root).expect("list skills");
    assert_eq!(inventory.items.len(), 2);
    let user = inventory
        .items
        .iter()
        .find(|item| item.name == "user-skill")
        .expect("user skill");
    assert!(user.deletable);
    assert_eq!(user.source, "user");
    let system = inventory
        .items
        .iter()
        .find(|item| item.name == "system-skill")
        .expect("system skill");
    assert!(!system.deletable);
    assert_eq!(system.source, "system");
    assert_eq!(system.directory_name, ".system/system-skill");

    let item_json = serde_json::to_string(system).expect("serialize system skill");
    assert!(!item_json.contains(&tree.path.to_string_lossy().to_string()));

    write_skill(
        &skills_root.join("unsafe name"),
        "safe-metadata-name",
        "Unsafe directory name",
    );
    let inventory = list_from_root(&tree.path, &skills_root).expect("list unsafe skill");
    let unsafe_item = inventory
        .items
        .iter()
        .find(|item| item.directory_name == "unsafe name")
        .expect("unsafe directory entry");
    assert!(!unsafe_item.deletable);
}

#[test]
fn agents_skills_are_listed_read_only_without_changing_managed_deletion() {
    let tree = TempTree::new("agents-read-only");
    let user_home = tree.path.join("user");
    let codex_home = tree.path.join("custom-codex");
    let skills_root = codex_home.join("skills");
    let external = user_home.join(".agents/skills/shared-skill");
    write_skill(&skills_root.join("shared-skill"), "shared-skill", "Managed");
    write_skill(&external, "shared-skill", "External");

    let inventory =
        list_with_user_home(&codex_home, Some(&user_home)).expect("list both user skill roots");
    assert_eq!(inventory.items.len(), 2);
    let managed = inventory.items.iter().find(|item| item.deletable).unwrap();
    assert_eq!(managed.directory_name, "shared-skill");
    let external_item = inventory.items.iter().find(|item| !item.deletable).unwrap();
    assert_eq!(
        external_item.directory_name,
        "~/.agents/skills/shared-skill"
    );
    assert_eq!(external_item.source, "user");
    assert!(external_item.valid);
    assert_eq!(external_item.description, "External");
    assert!(delete_from_root(&skills_root, &external_item.directory_name).is_err());
    assert!(external.join(SKILL_FILE_NAME).is_file());
    assert!(skills_root.join("shared-skill/SKILL.md").is_file());

    delete_from_root(&skills_root, &managed.directory_name).expect("delete only managed skill");
    assert!(external.join(SKILL_FILE_NAME).is_file());
    let remaining = list_with_user_home(&codex_home, Some(&user_home)).unwrap();
    assert_eq!(remaining.items.len(), 1);
    assert!(!remaining.items[0].deletable);
}

#[test]
fn agents_inventory_uses_actual_user_home_with_a_custom_codex_home() {
    let _env_lock = crate::test_env_guard();
    struct RestoreEnvironment(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for RestoreEnvironment {
        fn drop(&mut self) {
            for (key, value) in self.0.drain(..) {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
    let tree = TempTree::new("agents-real-home");
    let user_home = tree.path.join("user");
    let codex_home = tree.path.join("custom-codex");
    let _restore = RestoreEnvironment(
        ["USERPROFILE", "HOME", "CODEX_HOME"]
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect(),
    );
    std::env::set_var("USERPROFILE", &user_home);
    std::env::set_var("HOME", &user_home);
    std::env::set_var("CODEX_HOME", &codex_home);
    write_skill(
        &user_home.join(".agents/skills/actual-home-skill"),
        "actual-home-skill",
        "Actual user home",
    );
    write_skill(
        &codex_home.join(".agents/skills/wrong-home-skill"),
        "wrong-home-skill",
        "Must not use the profile directory as the user home",
    );

    let inventory = list_for_profile(&codex_home).expect("list custom profile");
    assert_eq!(inventory.items.len(), 1);
    assert_eq!(inventory.items[0].name, "actual-home-skill");
    assert!(!inventory.items[0].deletable);
    assert!(!codex_home.join("skills").exists());
}

#[test]
fn agents_inventory_does_not_duplicate_the_managed_physical_root() {
    let tree = TempTree::new("agents-shared-root");
    let user_home = tree.path.join("user");
    fs::create_dir_all(user_home.join("unused")).expect("create canonical path component");
    let codex_home = user_home.join("unused/../.agents");
    write_skill(
        &user_home.join(".agents/skills/shared-skill"),
        "shared-skill",
        "Same physical directory",
    );

    let inventory = list_with_user_home(&codex_home, Some(&user_home)).expect("list aliased root");
    assert_eq!(inventory.items.len(), 1);
    assert!(inventory.items[0].deletable);
    assert_eq!(inventory.items[0].directory_name, "shared-skill");
}

#[cfg(unix)]
#[test]
fn agents_inventory_ignores_redirected_directories() {
    use std::os::unix::fs::symlink;
    let tree = TempTree::new("agents-symlink");
    let user_home = tree.path.join("user");
    let codex_home = tree.path.join("codex");
    let outside = tree.path.join("outside");
    write_skill(&outside.join("skills/external"), "external", "Outside root");
    fs::create_dir_all(&user_home).unwrap();
    symlink(&outside, user_home.join(".agents")).expect("redirect agents directory");

    let inventory = list_with_user_home(&codex_home, Some(&user_home)).expect("skip unsafe source");
    assert!(inventory.items.is_empty());
    assert!(inventory
        .warnings
        .iter()
        .any(|warning| warning.contains("unsafe")));
    assert!(outside.join("skills/external/SKILL.md").is_file());
}

#[test]
fn zip_install_accepts_one_skill_and_refuses_overwrite() {
    let tree = TempTree::new("zip-install");
    let skills_root = tree.path.join("skills");
    let markdown = skill_markdown("zip-skill", "Installed from ZIP");
    let archive = zip_bytes(&[
        ("zip-root/SKILL.md", markdown.as_bytes(), Some(0o100644)),
        ("zip-root/scripts/run.sh", b"#!/bin/sh\n", Some(0o100777)),
    ]);

    let installed = install_zip_into_root(&skills_root, &archive).expect("install zip");
    assert_eq!(installed, "zip-skill");
    assert!(skills_root.join("zip-skill/SKILL.md").is_file());
    assert!(skills_root.join("zip-skill/scripts/run.sh").is_file());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let skill_mode = fs::metadata(skills_root.join("zip-skill"))
            .expect("skill directory metadata")
            .permissions()
            .mode();
        let scripts_mode = fs::metadata(skills_root.join("zip-skill/scripts"))
            .expect("scripts directory metadata")
            .permissions()
            .mode();
        let executable_mode = fs::metadata(skills_root.join("zip-skill/scripts/run.sh"))
            .expect("executable metadata")
            .permissions()
            .mode();
        assert_eq!(skill_mode & 0o777, 0o700);
        assert_eq!(scripts_mode & 0o777, 0o755);
        assert_eq!(executable_mode & 0o777, 0o755);
    }

    let error = install_zip_into_root(&skills_root, &archive).expect_err("reject overwrite");
    assert!(error.contains("already installed"), "{error}");
}

#[test]
fn zip_install_rejects_path_traversal_and_symlink_entries() {
    let tree = TempTree::new("zip-unsafe");
    let skills_root = tree.path.join("skills");
    let markdown = skill_markdown("unsafe-skill", "Unsafe");
    let traversal = zip_bytes(&[
        ("unsafe/SKILL.md", markdown.as_bytes(), Some(0o100644)),
        ("unsafe/../../escape.txt", b"escape", Some(0o100644)),
    ]);
    let traversal_error =
        install_zip_into_root(&skills_root, &traversal).expect_err("reject traversal");
    assert!(traversal_error.contains("unsafe path"), "{traversal_error}");
    assert!(!tree.path.join("escape.txt").exists());

    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    writer
        .start_file("unsafe/SKILL.md", options)
        .expect("start skill markdown");
    writer
        .write_all(markdown.as_bytes())
        .expect("write skill markdown");
    writer
        .add_symlink("unsafe/link", "../../escape", options)
        .expect("add symlink");
    let symlink = writer.finish().expect("finish zip").into_inner();
    let symlink_error = install_zip_into_root(&skills_root, &symlink).expect_err("reject symlink");
    assert!(
        symlink_error.contains("symlink or special file"),
        "{symlink_error}"
    );
}

#[test]
fn zip_install_rejects_file_count_abuse() {
    let tree = TempTree::new("zip-count");
    let skills_root = tree.path.join("skills");
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    writer
        .start_file("many/SKILL.md", options)
        .expect("start skill markdown");
    writer
        .write_all(skill_markdown("many-files", "Many").as_bytes())
        .expect("write skill markdown");
    for index in 0..MAX_FILE_COUNT {
        writer
            .start_file(format!("many/files/{index}.txt"), options)
            .expect("start counted file");
    }
    let archive = writer.finish().expect("finish zip").into_inner();

    let error = install_zip_into_root(&skills_root, &archive).expect_err("reject file count");
    assert!(error.contains("file limit"), "{error}");
}

#[test]
fn zip_install_rejects_total_entry_count_abuse() {
    let tree = TempTree::new("zip-entry-count");
    let skills_root = tree.path.join("skills");
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    writer
        .start_file("many/SKILL.md", options)
        .expect("start skill markdown");
    writer
        .write_all(skill_markdown("many-entries", "Many").as_bytes())
        .expect("write skill markdown");
    for index in 0..MAX_ARCHIVE_ENTRY_COUNT {
        writer
            .add_directory(format!("many/empty-{index}/"), options)
            .expect("add empty directory");
    }
    let archive = writer.finish().expect("finish zip").into_inner();

    let error = install_zip_into_root(&skills_root, &archive).expect_err("reject entry count");
    assert!(error.contains("entry limit"), "{error}");
}

#[test]
fn zip_install_rejects_multiple_skill_manifests() {
    let tree = TempTree::new("zip-multiple-skills");
    let skills_root = tree.path.join("skills");
    let root_markdown = skill_markdown("root-skill", "Root");
    let nested_markdown = skill_markdown("nested-skill", "Nested");
    let archive = zip_bytes(&[
        ("SKILL.md", root_markdown.as_bytes(), Some(0o100644)),
        (
            "nested/SKILL.md",
            nested_markdown.as_bytes(),
            Some(0o100644),
        ),
    ]);

    let error = install_zip_into_root(&skills_root, &archive).expect_err("reject multiple skills");
    assert!(error.contains("exactly one skill"), "{error}");
}

#[test]
fn directory_import_copies_one_valid_skill_and_refuses_existing_target() {
    let tree = TempTree::new("directory-import");
    let source = tree.path.join("source");
    let skills_root = tree.path.join("codex/skills");
    write_skill(&source, "imported-skill", "Imported directory");
    fs::create_dir_all(source.join("references")).expect("create references");
    fs::write(source.join("references/guide.md"), "guide").expect("write guide");

    let installed = import_directory_into_root(&skills_root, &source).expect("import directory");
    assert_eq!(installed, "imported-skill");
    assert!(skills_root
        .join("imported-skill/references/guide.md")
        .is_file());

    let error = import_directory_into_root(&skills_root, &source).expect_err("reject overwrite");
    assert!(error.contains("already installed"), "{error}");
}

#[test]
fn directory_import_rejects_sources_that_overlap_the_skills_root() {
    let tree = TempTree::new("directory-overlap");
    let skills_root = tree.path.join("codex/skills");
    let nested_source = skills_root.join("group/nested-source");
    write_skill(&nested_source, "nested-source", "Nested source");

    let nested_error = import_directory_into_root(&skills_root, &nested_source)
        .expect_err("reject source below skills root");
    assert!(nested_error.contains("must not overlap"), "{nested_error}");

    let ancestor_source = tree.path.join("ancestor-source");
    write_skill(&ancestor_source, "ancestor-source", "Ancestor source");
    let descendant_skills_root = ancestor_source.join("codex/skills");
    let ancestor_error = import_directory_into_root(&descendant_skills_root, &ancestor_source)
        .expect_err("reject source above skills root");
    assert!(
        ancestor_error.contains("must not overlap"),
        "{ancestor_error}"
    );
}

#[test]
fn directory_import_rejects_total_entry_count_abuse() {
    let tree = TempTree::new("directory-entry-count");
    let source = tree.path.join("source");
    let skills_root = tree.path.join("codex/skills");
    write_skill(&source, "many-source-entries", "Many entries");
    for index in 0..MAX_SOURCE_ENTRY_COUNT {
        fs::create_dir_all(source.join(format!("empty-{index}")))
            .expect("create empty source directory");
    }

    let error = import_directory_into_root(&skills_root, &source)
        .expect_err("reject excessive source entries");
    assert!(error.contains("entry limit"), "{error}");
}

#[cfg(unix)]
#[test]
fn directory_import_strips_special_permission_bits() {
    use std::os::unix::fs::PermissionsExt;

    let tree = TempTree::new("directory-permissions");
    let source = tree.path.join("source");
    let skills_root = tree.path.join("codex/skills");
    write_skill(&source, "permission-skill", "Permissions");
    let executable = source.join("run.sh");
    fs::write(&executable, "#!/bin/sh\n").expect("write executable");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o4777))
        .expect("set source permissions");

    import_directory_into_root(&skills_root, &source).expect("import directory");
    let installed_mode = fs::metadata(skills_root.join("permission-skill/run.sh"))
        .expect("installed executable metadata")
        .permissions()
        .mode();
    assert_eq!(installed_mode & 0o7777, 0o755);
}

#[cfg(unix)]
#[test]
fn managed_skills_root_permissions_never_expand_existing_access() {
    use std::os::unix::fs::PermissionsExt;

    let tree = TempTree::new("root-permissions");

    let new_root = tree.path.join("new/skills");
    create_managed_skills_root(&new_root).expect("create managed skills root");
    let new_mode = fs::metadata(&new_root)
        .expect("new root metadata")
        .permissions()
        .mode();
    assert_eq!(new_mode & 0o777, 0o700);

    for (label, initial_mode, expected_mode) in [
        ("private", 0o700, 0o700),
        ("shared-read", 0o750, 0o750),
        ("world-writable", 0o777, 0o755),
    ] {
        let root = tree.path.join(label);
        fs::create_dir(&root).expect("create existing skills root");
        fs::set_permissions(&root, fs::Permissions::from_mode(initial_mode))
            .expect("set existing root permissions");
        create_managed_skills_root(&root).expect("reuse existing skills root");
        let resulting_mode = fs::metadata(&root)
            .expect("existing root metadata")
            .permissions()
            .mode();
        assert_eq!(resulting_mode & 0o777, expected_mode, "root {label}");
    }
}

#[test]
fn bounded_copy_refuses_to_overwrite_an_existing_file() {
    let tree = TempTree::new("copy-create-new");
    let output = tree.path.join("existing.txt");
    fs::write(&output, "keep-me").expect("write existing output");
    let mut input = Cursor::new(b"replacement".as_slice());
    let mut total_written = 0;
    let mut files_written = 0;

    let error = copy_reader_bounded(&mut input, &output, &mut total_written, &mut files_written)
        .expect_err("refuse existing output");

    assert!(error.contains("create installed skill file"), "{error}");
    assert_eq!(fs::read_to_string(output).expect("read output"), "keep-me");
}

#[test]
fn relative_paths_reject_windows_unsafe_components() {
    for path in [
        "folder/CON.txt",
        "folder/contains:colon.txt",
        "folder/trailing.",
        "folder/trailing ",
    ] {
        assert!(
            validate_relative_path(Path::new(path), MAX_PATH_DEPTH).is_err(),
            "expected {path:?} to be rejected"
        );
    }
}

#[cfg(unix)]
#[test]
fn directory_import_and_delete_reject_symlinks() {
    use std::os::unix::fs::symlink;

    let tree = TempTree::new("symlink");
    let source = tree.path.join("source");
    let skills_root = tree.path.join("codex/skills");
    write_skill(&source, "linked-skill", "Linked");
    let source_alias = tree.path.join("source-alias");
    symlink(&source, &source_alias).expect("create source directory link");
    let source_alias_error = import_directory_into_root(&skills_root, &source_alias)
        .expect_err("reject symlinked source directory");
    assert!(
        source_alias_error.contains("symbolic link"),
        "{source_alias_error}"
    );

    fs::write(tree.path.join("outside.txt"), "outside").expect("write outside");
    symlink(tree.path.join("outside.txt"), source.join("link.txt")).expect("create source link");

    let import_error =
        import_directory_into_root(&skills_root, &source).expect_err("reject source link");
    assert!(import_error.contains("symbolic link"), "{import_error}");

    fs::create_dir_all(&skills_root).expect("create skills root");
    symlink(&source, skills_root.join("linked-skill")).expect("create managed link");
    let delete_error =
        delete_from_root(&skills_root, "linked-skill").expect_err("reject managed link");
    assert!(delete_error.contains("symlink"), "{delete_error}");
    assert!(source.exists());
}

#[cfg(unix)]
#[test]
fn bounded_manifest_read_rejects_symlinks_and_does_not_block_on_fifos() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{symlink, OpenOptionsExt};
    use std::thread;
    use std::time::{Duration, Instant};

    let tree = TempTree::new("manifest-special-files");
    let outside = tree.path.join("outside.md");
    fs::write(&outside, skill_markdown("outside", "Outside")).expect("write outside file");
    let linked = tree.path.join(SKILL_FILE_NAME);
    symlink(&outside, &linked).expect("create manifest symlink");
    assert!(
        read_text_file_bounded(&linked, MAX_SKILL_MD_BYTES).is_err(),
        "manifest reader must not follow symlinks"
    );

    let fifo = tree.path.join("manifest.pipe");
    let fifo_c_path = CString::new(fifo.as_os_str().as_bytes()).expect("fifo path CString");
    let result = unsafe { libc::mkfifo(fifo_c_path.as_ptr(), 0o600) };
    assert_eq!(
        result,
        0,
        "create FIFO: {}",
        std::io::Error::last_os_error()
    );

    // If O_NONBLOCK is accidentally removed, this writer releases the reader
    // after a bounded delay so the regression fails instead of hanging CI.
    let writer_fifo = fifo.clone();
    let writer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(500));
        let mut options = OpenOptions::new();
        options.write(true).custom_flags(libc::O_NONBLOCK);
        let _ = options.open(writer_fifo);
    });
    let started = Instant::now();
    let error = read_text_file_bounded(&fifo, MAX_SKILL_MD_BYTES)
        .expect_err("manifest reader must reject FIFOs");
    let elapsed = started.elapsed();
    writer.join().expect("join fallback FIFO writer");

    assert!(error.contains("regular file"), "{error}");
    assert!(
        elapsed < Duration::from_millis(250),
        "FIFO read blocked for {elapsed:?}"
    );
}

#[cfg(unix)]
#[test]
fn directory_import_detects_same_size_file_replacement_and_fifo_entries() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let tree = TempTree::new("source-race");
    let source = tree.path.join("source");
    write_skill(&source, "source-race", "Source race");
    let payload = source.join("payload.txt");
    fs::write(&payload, "before").expect("write original payload");

    let mut files = Vec::new();
    let mut directories = Vec::new();
    let mut file_count = 0usize;
    let mut entry_count = 0usize;
    let mut total_size = 0u64;
    collect_source_entries(
        &source,
        &source,
        &mut files,
        &mut directories,
        &mut file_count,
        &mut entry_count,
        &mut total_size,
    )
    .expect("collect source files");
    let payload_entry = files
        .iter()
        .find(|entry| entry.relative_path == Path::new("payload.txt"))
        .expect("payload entry");
    fs::remove_file(&payload).expect("remove original payload");
    fs::write(&payload, "after!").expect("replace payload at same size");
    let replacement_error =
        open_validated_source_file(payload_entry).expect_err("reject same-size replacement");
    assert!(replacement_error.contains("changed"), "{replacement_error}");

    let fifo_path = source.join("pipe");
    let fifo_c_path = CString::new(fifo_path.as_os_str().as_bytes()).expect("fifo path CString");
    let result = unsafe { libc::mkfifo(fifo_c_path.as_ptr(), 0o600) };
    assert_eq!(
        result,
        0,
        "create FIFO: {}",
        std::io::Error::last_os_error()
    );
    let mut fifo_files = Vec::new();
    let mut fifo_directories = Vec::new();
    let mut fifo_file_count = 0usize;
    let mut fifo_entry_count = 0usize;
    let mut fifo_total_size = 0u64;
    let fifo_error = collect_source_entries(
        &source,
        &source,
        &mut fifo_files,
        &mut fifo_directories,
        &mut fifo_file_count,
        &mut fifo_entry_count,
        &mut fifo_total_size,
    )
    .expect_err("reject FIFO");
    assert!(fifo_error.contains("special file"), "{fifo_error}");
}

#[test]
fn delete_is_limited_to_direct_user_skill_directories() {
    let tree = TempTree::new("delete");
    let skills_root = tree.path.join("skills");
    write_skill(&skills_root.join("delete-me"), "delete-me", "Delete me");
    write_skill(
        &skills_root.join(SYSTEM_DIRECTORY_NAME).join("keep-system"),
        "keep-system",
        "Keep",
    );

    delete_from_root(&skills_root, "delete-me").expect("delete user skill");
    assert!(!skills_root.join("delete-me").exists());
    assert!(
        fs::read_dir(&skills_root)
            .expect("read skills root")
            .all(|entry| !entry
                .expect("skills entry")
                .file_name()
                .to_string_lossy()
                .starts_with(QUARANTINE_PREFIX)),
        "successful deletion must not leave quarantine directories"
    );
    assert!(delete_from_root(&skills_root, ".system").is_err());
    assert!(delete_from_root(&skills_root, "nested/skill").is_err());
    assert!(skills_root
        .join(SYSTEM_DIRECTORY_NAME)
        .join("keep-system")
        .exists());
}
