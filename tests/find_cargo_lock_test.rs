use std::fs;

use cargo2port::find_cargo_lock;
use tempfile::TempDir;

#[test]
fn test_find_cargo_lock_at_depth_2() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("foo-1.0");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("Cargo.toml"), "").unwrap();
    fs::write(project.join("Cargo.lock"), "").unwrap();

    let result = find_cargo_lock(tmp.path());
    assert_eq!(result.unwrap(), project.join("Cargo.lock"));
}

#[test]
fn test_find_cargo_lock_empty_dir() {
    let tmp = TempDir::new().unwrap();
    assert!(find_cargo_lock(tmp.path()).is_none());
}

#[test]
fn test_find_cargo_lock_without_cargo_toml() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("foo-1.0");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("Cargo.lock"), "").unwrap();
    // No Cargo.toml alongside — should not match

    assert!(find_cargo_lock(tmp.path()).is_none());
}

#[test]
fn test_find_cargo_lock_too_deep() {
    let tmp = TempDir::new().unwrap();
    let deep = tmp.path().join("a").join("b").join("c");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("Cargo.toml"), "").unwrap();
    fs::write(deep.join("Cargo.lock"), "").unwrap();

    // depth 3 — beyond max_depth of 2
    assert!(find_cargo_lock(tmp.path()).is_none());
}

#[test]
fn test_find_cargo_lock_at_root_level() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("Cargo.toml"), "").unwrap();
    fs::write(tmp.path().join("Cargo.lock"), "").unwrap();

    // Cargo.lock right at the workpath root (depth 1)
    let result = find_cargo_lock(tmp.path());
    assert_eq!(result.unwrap(), tmp.path().join("Cargo.lock"));
}

#[test]
fn test_find_cargo_lock_ignores_nested_without_toml() {
    let tmp = TempDir::new().unwrap();

    // A Cargo.lock without Cargo.toml (e.g. a vendored or stray lockfile)
    let stray = tmp.path().join("subdir");
    fs::create_dir_all(&stray).unwrap();
    fs::write(stray.join("Cargo.lock"), "").unwrap();

    // The real project with both files
    let project = tmp.path().join("real-project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("Cargo.toml"), "").unwrap();
    fs::write(project.join("Cargo.lock"), "").unwrap();

    let result = find_cargo_lock(tmp.path());
    assert_eq!(result.unwrap(), project.join("Cargo.lock"));
}
