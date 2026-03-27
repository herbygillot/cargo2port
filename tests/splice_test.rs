use cargo2port::splice_cargo_crates;

const PORTFILE_WITH_BLOCK: &str = "\
PortSystem          1.0
PortGroup           cargo   1.0

github.setup        Foo bar 1.0.0 v

cargo.crates \\
    foo  1.0.0  aaaaaa \\
    bar  2.0.0  bbbbbb

destroot {
    xinstall -m 0755 ${name} ${destroot}${prefix}/bin/
}
";

const PORTFILE_WITHOUT_BLOCK: &str = "\
PortSystem          1.0
PortGroup           cargo   1.0

github.setup        Foo bar 1.0.0 v

destroot {
    xinstall -m 0755 ${name} ${destroot}${prefix}/bin/
}
";

const NEW_BLOCK: &str = "\
cargo.crates \\
    baz  3.0.0  cccccc \\
    qux  4.0.0  dddddd";

#[test]
fn test_splice_replaces_existing_block() {
    let (result, appended) = splice_cargo_crates(PORTFILE_WITH_BLOCK, NEW_BLOCK);
    assert!(!appended);
    assert!(result.contains("baz  3.0.0  cccccc"));
    assert!(!result.contains("foo  1.0.0  aaaaaa"));
    assert!(!result.contains("bar  2.0.0  bbbbbb"));
}

#[test]
fn test_splice_preserves_surrounding_content() {
    let (result, _) = splice_cargo_crates(PORTFILE_WITH_BLOCK, NEW_BLOCK);
    assert!(result.contains("github.setup        Foo bar 1.0.0 v"));
    assert!(result.contains("destroot {"));
    assert!(result.contains("xinstall -m 0755 ${name} ${destroot}${prefix}/bin/"));
}

#[test]
fn test_splice_appends_when_no_block() {
    let (result, appended) = splice_cargo_crates(PORTFILE_WITHOUT_BLOCK, NEW_BLOCK);
    assert!(appended);
    assert!(result.contains("baz  3.0.0  cccccc"));
    assert!(result.contains("destroot {"));
    assert!(result.ends_with("dddddd\n"));
}

#[test]
fn test_splice_appends_to_file_without_trailing_newline() {
    let portfile = "PortSystem 1.0\nPortGroup cargo 1.0";
    let (result, appended) = splice_cargo_crates(portfile, NEW_BLOCK);
    assert!(appended);
    assert!(result.starts_with("PortSystem 1.0\n"));
    assert!(result.contains("baz  3.0.0  cccccc"));
    assert!(result.ends_with("dddddd\n"));
}

#[test]
fn test_splice_block_at_end_of_file() {
    let portfile = "\
PortSystem 1.0

cargo.crates \\
    old  1.0.0  ffffff \\
    stale  2.0.0  eeeeee";

    let (result, appended) = splice_cargo_crates(portfile, NEW_BLOCK);
    assert!(!appended);
    assert!(result.contains("baz  3.0.0  cccccc"));
    assert!(result.contains("qux  4.0.0  dddddd"));
    assert!(!result.contains("old  1.0.0"));
    assert!(!result.contains("stale  2.0.0"));
}

#[test]
fn test_splice_single_line_block_no_continuation() {
    let portfile = "\
PortSystem 1.0

cargo.crates

destroot {}
";

    let (result, appended) = splice_cargo_crates(portfile, NEW_BLOCK);
    assert!(!appended);
    assert!(result.contains("baz  3.0.0  cccccc"));
    assert!(result.contains("destroot {}"));
}

#[test]
fn test_splice_preserves_content_order() {
    let (result, _) = splice_cargo_crates(PORTFILE_WITH_BLOCK, NEW_BLOCK);
    let github_pos = result.find("github.setup").unwrap();
    let cargo_pos = result.find("cargo.crates").unwrap();
    let destroot_pos = result.find("destroot {").unwrap();
    assert!(github_pos < cargo_pos);
    assert!(cargo_pos < destroot_pos);
}
