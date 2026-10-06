use super::*;
use std::{
    io::{Seek, SeekFrom},
    os::unix::fs::symlink,
};

fn fixture(dir: &Path, count: u8) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let mut toc = vec![0u8; 32];
    toc[..4].copy_from_slice(b"CDVN");
    let mut bin = toc.clone();
    for key in 0..count {
        let offset = bin.len() as u32;
        let mut section = vec![0u8; 32];
        section[..4].copy_from_slice(&[0x9d, 0xa1, 0x46, 0x98]);
        section[4..20].fill(key + 1);
        section[28..32].copy_from_slice(&8u32.to_le_bytes());
        bin.extend(section);
        bin.extend([key; 8]);
        toc.extend([key + 1; 16]);
        toc.extend(offset.to_le_bytes());
        toc.extend(8u32.to_le_bytes());
    }
    let path = dir.join("cache.toc");
    fs::write(&path, toc).unwrap();
    fs::write(path.with_extension("bin"), bin).unwrap();
    path
}
fn fake_paths(dir: &Path) -> Paths {
    Paths {
        home: dir.into(),
        data: dir.join("data/steam-shader-guard"),
        state: dir.join("state/steam-shader-guard"),
        bin: dir.join("bin/steam-shader-guard"),
    }
}

#[test]
fn validates_complete_index() {
    let t = tempfile::tempdir().unwrap();
    let path = fixture(t.path(), 3);
    let (_, rows, length) = cache::read_records(&path).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(length, 152);
    assert!(rows.iter().all(|r| !r.wrapped));
}
#[test]
fn detects_real_four_gib_offset_wrap_without_allocating_four_gib() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("wrapped.toc");
    let mut header = vec![0u8; 32];
    header[..4].copy_from_slice(b"CDVN");
    let mut toc = header.clone();
    let first_size = u32::MAX - 31;
    for (key, size) in [(1u8, first_size), (2, 4)] {
        toc.extend([key; 16]);
        toc.extend(32u32.to_le_bytes());
        toc.extend(size.to_le_bytes());
    }
    fs::write(&path, toc).unwrap();
    let mut bin = File::create(path.with_extension("bin")).unwrap();
    bin.write_all(&header).unwrap();
    for (offset, key, size) in [(32u64, 1u8, first_size), ((1u64 << 32) + 32, 2, 4)] {
        let mut record = [0u8; 32];
        record[..4].copy_from_slice(&[0x9d, 0xa1, 0x46, 0x98]);
        record[4..20].fill(key);
        record[28..32].copy_from_slice(&size.to_le_bytes());
        bin.seek(SeekFrom::Start(offset)).unwrap();
        bin.write_all(&record).unwrap();
    }
    bin.set_len((1u64 << 32) + 68).unwrap();
    let (_, rows, _) = cache::read_records(&path).unwrap();
    assert!(!rows[0].wrapped);
    assert!(rows[1].wrapped);
    assert_eq!(rows[1].offset, (1u64 << 32) + 32);
    assert!(bin.metadata().unwrap().blocks() * 512 < 1024 * 1024);
}
#[test]
fn refuses_unknown_header() {
    let t = tempfile::tempdir().unwrap();
    let p = fixture(t.path(), 1);
    let mut toc = fs::read(&p).unwrap();
    toc[0] = 0;
    fs::write(&p, toc).unwrap();
    assert!(cache::read_records(&p).is_err());
}
#[test]
fn refuses_truncated_payload() {
    let t = tempfile::tempdir().unwrap();
    let p = fixture(t.path(), 1);
    OpenOptions::new()
        .write(true)
        .open(p.with_extension("bin"))
        .unwrap()
        .set_len(66)
        .unwrap();
    assert!(cache::read_records(&p).is_err());
}
#[test]
fn refuses_duplicate_index_entries() {
    let t = tempfile::tempdir().unwrap();
    let p = fixture(t.path(), 1);
    let mut toc = fs::read(&p).unwrap();
    toc.extend_from_within(32..56);
    fs::write(&p, toc).unwrap();
    assert!(cache::read_records(&p).is_err());
}
#[test]
fn refuses_unindexed_tail() {
    let t = tempfile::tempdir().unwrap();
    let p = fixture(t.path(), 1);
    OpenOptions::new()
        .append(true)
        .open(p.with_extension("bin"))
        .unwrap()
        .write_all(b"unknown")
        .unwrap();
    assert!(cache::read_records(&p).is_err());
}
#[test]
fn refuses_missing_pair_and_symlink() {
    let t = tempfile::tempdir().unwrap();
    let p = fixture(t.path(), 1);
    fs::remove_file(p.with_extension("bin")).unwrap();
    assert!(cache::pairs(t.path()).is_err());
    symlink("/dev/null", p.with_extension("bin")).unwrap();
    assert!(cache::pairs(t.path()).is_err());
}
#[test]
fn recovery_shards_are_verified_and_original_is_unchanged() {
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("source");
    let p = fixture(&source.join("GLCache/a/b"), 5);
    let original = fs::read(p.with_extension("bin")).unwrap();
    let original_toc = fs::read(&p).unwrap();
    let dest = t.path().join("games/42");
    cache::recover(&source, &dest, 120).unwrap();
    let files = cache::pairs(&dest.join("nvidia")).unwrap();
    assert_eq!(files.len(), 3);
    let mut payloads = Vec::new();
    for file in files {
        let (_, rows, len) = cache::read_records(&file).unwrap();
        assert!(len < 120);
        assert!(rows.iter().all(|r| !r.wrapped));
        payloads.extend_from_slice(&fs::read(file.with_extension("bin")).unwrap()[32..]);
    }
    assert_eq!(payloads, &original[32..]);
    assert_eq!(fs::read(p.with_extension("bin")).unwrap(), original);
    assert_eq!(fs::read(p).unwrap(), original_toc);
    assert!(cache::recover(&source, &dest, 120).is_err());
}
#[test]
fn failed_recovery_publishes_nothing() {
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("source");
    fixture(&source, 2);
    let dest = t.path().join("games/42");
    assert!(cache::recover(&source, &dest, 64).is_err());
    assert!(!dest.exists());
    assert_eq!(fs::read_dir(dest.parent().unwrap()).unwrap().count(), 0);
}
#[test]
fn rejected_recovery_does_not_create_directories_inside_source() {
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("source");
    fixture(&source, 1);
    for parent in [source.clone(), t.path().join("source-link")] {
        if parent != source {
            symlink(&source, &parent).unwrap();
        }
        assert!(cache::recover(&source, &parent.join("new/42"), 120).is_err());
        assert!(!source.join("new").exists());
    }
}
#[test]
fn rename_cannot_overwrite_existing_directory() {
    let t = tempfile::tempdir().unwrap();
    let a = t.path().join("a");
    let b = t.path().join("b");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    assert!(rename_new(&a, &b).is_err());
    assert!(a.is_dir());
    assert!(b.is_dir());
}
#[test]
fn empty_cache_is_supported_without_invalid_seed_list() {
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("source");
    fixture(&source, 0);
    let dest = t.path().join("games/42");
    cache::recover(&source, &dest, 120).unwrap();
    assert!(!dest.join("readonly-names.txt").exists());
}
#[test]
fn vdf_preserves_unrelated_bytes_and_roundtrips_escaped_launch_option() {
    let s = "// keep\n\"root\"\n{\n\t\"token\" \"private value\"\n\t\"option\" \"old\"\n}\n";
    let doc = vdf::Vdf::parse(s.into()).unwrap();
    let value = "'/a path/it'\"'\"'s/tool' run -- %command%";
    let new = doc.set(&["root", "option"], Some(value)).unwrap();
    assert!(new.contains("\t\"token\" \"private value\""));
    let parsed = vdf::Vdf::parse(new).unwrap();
    assert_eq!(parsed.text(&["root", "option"]), Some(value));
    assert_eq!(
        vdf::Vdf::parse(parsed.set(&["root", "option"], Some("old")).unwrap())
            .unwrap()
            .data,
        doc.data
    );
}
#[test]
fn vdf_adds_missing_app_without_losing_existing_values() {
    let d = vdf::Vdf::parse("\"apps\" { \"old\" { \"x\" \"1\" } }".into()).unwrap();
    let modified = vdf::Vdf::parse(
        d.set(&["apps", "42", "LaunchOptions"], Some("run"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(modified.text(&["apps", "old", "x"]), Some("1"));
    assert_eq!(modified.text(&["apps", "42", "LaunchOptions"]), Some("run"));
}
#[test]
fn vdf_refuses_ambiguous_or_broken_input() {
    for text in [
        "\"a\" \"1\" \"a\" \"2\"",
        "\"a\" {",
        "}",
        "garbage",
        "\"a\" \"unterminated",
    ] {
        assert!(vdf::Vdf::parse(text.into()).is_err());
    }
}
#[test]
fn vdf_keys_are_case_insensitive_without_rewriting_unrelated_bytes() {
    let text = "\"ROOT\" { \"KeepCase\" \"KeepValue\" \"launchoptions\" \"old\" }";
    let doc = vdf::Vdf::parse(text.into()).unwrap();
    assert_eq!(doc.text(&["root", "LaunchOptions"]), Some("old"));
    let changed = doc.set(&["Root", "LaunchOptions"], Some("new")).unwrap();
    assert!(changed.contains("\"KeepCase\" \"KeepValue\""));
    let parsed = vdf::Vdf::parse(changed).unwrap();
    assert_eq!(parsed.text(&["ROOT", "LAUNCHOPTIONS"]), Some("new"));
    let removed = parsed.set(&["root", "launchoptions"], None).unwrap();
    assert!(
        vdf::Vdf::parse(removed)
            .unwrap()
            .get(&["ROOT", "LaunchOptions"])
            .is_none()
    );
}
#[test]
fn vdf_rejects_case_variant_duplicate_keys() {
    assert!(
        vdf::Vdf::parse("\"LaunchOptions\" \"first\" \"launchoptions\" \"second\"".into()).is_err()
    );
}
#[test]
fn game_cache_is_isolated_and_explicit_overrides_survive() {
    let t = tempfile::tempdir().unwrap();
    let p = fake_paths(t.path());
    let e = BTreeMap::from([
        (
            "__GL_SHADER_DISK_CACHE_PATH".into(),
            "/games/steamapps/shadercache/42/nvidiav1".into(),
        ),
        ("__GL_SHADER_DISK_CACHE_SIZE".into(), "123456".into()),
        (
            "__GL_SHADER_DISK_CACHE_READ_ONLY_APP_NAME".into(),
            "old".into(),
        ),
    ]);
    let out = steam::game_environment(&p, "42", &e).unwrap();
    assert_eq!(
        out["__GL_SHADER_DISK_CACHE_PATH"],
        p.app("42").join("nvidia").to_str().unwrap()
    );
    assert_eq!(out["__GL_SHADER_DISK_CACHE_SIZE"], "123456");
    assert!(!out.contains_key("__GL_SHADER_DISK_CACHE_READ_ONLY_APP_NAME"));
    let custom = BTreeMap::from([("__GL_SHADER_DISK_CACHE_PATH".into(), "/my/cache".into())]);
    assert_eq!(steam::game_environment(&p, "42", &custom).unwrap(), custom);
}
#[test]
fn steam_cache_path_suffix_is_compared_as_a_path() {
    let t = tempfile::tempdir().unwrap();
    let p = fake_paths(t.path());
    for path in [
        "/games/steamapps/shadercache/42/nvidiav1/",
        "/games/steamapps//shadercache/42/./nvidiav1",
    ] {
        let env = BTreeMap::from([("__GL_SHADER_DISK_CACHE_PATH".into(), path.into())]);
        let out = steam::game_environment(&p, "42", &env).unwrap();
        assert_eq!(
            out["__GL_SHADER_DISK_CACHE_PATH"],
            p.app("42").join("nvidia").to_str().unwrap()
        );
    }
}
#[test]
fn reads_exact_seed_names_and_rejects_path_traversal() {
    let t = tempfile::tempdir().unwrap();
    let p = fake_paths(t.path());
    fs::create_dir_all(p.app("42")).unwrap();
    fs::write(p.app("42").join("readonly-names.txt"), "sg_a_0;sg_b_1\n").unwrap();
    assert_eq!(
        steam::game_environment(&p, "42", &BTreeMap::new()).unwrap()["__GL_SHADER_DISK_CACHE_READ_ONLY_APP_NAME"],
        "sg_a_0;sg_b_1"
    );
    fs::write(p.app("42").join("readonly-names.txt"), "../../cache").unwrap();
    assert!(steam::game_environment(&p, "42", &BTreeMap::new()).is_err());
    assert!(steam::game_environment(&p, "../42", &BTreeMap::new()).is_err());
}
#[test]
fn discovery_reads_external_library_and_skips_runtime_tools() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("steam");
    let library = t.path().join("other library");
    fs::create_dir_all(root.join("steamapps")).unwrap();
    fs::create_dir_all(library.join("steamapps")).unwrap();
    fs::write(
        root.join("steamapps/libraryfolders.vdf"),
        format!(
            "\"libraryfolders\" {{ \"1\" {{ \"path\" \"{}\" }} }}",
            library.display()
        ),
    )
    .unwrap();
    fs::write(
        library.join("steamapps/appmanifest_42.acf"),
        "\"AppState\" { \"appid\" \"42\" \"name\" \"Example Game\" }",
    )
    .unwrap();
    fs::write(
        root.join("steamapps/appmanifest_43.acf"),
        "\"AppState\" { \"appid\" \"43\" \"name\" \"Proton Experimental\" }",
    )
    .unwrap();
    let games = steam::games(&root).unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].appid, "42");
    assert_eq!(
        games[0].cache,
        library.join("steamapps/shadercache/42/nvidiav1")
    );
}
#[test]
fn atomic_write_preserves_symlinks() {
    let t = tempfile::tempdir().unwrap();
    let real = t.path().join("real");
    let link = t.path().join("link");
    fs::write(&real, "keep").unwrap();
    symlink(&real, &link).unwrap();
    assert!(atomic_write(&link, b"replace", 0o600).is_err());
    assert_eq!(fs::read_to_string(real).unwrap(), "keep");
}
