use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

#[tokio::test(flavor = "current_thread")]
async fn cancelled_mutation_retains_lock_until_disk_phase_finishes() {
    let (started, entered) = tokio::sync::oneshot::channel();
    let (release, resume) = std::sync::mpsc::channel();
    let (finished, done) = tokio::sync::oneshot::channel();
    let mutation = tokio::spawn(async move {
        let lease = Arc::new(mutation_lock().lock().await);
        run_locked_phase(&lease, move || {
            started.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(5)).unwrap();
            let _ = finished.send(());
            Ok(())
        })
        .await
    });
    tokio::time::timeout(Duration::from_secs(2), entered)
        .await
        .expect("disk phase started")
        .unwrap();
    mutation.abort();
    assert!(mutation.await.unwrap_err().is_cancelled());
    // The HTTP task is gone, but the outstanding disk commit still owns its
    // mutation lease. A concurrent delete/install cannot pass it.
    assert!(mutation_lock().try_lock().is_err());
    release.send(()).unwrap();
    done.await.unwrap();
    let _next = tokio::time::timeout(Duration::from_secs(2), mutation_lock().lock())
        .await
        .expect("the lock releases after the disk phase finishes");
}

#[tokio::test(flavor = "current_thread")]
async fn repository_downloads_yield_and_enforce_both_body_size_limits() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let (entered, mut started) = tokio::sync::mpsc::channel(16);
    let app = axum::Router::new()
        .route(
            "/slow",
            axum::routing::get({
                let gate = gate.clone();
                move || {
                    let gate = gate.clone();
                    let entered = entered.clone();
                    async move {
                        entered.send(()).await.unwrap();
                        gate.acquire().await.unwrap().forget();
                        "archive"
                    }
                }
            }),
        )
        .route("/fast", axum::routing::get(|| async { "metadata" }))
        .route("/known-large", axum::routing::get(|| async { "oversized" }))
        .route(
            "/chunked-large",
            axum::routing::get(|| async {
                axum::body::Body::from_stream(futures_util::stream::iter([
                    Ok::<_, std::io::Error>(bytes::Bytes::from_static(b"1234")),
                    Ok(bytes::Bytes::from_static(b"5678")),
                ]))
            }),
        );
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let mut requests = Vec::new();
    for _ in 0..16 {
        let url = format!("{base}/slow");
        requests.push(tokio::spawn(async move {
            get_bounded(&url, 32, "download").await
        }));
    }
    for _ in 0..16 {
        tokio::time::timeout(Duration::from_secs(2), started.recv())
            .await
            .expect("concurrent download starts")
            .unwrap();
    }
    let metadata = tokio::time::timeout(
        Duration::from_secs(2),
        get_bounded(&format!("{base}/fast"), 32, "metadata"),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(metadata, b"metadata");
    assert_eq!(run_phase(|| Ok(17)).await.unwrap(), 17);
    gate.add_permits(16);
    for request in requests {
        assert_eq!(request.await.unwrap().unwrap(), b"archive");
    }
    for path in ["known-large", "chunked-large"] {
        assert!(get_bounded(&format!("{base}/{path}"), 5, "download")
            .await
            .unwrap_err()
            .contains("size limit"));
    }
    server.abort();
}

struct TempTree {
    path: PathBuf,
}

impl TempTree {
    fn new(label: &str) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "codexmanager-skill-repositories-{label}-{}-{}",
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

fn repository_zip(entries: &[(&str, &[u8], Option<u32>)]) -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    for (path, content, mode) in entries {
        let mut options =
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        if let Some(mode) = mode {
            options = options.unix_permissions(*mode);
        }
        writer.start_file(*path, options).expect("start file");
        writer.write_all(content).expect("write file");
    }
    writer.finish().expect("finish repository zip").into_inner()
}

fn skill_markdown(name: &str, description: &str) -> Vec<u8> {
    format!("---\nname: \"{name}\"\ndescription: \"{description}\"\n---\n\n# {name}\n").into_bytes()
}

#[test]
fn github_sources_and_refs_are_strictly_bounded() {
    assert_eq!(
        normalize_github_source("https://github.com/anthropics/skills").expect("valid source"),
        GitHubRepositorySource {
            owner: "anthropics".to_string(),
            repository: "skills".to_string(),
        }
    );
    assert!(normalize_github_source("http://github.com/anthropics/skills").is_err());
    assert!(normalize_github_source("https://github.com/anthropics/skills/issues").is_err());
    assert!(normalize_github_source("https://github.com.evil.test/anthropics/skills").is_err());
    assert!(normalize_ref_name(Some("../../main")).is_err());
    assert!(normalize_ref_name(Some("main;touch-pwned")).is_err());
    assert_eq!(
        normalize_ref_name(Some("release/v1")).expect("valid ref"),
        Some("release/v1".to_string())
    );
}

#[test]
fn repository_scan_finds_valid_skills_and_ignores_invalid_documents() {
    let first = skill_markdown("first-skill", "First description");
    let second = skill_markdown("second-skill", "Second description");
    let archive = repository_zip(&[
        ("skills-main/skills/first/SKILL.md", &first, None),
        ("skills-main/skills/first/reference.md", b"reference", None),
        ("skills-main/nested/second/SKILL.md", &second, None),
        ("skills-main/broken/SKILL.md", b"not frontmatter", None),
        (
            "skills-main/node_modules/ignored/SKILL.md",
            &skill_markdown("ignored-skill", "ignored"),
            None,
        ),
    ]);
    let source = GitHubRepositorySource {
        owner: "anthropics".to_string(),
        repository: "skills".to_string(),
    };
    let items =
        scan_repository_archive(&archive, "repo_test", &source, "main").expect("scan repository");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].name, "first-skill");
    assert_eq!(items[0].path, "skills/first");
    assert!(items[0].skill_id.starts_with("skill_"));
    assert_eq!(items[1].name, "second-skill");
}

#[test]
fn repository_scan_rejects_empty_and_bounded_metadata_abuse() {
    let archive = repository_zip(&[("repo-main/README.md", b"no skills", None)]);
    let source = GitHubRepositorySource {
        owner: "example".to_string(),
        repository: "empty".to_string(),
    };
    let error = scan_repository_archive(&archive, "repo_empty", &source, "main")
        .expect_err("empty repository must preserve the last good snapshot");
    assert!(error.contains("no valid Skills"));

    let mut candidates = MAX_SKILL_DOCUMENT_CANDIDATES;
    let mut total_bytes = 0;
    let error = consume_skill_document_budget(&mut candidates, &mut total_bytes, 1)
        .expect_err("candidate limit");
    assert!(error.contains("candidates"));

    let mut candidates = 0;
    let mut total_bytes = MAX_SKILL_DOCUMENT_BYTES_TOTAL;
    let error = consume_skill_document_budget(&mut candidates, &mut total_bytes, 1)
        .expect_err("metadata byte limit");
    assert!(error.contains("scan size limit"));

    let long = "技".repeat(MAX_SKILL_DESCRIPTION_BYTES);
    let truncated = truncate_utf8_bytes(long, MAX_SKILL_DESCRIPTION_BYTES);
    assert!(truncated.len() <= MAX_SKILL_DESCRIPTION_BYTES);
    assert!(truncated.is_char_boundary(truncated.len()));
}

#[test]
fn selected_skill_archive_contains_only_the_requested_skill() {
    let parent = skill_markdown("parent-skill", "Parent");
    let nested = skill_markdown("nested-skill", "Nested");
    let sibling = skill_markdown("sibling-skill", "Sibling");
    let archive = repository_zip(&[
        ("repo-main/skills/parent/SKILL.md", &parent, None),
        ("repo-main/skills/parent/reference.md", b"parent ref", None),
        ("repo-main/skills/parent/nested/SKILL.md", &nested, None),
        ("repo-main/skills/parent/nested/data.txt", b"nested", None),
        ("repo-main/skills/sibling/SKILL.md", &sibling, None),
    ]);
    let selected = build_selected_skill_archive(&archive, "skills/parent").expect("select skill");
    let mut selected = ZipArchive::new(Cursor::new(selected)).expect("open selected zip");
    let mut names = (0..selected.len())
        .map(|index| selected.by_index(index).expect("entry").name().to_string())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, vec!["SKILL.md", "reference.md"]);
}

#[test]
fn root_level_skill_is_scanned_and_packaged_without_nested_skills() {
    let root = skill_markdown("root-skill", "Root");
    let nested = skill_markdown("nested-skill", "Nested");
    let archive = repository_zip(&[
        ("repo-main/SKILL.md", &root, None),
        ("repo-main/reference.md", b"root ref", None),
        ("repo-main/nested/SKILL.md", &nested, None),
        ("repo-main/nested/data.txt", b"nested", None),
    ]);
    let source = GitHubRepositorySource {
        owner: "example".to_string(),
        repository: "root-skill".to_string(),
    };
    let scanned =
        scan_repository_archive(&archive, "repo_root", &source, "main").expect("scan root Skill");
    assert!(scanned
        .iter()
        .any(|skill| skill.name == "root-skill" && skill.path == "."));

    let selected = build_selected_skill_archive(&archive, ".").expect("package root Skill");
    let mut selected = ZipArchive::new(Cursor::new(selected)).expect("open selected zip");
    let mut names = (0..selected.len())
        .map(|index| selected.by_index(index).expect("entry").name().to_string())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, vec!["SKILL.md", "reference.md"]);
}

#[test]
fn selected_skill_archive_rejects_symlinks() {
    let skill = skill_markdown("unsafe-skill", "Unsafe");
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    writer
        .start_file("repo-main/skills/unsafe/SKILL.md", options)
        .expect("start skill markdown");
    writer.write_all(&skill).expect("write skill markdown");
    writer
        .add_symlink("repo-main/skills/unsafe/link", "../../outside", options)
        .expect("add symlink");
    let archive = writer.finish().expect("finish zip").into_inner();
    let error = build_selected_skill_archive(&archive, "skills/unsafe")
        .expect_err("symlink must be rejected");
    assert!(error.contains("symlink"));
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "live GitHub repository probe"]
async fn live_builtin_repositories_scan_and_package() {
    for (owner, repository, ref_name, minimum) in [
        ("anthropics", "skills", "main", 10usize),
        ("ComposioHQ", "awesome-claude-skills", "master", 100usize),
        ("cexll", "myclaude", "master", 5usize),
        ("JimLiu", "baoyu-skills", "main", 5usize),
    ] {
        let source = GitHubRepositorySource {
            owner: owner.to_string(),
            repository: repository.to_string(),
        };
        let archive = download_repository_archive(&source, ref_name)
            .await
            .expect("download repository");
        let skills = scan_repository_archive(&archive, "repo_live", &source, ref_name)
            .expect("scan repository");
        assert!(
            skills.len() >= minimum,
            "{owner}/{repository} found only {} Skills",
            skills.len()
        );
        let selected = build_selected_skill_archive(&archive, &skills[0].path)
            .expect("package selected Skill");
        let mut selected = ZipArchive::new(Cursor::new(selected)).expect("open selected Skill");
        assert!(selected.by_name("SKILL.md").is_ok());
    }
}

#[test]
#[ignore = "live repository persistence and install probe"]
fn live_repository_refresh_persists_and_installs() {
    let _env_guard = crate::test_env_guard();
    let tree = TempTree::new("live-install");
    let database = tree.path.join("codexmanager.db");
    std::env::set_var("CODEXMANAGER_DB_PATH", &database);
    crate::initialize_storage_if_needed().expect("initialize storage");
    let codex_home = tree.path.join("codex-home");
    let codex_home_text = codex_home.to_string_lossy().to_string();

    let initial = list(Some(&codex_home_text)).expect("list builtin repositories");
    assert_eq!(initial.repositories.len(), 4);
    let refreshed = refresh(Some("builtin-anthropics-skills"), Some(&codex_home_text))
        .expect("refresh anthropics Skills");
    let first = refreshed
        .items
        .iter()
        .find(|item| item.repository_id == "builtin-anthropics-skills")
        .expect("refreshed Skill")
        .clone();
    assert!(!first.installed);

    let installed = install(
        Some("builtin-anthropics-skills"),
        Some(&first.skill_id),
        Some(&codex_home_text),
    )
    .expect("install repository Skill");
    assert!(installed
        .items
        .iter()
        .any(|item| item.skill_id == first.skill_id && item.installed));
    assert!(codex_home.join("skills").join(first.name).is_dir());
}

#[test]
#[ignore = "live skills.sh search and install probe"]
fn live_registry_search_and_install() {
    let tree = TempTree::new("live-registry");
    let codex_home = tree.path.join("codex-home");
    let codex_home_text = codex_home.to_string_lossy().to_string();
    let result = registry_search(
        Some("frontend design"),
        Some(10),
        Some(0),
        Some(&codex_home_text),
    )
    .expect("search skills.sh");
    assert!(!result.items.is_empty());

    let installed = registry_install(
        Some("anthropics/skills"),
        Some("frontend-design"),
        Some(&codex_home_text),
    )
    .expect("install skills.sh result");
    assert!(installed
        .items
        .iter()
        .any(|item| item.name == "frontend-design"));
}
