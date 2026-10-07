use super::*;

fn usage(page_count: i64, freelist_count: i64) -> DatabaseSpaceUsage {
    let page_size = 4096;
    DatabaseSpaceUsage {
        page_size,
        page_count,
        freelist_count,
        used_bytes: (page_count - freelist_count) * page_size,
        reclaimable_bytes: freelist_count * page_size,
        auto_vacuum: AUTO_VACUUM_INCREMENTAL,
        ..Default::default()
    }
}

#[test]
fn automatic_reclaim_needs_both_size_and_share_thresholds() {
    // 64 MiB free out of 128 MiB: worthwhile.
    assert!(auto_reclaim_worthwhile(&usage(32_768, 16_384)));
    // Plenty of share but only 4 MiB free: not worth the write lock.
    assert!(!auto_reclaim_worthwhile(&usage(2_048, 1_024)));
    // 64 MiB free in a 10 GiB database is below the 10 % share.
    assert!(!auto_reclaim_worthwhile(&usage(2_621_440, 16_384)));
}

#[test]
fn reclaim_steps_adapt_to_lock_time() {
    assert_eq!(next_step_pages(256, Duration::from_millis(5)), 512);
    assert_eq!(next_step_pages(256, Duration::from_millis(60)), 256);
    assert_eq!(next_step_pages(256, Duration::from_millis(300)), 128);
    assert_eq!(
        next_step_pages(RECLAIM_MIN_PAGES, Duration::from_secs(2)),
        RECLAIM_MIN_PAGES
    );
    assert_eq!(
        next_step_pages(RECLAIM_MAX_PAGES, Duration::ZERO),
        RECLAIM_MAX_PAGES
    );
}

#[cfg(unix)]
#[test]
fn disk_lookup_picks_the_longest_matching_mount() {
    let temp = std::env::temp_dir();
    let mounts = vec![
        (PathBuf::from("/"), 10),
        (temp.clone(), 20),
        (PathBuf::from("/definitely-not-a-mount"), 30),
    ];
    let inside = temp.join("codexmanager-missing-file.db");
    assert_eq!(disk_for_path(&mounts, &inside).map(|(_, a)| a), Some(20));
    // A mount point that does not exist must not collapse onto `/`.
    assert_eq!(
        disk_for_path(&mounts, Path::new("/")).map(|(_, a)| a),
        Some(10)
    );
    // Missing components below a missing directory are kept.
    assert_eq!(
        disk_for_path(&mounts, Path::new("/definitely-not-a-mount/sub/a.db")).map(|(_, a)| a),
        Some(30)
    );
}

#[test]
fn labels_and_sizes_are_human_readable() {
    assert_eq!(auto_vacuum_label(AUTO_VACUUM_INCREMENTAL), "incremental");
    assert_eq!(auto_vacuum_label(AUTO_VACUUM_FULL), "full");
    assert_eq!(auto_vacuum_label(0), "none");
    assert_eq!(format_bytes(512 * 1024 * 1024), "512 MiB");
    assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
}
