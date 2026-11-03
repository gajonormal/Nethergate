use super::*;

const ICON: &[u8] = include_bytes!("../../ui/assets/nethergate-mark.png");
const OTHER: &[u8] = include_bytes!("../../public/favicon.png");

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "nethergate-thumbnail-{}",
            crate::transfer::key().unwrap()
        ));
        fs::create_dir_all(path.join("world")).unwrap();
        fs::write(path.join("server.properties"), "level-name=world\n").unwrap();
        fs::write(path.join("world/level.dat"), b"fixture, never interpreted").unwrap();
        Self(path)
    }
    fn store(&self) -> PathBuf {
        self.0.join("cache")
    }
    fn icon(&self, bytes: &[u8]) {
        fs::write(self.0.join("world/icon.png"), bytes).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn original_thumbnail_is_used_byte_for_byte_and_cached() {
    let fixture = Fixture::new();
    fixture.icon(ICON);
    let before = fs::read(fixture.0.join("world/level.dat")).unwrap();
    let first = load_preview(&fixture.store(), "one", &fixture.0).unwrap();
    assert_eq!(
        first.image.as_deref(),
        Some(format!("data:image/png;base64,{}", STANDARD.encode(ICON)).as_str())
    );
    let time = fs::metadata(fixture.store().join("one.json"))
        .unwrap()
        .modified()
        .unwrap();
    let second = load_preview(&fixture.store(), "one", &fixture.0).unwrap();
    assert_eq!(first.revision, second.revision);
    assert_eq!(
        time,
        fs::metadata(fixture.store().join("one.json"))
            .unwrap()
            .modified()
            .unwrap()
    );
    assert_eq!(before, fs::read(fixture.0.join("world/level.dat")).unwrap());
    assert_eq!(ICON, fs::read(fixture.0.join("world/icon.png")).unwrap());
}

#[test]
fn absent_thumbnail_keeps_default_and_appearing_thumbnail_replaces_it() {
    let fixture = Fixture::new();
    assert!(load_preview(&fixture.store(), "one", &fixture.0)
        .unwrap()
        .image
        .is_none());
    fixture.icon(ICON);
    assert!(load_preview(&fixture.store(), "one", &fixture.0)
        .unwrap()
        .image
        .is_some());
    fs::remove_file(fixture.0.join("world/icon.png")).unwrap();
    assert!(load_preview(&fixture.store(), "one", &fixture.0)
        .unwrap()
        .image
        .is_none());
}

#[test]
fn isometric_caches_are_rejected_and_native_updates_are_detected() {
    let fixture = Fixture::new();
    fixture.icon(ICON);
    let first = load_preview(&fixture.store(), "one", &fixture.0).unwrap();
    let path = fixture.store().join("one.json");
    let mut old: Cache = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    old.format = 1;
    old.image = "old isometric map".into();
    fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    let replaced = load_preview(&fixture.store(), "one", &fixture.0).unwrap();
    assert_eq!(first.image, replaced.image);
    let saved: Cache = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved.format, FORMAT);
    fixture.icon(OTHER);
    let updated = load_preview(&fixture.store(), "one", &fixture.0).unwrap();
    assert_ne!(first.revision, updated.revision);
    assert!(load_preview(&fixture.store(), "../outside", &fixture.0).is_err());
    fs::write(fixture.0.join(".nethergate-restore-pending.json"), "{}").unwrap();
    assert!(load_preview(&fixture.store(), "one", &fixture.0).is_err());
}

#[test]
fn corrupt_truncated_and_oversized_icons_are_refused() {
    assert!(validate_png(ICON).is_ok());
    for len in 0..ICON.len() {
        assert!(validate_png(&ICON[..len]).is_err());
    }
    let mut corrupt = ICON.to_vec();
    corrupt[35] ^= 1;
    assert!(validate_png(&corrupt).is_err());
    let mut oversized = ICON.to_vec();
    oversized[16..20].copy_from_slice(&100_000_u32.to_be_bytes());
    assert!(validate_png(&oversized).is_err());
    let fixture = Fixture::new();
    fixture.icon(&vec![0; MAX_IMAGE as usize + 1]);
    assert!(load_preview(&fixture.store(), "one", &fixture.0).is_err());
}

#[test]
fn background_requests_reuse_revisions_without_resending_images() {
    let fixture = Fixture::new();
    fixture.icon(ICON);
    let manager = Arc::new(Manager::default());
    assert!(
        manager
            .request(fixture.store(), "one", fixture.0.clone(), None)
            .unwrap()
            .generating
    );
    let start = Instant::now();
    let ready = loop {
        let preview = manager
            .request(fixture.store(), "one", fixture.0.clone(), None)
            .unwrap();
        if !preview.generating {
            break preview;
        }
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(ready.image.is_some());
    let same = manager
        .request(
            fixture.store(),
            "one",
            fixture.0.clone(),
            ready.revision.as_deref(),
        )
        .unwrap();
    assert_eq!(ready.revision, same.revision);
    assert!(same.image.is_none());
}
