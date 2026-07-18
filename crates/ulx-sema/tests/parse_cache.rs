//! `ParseCache` (§13.7's incremental-analysis gap, narrowed to real
//! re-parse avoidance — see the type's own docs in `resolve.rs`): a file
//! whose mtime hasn't advanced since a prior `load_and_analyze_with_deps_cached`
//! call should be reused (a cache *hit*, not a fresh read+parse), and a
//! file that *has* changed should be picked up correctly on the next call
//! using the same cache (a *miss*).

use std::path::PathBuf;

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ulexite-sema-parse-cache-{name}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_fixture(dir: &std::path::Path) -> (PathBuf, PathBuf) {
    let lib_file = dir.join("lib.ulx");
    std::fs::write(
        &lib_file,
        r#"
        judge Fluency(subject: text) -> Verdict {
          rubric: """Is this fluent?"""
        }
        "#,
    )
    .unwrap();
    let main_file = dir.join("main.ulx");
    std::fs::write(
        &main_file,
        r#"
        import judge Fluency from "lib.ulx"

        conversation UseIt(x: text) -> Verdict {
          judge Fluency(x)
        }
        "#,
    )
    .unwrap();
    (main_file, lib_file)
}

#[test]
fn an_unchanged_workspace_is_all_cache_hits_on_the_second_call() {
    let dir = scratch_dir("unchanged");
    let (main_file, _lib_file) = write_fixture(&dir);

    let mut cache = ulx_sema::ParseCache::new();
    let deps = ulx_sema::DependencyPaths::default();
    let ws1 = ulx_sema::analyze_file_with_deps_cached(&main_file, None, &deps, &mut cache)
        .expect("first call must succeed");
    assert!(ws1.modules.values().all(|m| m.diagnostics.is_empty()));
    assert_eq!(cache.misses, 2, "both main.ulx and lib.ulx are fresh reads");
    assert_eq!(cache.hits, 0);

    let ws2 = ulx_sema::analyze_file_with_deps_cached(&main_file, None, &deps, &mut cache)
        .expect("second call must succeed");
    assert!(ws2.modules.values().all(|m| m.diagnostics.is_empty()));
    assert_eq!(
        cache.hits, 2,
        "both files are unchanged, so both should now be cache hits"
    );
    assert_eq!(
        cache.misses, 2,
        "no new misses — nothing was re-read from disk"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// The other half: a file that *did* change since the last call must be
/// picked up correctly — the cache invalidates by mtime, it doesn't just
/// always reuse whatever it first saw, and its sibling (unchanged) file
/// still gets a cache hit rather than being needlessly re-read too.
#[test]
fn a_changed_imported_file_is_reparsed_while_its_unchanged_sibling_stays_a_cache_hit() {
    let dir = scratch_dir("changed");
    let (main_file, lib_file) = write_fixture(&dir);

    let mut cache = ulx_sema::ParseCache::new();
    let deps = ulx_sema::DependencyPaths::default();
    let ws1 = ulx_sema::analyze_file_with_deps_cached(&main_file, None, &deps, &mut cache)
        .expect("first call must succeed");
    assert!(ws1.modules.values().all(|m| m.diagnostics.is_empty()));

    // Rename the judge declared in lib.ulx — main.ulx's import now
    // references a name that no longer exists there. A brief sleep keeps
    // this robust against coarse filesystem mtime resolution actually
    // reporting the same timestamp for both writes.
    std::thread::sleep(std::time::Duration::from_millis(10));
    std::fs::write(
        &lib_file,
        r#"
        judge Politeness(subject: text) -> Verdict {
          rubric: """Is this polite?"""
        }
        "#,
    )
    .unwrap();

    let misses_before = cache.misses;
    let hits_before = cache.hits;
    let ws2 = ulx_sema::analyze_file_with_deps_cached(&main_file, None, &deps, &mut cache)
        .expect("second call must still succeed (a semantic error, not an I/O one)");

    assert_eq!(
        cache.misses,
        misses_before + 1,
        "only lib.ulx (the changed file) should be a fresh miss"
    );
    assert_eq!(
        cache.hits,
        hits_before + 1,
        "main.ulx (unchanged) should still be a cache hit, not re-read too"
    );

    let has_error = ws2.modules.values().any(|m| {
        m.diagnostics
            .iter()
            .any(|d| d.severity == ulx_sema::Severity::Error)
    });
    assert!(
        has_error,
        "expected the renamed judge to surface as a resolution error, got: {:#?}",
        ws2.modules
            .values()
            .map(|m| &m.diagnostics)
            .collect::<Vec<_>>()
    );

    std::fs::remove_dir_all(&dir).ok();
}
