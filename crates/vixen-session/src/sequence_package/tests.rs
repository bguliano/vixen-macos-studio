use super::*;
use parking_lot::Mutex;
use vixen_core::TimeSpan;

pub(super) struct Fixture {
    pub(super) root: PathBuf,
    pub(super) core: AppCore,
}
impl Fixture {
    pub(super) fn new() -> Self {
        let root = std::env::temp_dir().join(format!("vixen-package-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        Self { root, core }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[derive(Default)]
struct Messages {
    overwrite: bool,
    skip: bool,
    warnings: Mutex<Vec<String>>,
    progress: Mutex<Vec<(String, u32, u32)>>,
}
impl Listener for Messages {
    fn progress(&self, t: &str, p: u32, o: u32) {
        self.progress.lock().push((t.into(), p, o));
    }
    fn overwrite_package(&self, _: &Path) -> bool {
        self.overwrite
    }
    fn sequence_destination(&self, _: &str, p: &Path) -> Option<PathBuf> {
        (!self.skip).then(|| p.to_owned())
    }
    fn warning(&self, m: &str) {
        self.warnings.lock().push(m.into());
    }
}
pub(super) fn archive(path: &Path, entries: &[(&str, &[u8])]) {
    let mut z = ZipWriter::new(File::create(path).unwrap());
    for (n, b) in entries {
        z.start_file(*n, SimpleFileOptions::default()).unwrap();
        z.write_all(b).unwrap();
    }
    z.finish().unwrap();
}
fn effect(source: u128) -> vixen_io::EffectNode {
    let descriptor = vixen_effect_registry::all()
        .iter()
        .find(|d| d.name == "Set Level")
        .unwrap();
    vixen_io::EffectNode {
        start: TimeSpan::ZERO,
        time_span: TimeSpan::from_ms(500.0),
        type_id: descriptor.type_id,
        instance_id: Uuid::new_v4(),
        targets: vec![(Uuid::from_u128(source), "Old".into())],
        params: vixen_effects::framework::Params::defaults(descriptor.params),
    }
}
fn seq_text() -> String {
    let mut s = vixen_io::new_sequence();
    s.length = TimeSpan::from_ms(1000.0);
    s.effects = vec![effect(1), effect(2)];
    vixen_io::sequence::sequence_to_document(&s).unwrap()
}
fn mapping(f: &Fixture) -> PathBuf {
    let mut tree = f.core.system.write();
    tree.tree
        .add_node(Uuid::from_u128(3), "Target", Some(Uuid::from_u128(4)));
    drop(tree);
    let mut m = ElementMapState::new(
        "Map".into(),
        vec![(Uuid::from_u128(1), "Old".into())],
        Arc::new(f.core.system.read().tree.clone()),
    );
    m.assign(Uuid::from_u128(1), Some(Uuid::from_u128(3)))
        .unwrap();
    let path = f.root.join("map.vmap");
    m.save(&path).unwrap();
    path
}
#[test]
fn settings_roundtrip_keeps_unknown_data_and_windows_path_default() {
    let f = Fixture::new();
    let mut store = ModuleStore::new();
    let mut s = ExportSettings::load(&store, &f.core.paths);
    assert!(s.audio);
    s.output = r"C:\Users\owner\My.Folder\Export".into();
    s.enter_output(&f.core.paths, None);
    assert_eq!(s.output, f.root.join("Export/show.vpkg").to_string_lossy());
    s.sequences = vec!["one.tim".into()];
    s.store(&mut store, &f.core.paths);
    store
        .type_data_mut(MODULE_TYPE)
        .unwrap()
        .data_mut()
        .unwrap()
        .push(XElement::with_text(NS, "Future", "keep"));
    assert_eq!(ExportSettings::load(&store, &f.core.paths), s);
    s.audio = false;
    s.store(&mut store, &f.core.paths);
    assert_eq!(
        store
            .type_data(MODULE_TYPE)
            .unwrap()
            .data()
            .unwrap()
            .child_text("Future")
            .as_deref(),
        Some("keep")
    );
}
#[test]
fn export_preserves_sequence_bytes_windows_entries_media_and_tree() {
    let f = Fixture::new();
    let sequence = f.core.paths.sequence_dir().join("One.tim");
    let source=b"<root><FileName>SONG.*</FileName><FileName>image?.png</FileName><FileName>SONG.*</FileName></root>";
    fs::write(&sequence, source).unwrap();
    fs::write(f.core.paths.media_dir().join("song.mp3"), b"audio").unwrap();
    let modules = f.core.paths.module_data_dir().join("Picture");
    fs::create_dir_all(&modules).unwrap();
    fs::write(modules.join("image1.png"), b"image").unwrap();
    let output = f.root.join("show.vpkg");
    let s = ExportSettings {
        output: output.to_string_lossy().into(),
        audio: true,
        sequences: vec![sequence.to_string_lossy().into()],
    };
    let result = export(&f.core, &s, &AtomicBool::new(false), &Messages::default()).unwrap();
    assert_eq!(result.completed, 1);
    let mut z = ZipArchive::new(File::open(&output).unwrap()).unwrap();
    let mut raw = vec![];
    z.by_name("Sequence\\One.tim")
        .unwrap()
        .read_to_end(&mut raw)
        .unwrap();
    assert_eq!(raw, source);
    assert!(z.by_name("SequenceData\\One\\Media\\song.mp3").is_ok());
    assert!(z
        .by_name("SequenceData\\One\\Module Data Files\\Picture\\image1.png")
        .is_ok());
    assert!(z.by_name("ProfileInfo\\ElementTree.v3e").is_ok());
    assert_eq!(inspect(&output).unwrap().sequences[0].name, "One");
}
#[test]
fn declined_overwrite_and_cancel_preserve_previous_package() {
    let f = Fixture::new();
    let output = f.root.join("show.vpkg");
    fs::write(&output, b"previous").unwrap();
    let s = ExportSettings {
        output: output.to_string_lossy().into(),
        audio: true,
        sequences: vec![],
    };
    assert!(
        export(&f.core, &s, &AtomicBool::new(false), &Messages::default())
            .unwrap()
            .cancelled
    );
    assert!(
        export(
            &f.core,
            &s,
            &AtomicBool::new(true),
            &Messages {
                overwrite: true,
                ..Default::default()
            }
        )
        .unwrap()
        .cancelled
    );
    assert_eq!(fs::read(&output).unwrap(), b"previous");
    assert!(!fs::read_dir(&f.root)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().contains(".part")));
}
#[test]
fn unsafe_packages_are_rejected_before_any_write() {
    let f = Fixture::new();
    for (i, name) in [
        "Sequence/../escape.tim",
        "SequenceData\\A\\Media\\..\\escape",
        "/tmp/absolute",
        "C:\\escape",
        "Sequence/./A.tim",
    ]
    .iter()
    .enumerate()
    {
        let p = f.root.join(format!("{i}.vpkg"));
        archive(&p, &[("Sequence/A.tim", b"x"), (*name, b"x")]);
        assert!(inspect(&p).is_err(), "{name}");
    }
}
#[test]
fn import_exact_name_remaps_first_target_and_skips_existing_media() {
    let f = Fixture::new();
    let map = mapping(&f);
    let text = seq_text();
    let path = f.root.join("package.vpkg");
    archive(
        &path,
        &[
            ("Sequence\\BigFoo.tim", b"bad"),
            ("Sequence/Foo.tim", text.as_bytes()),
            ("SequenceData/Foo/Media/song.mp3", b"new"),
            (
                "SequenceData\\Foo\\Module Data Files\\Picture\\new.png",
                b"image",
            ),
        ],
    );
    fs::write(f.core.paths.media_dir().join("song.mp3"), b"existing").unwrap();
    let listener = Messages::default();
    let result = import(
        &f.core,
        &path,
        &map,
        &["Foo".into()],
        &AtomicBool::new(false),
        &listener,
    )
    .unwrap();
    assert_eq!(result.completed, 1);
    assert!(result.errors.is_empty());
    let seq = vixen_io::load_sequence(&f.core.paths.sequence_dir().join("Foo.tim")).unwrap();
    assert_eq!(seq.effects.len(), 1);
    assert_eq!(
        seq.effects[0].targets,
        vec![(Uuid::from_u128(3), "Target".into())]
    );
    assert_eq!(
        fs::read(f.core.paths.media_dir().join("song.mp3")).unwrap(),
        b"existing"
    );
    assert_eq!(
        fs::read(f.core.paths.module_data_dir().join("Picture/new.png")).unwrap(),
        b"image"
    );
    assert!(!f.core.paths.sequence_dir().join("BigFoo.tim").exists());
    assert_eq!(listener.progress.lock().last().unwrap().2, 100);
}
#[test]
fn failed_load_leaves_destination_and_unrelated_backup_intact() {
    let f = Fixture::new();
    let map = mapping(&f);
    let path = f.root.join("package.vpkg");
    archive(&path, &[("Sequence/A.tim", b"not xml")]);
    let dest = f.core.paths.sequence_dir().join("A.tim");
    fs::write(&dest, b"old").unwrap();
    let backup = f.core.paths.sequence_dir().join("XA.tim.8");
    fs::write(&backup, b"backup").unwrap();
    let result = import(
        &f.core,
        &path,
        &map,
        &["A".into()],
        &AtomicBool::new(false),
        &Messages::default(),
    )
    .unwrap();
    assert_eq!(result.errors.len(), 1);
    assert_eq!(fs::read(&dest).unwrap(), b"old");
    assert_eq!(fs::read(backup).unwrap(), b"backup");
    assert!(!fs::read_dir(f.core.paths.sequence_dir())
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with(".vixen-import")));
}

fn legacy_sequence_with_media(source: &Path) -> String {
    let mut sequence = vixen_io::new_sequence();
    sequence.media.push(vixen_io::MediaEntry {
        type_id: uuid::uuid!("fe460392-3fab-4c63-99dd-d76c48354150"),
        instance_id: Uuid::new_v4(),
        file_name: String::new(),
        data: None,
    });
    let mut root =
        XElement::parse(&vixen_io::sequence::sequence_to_document(&sequence).unwrap()).unwrap();
    root.set_attr("version", "0");
    let file = root
        .child_mut("_mediaSurrogates")
        .unwrap()
        .child_mut("MediaSurrogate")
        .unwrap()
        .child_mut("FileName")
        .unwrap();
    file.local = "FilePath".into();
    file.children = vec![vixen_xml::XNode::Text(
        source.to_string_lossy().into_owned(),
    )];
    root.to_document()
}

#[test]
fn legacy_package_import_restores_reachable_sources_before_saving_and_extracting() {
    for (embedded, existing) in [(false, false), (true, false), (true, true)] {
        let f = Fixture::new();
        let map = mapping(&f);
        let source = f.root.join("Song & ü.wav");
        fs::write(&source, b"reachable original media").unwrap();
        let text = legacy_sequence_with_media(&source);
        let package = f.root.join("legacy.vpkg");
        let mut entries = vec![("Sequence/Legacy.tim", text.as_bytes())];
        if embedded {
            entries.push(("SequenceData/Legacy/Media/Song & ü.wav", b"embedded media"));
        }
        archive(&package, &entries);
        let package_before = fs::read(&package).unwrap();
        let target = f.core.paths.media_dir().join("Song & ü.wav");
        if existing {
            fs::write(&target, b"existing profile media").unwrap();
        }
        let result = import(
            &f.core,
            &package,
            &map,
            &["Legacy".into()],
            &AtomicBool::new(false),
            &Messages::default(),
        )
        .unwrap();
        assert_eq!(result.completed, 1, "{:?}", result.errors);
        assert!(result.errors.is_empty());
        let expected: &[u8] = if existing {
            b"existing profile media"
        } else {
            b"reachable original media"
        };
        assert_eq!(fs::read(&target).unwrap(), expected);
        assert_eq!(fs::read(&source).unwrap(), b"reachable original media");
        assert_eq!(fs::read(&package).unwrap(), package_before);
        let loaded =
            vixen_io::load_sequence(&f.core.paths.sequence_dir().join("Legacy.tim")).unwrap();
        assert_eq!(
            loaded.source_version,
            vixen_io::sequence::CURRENT_SEQUENCE_VERSION
        );
        assert_eq!(loaded.media[0].file_name, "Song & ü.wav");
        assert_eq!(fs::read_dir(f.core.paths.media_dir()).unwrap().count(), 1);
    }
}

#[test]
fn legacy_package_copy_failure_preserves_the_existing_sequence_and_source() {
    let f = Fixture::new();
    let map = mapping(&f);
    let source = f.root.join("Song & ü.wav");
    fs::write(&source, b"original media").unwrap();
    let text = legacy_sequence_with_media(&source);
    let package = f.root.join("legacy.vpkg");
    archive(&package, &[("Sequence/Legacy.tim", text.as_bytes())]);
    let target = f.core.paths.media_dir().join("Song & ü.wav");
    fs::create_dir(&target).unwrap();
    let sequence = f.core.paths.sequence_dir().join("Legacy.tim");
    fs::write(&sequence, b"original sequence").unwrap();
    let result = import(
        &f.core,
        &package,
        &map,
        &["Legacy".into()],
        &AtomicBool::new(false),
        &Messages::default(),
    )
    .unwrap();
    assert_eq!(result.completed, 0);
    assert_eq!(result.errors.len(), 1);
    assert_eq!(fs::read(&sequence).unwrap(), b"original sequence");
    assert_eq!(fs::read(&source).unwrap(), b"original media");
    assert!(target.is_dir());
    assert_eq!(fs::read_dir(f.core.paths.media_dir()).unwrap().count(), 1);
    assert!(!fs::read_dir(f.core.paths.sequence_dir())
        .unwrap()
        .flatten()
        .any(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with(".vixen-import")));
}

#[test]
fn multi_target_and_targetless_effects_follow_first_target_rule() {
    let f = Fixture::new();
    mapping(&f);
    let mut s = vixen_io::new_sequence();
    let mut multi = effect(1);
    multi.targets.push((Uuid::from_u128(2), "Second".into()));
    let mut empty = effect(1);
    empty.targets.clear();
    s.effects = vec![multi, empty, effect(2)];
    remap_sequence(
        &mut s,
        &[(Uuid::from_u128(1), Uuid::from_u128(3))],
        &f.core.system.read().tree,
    );
    assert_eq!(s.effects.len(), 1);
    assert_eq!(s.effects[0].targets.len(), 1);
}
#[test]
fn import_skips_declined_duplicate_and_rejects_symlink_media() {
    let f = Fixture::new();
    let map = mapping(&f);
    let text = seq_text();
    let path = f.root.join("package.vpkg");
    archive(
        &path,
        &[
            ("Sequence/A.tim", text.as_bytes()),
            ("SequenceData/A/Media/link/song.mp3", b"new"),
        ],
    );
    let dest = f.core.paths.sequence_dir().join("A.tim");
    fs::write(&dest, b"old").unwrap();
    let result = import(
        &f.core,
        &path,
        &map,
        &["A".into()],
        &AtomicBool::new(false),
        &Messages {
            skip: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.skipped, 1);
    assert_eq!(fs::read(&dest).unwrap(), b"old");
    let outside = f.root.join("outside");
    fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, f.core.paths.media_dir().join("link")).unwrap();
    let result = import(
        &f.core,
        &path,
        &map,
        &["A".into()],
        &AtomicBool::new(false),
        &Messages::default(),
    )
    .unwrap();
    assert_eq!(result.errors.len(), 1);
    assert!(!outside.join("song.mp3").exists());
}

#[test]
fn streaming_copy_observes_cancellation_between_chunks() {
    struct CancelReader<'a>(&'a AtomicBool);
    impl Read for CancelReader<'_> {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            bytes.fill(7);
            self.0.store(true, Ordering::Release);
            Ok(bytes.len())
        }
    }
    let cancel = AtomicBool::new(false);
    let mut bytes = vec![];
    assert!(copy_cancellable(&mut CancelReader(&cancel), &mut bytes, &cancel).is_err());
    assert_eq!(bytes.len(), 64 * 1024);
}

#[test]
fn export_failure_preserves_old_file_and_removes_private_partial() {
    let f = Fixture::new();
    let output = f.root.join("show.vpkg");
    fs::write(&output, b"previous").unwrap();
    let settings = ExportSettings {
        output: output.to_string_lossy().into(),
        audio: false,
        sequences: vec![f.root.join("missing.tim").to_string_lossy().into()],
    };
    assert!(export(
        &f.core,
        &settings,
        &AtomicBool::new(false),
        &Messages {
            overwrite: true,
            ..Default::default()
        }
    )
    .is_err());
    assert_eq!(fs::read(&output).unwrap(), b"previous");
    assert!(!fs::read_dir(&f.root)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().contains(".part")));
}

#[test]
fn audio_off_keeps_effect_media_and_rooted_name_stops_further_discovery() {
    let f = Fixture::new();
    let sequence = f.core.paths.sequence_dir().join("One.tim");
    fs::write(&sequence, b"<root><FileName>first.png</FileName><FileName>C:\\missing.mp3</FileName><FileName>last.png</FileName></root>").unwrap();
    for file in ["first.png", "last.png"] {
        fs::write(f.core.paths.module_data_dir().join(file), b"module").unwrap();
        fs::write(f.core.paths.media_dir().join(file), b"media").unwrap();
    }
    let output = f.root.join("show.vpkg");
    export(
        &f.core,
        &ExportSettings {
            output: output.to_string_lossy().into(),
            audio: false,
            sequences: vec![sequence.to_string_lossy().into()],
        },
        &AtomicBool::new(false),
        &Messages::default(),
    )
    .unwrap();
    let mut zip = ZipArchive::new(File::open(output).unwrap()).unwrap();
    assert!(zip
        .by_name("SequenceData\\One\\Module Data Files\\first.png")
        .is_ok());
    assert!(zip
        .by_name("SequenceData\\One\\Module Data Files\\last.png")
        .is_err());
    assert!(zip.by_name("SequenceData\\One\\Media\\first.png").is_err());
}

#[test]
fn canonical_media_matching_preserves_wildcards_case_and_distinct_accents() {
    for (pattern, name) in [
        ("Café.wav", "Cafe\u{301}.wav"),
        ("CAF?.WAV", "Cafe\u{301}.wav"),
        ("?toile.png", "E\u{301}toile.png"),
        ("SONG.*", "song.mp3"),
        ("image?.png", "IMAGE1.PNG"),
    ] {
        assert!(wildcard(pattern, name), "{pattern:?} / {name:?}");
    }
    for (pattern, name) in [
        ("Cafe.wav", "Café.wav"),
        ("ffi.png", "ﬃ.png"),
        ("A.png", "Ａ.png"),
        ("image?.png", "image12.png"),
        ("Caf??.wav", "Cafe\u{301}.wav"),
    ] {
        assert!(!wildcard(pattern, name), "{pattern:?} / {name:?}");
    }
}

#[test]
fn literal_archive_aliases_are_distinct_but_case_only_and_globs_keep_disk_name() {
    let source = Path::new("/private/Media/Cafe\u{301}.wav");
    let nfc = media_match(source, "Café.wav");
    let nfd = media_match(source, "Cafe\u{301}.wav");
    assert_eq!(nfc.archive_name, "Café.wav");
    assert_eq!(nfd.archive_name, "Cafe\u{301}.wav");
    assert_eq!(
        media_match(source, "Caf?.wav").archive_name,
        "Cafe\u{301}.wav"
    );
    assert_eq!(
        media_match(Path::new("/private/Media/SONG.wav"), "song.wav").archive_name,
        "SONG.wav"
    );
    assert_eq!(
        unique_media(vec![nfc.clone(), nfd.clone(), nfc.clone(), nfd.clone()]).unwrap(),
        vec![nfc, nfd]
    );
}

#[test]
fn different_sources_cannot_silently_claim_one_exact_archive_alias() {
    // These exact distinct disk spellings can coexist on a normalization-sensitive filesystem.
    let a = media_match(Path::new("/private/Media/Cafe\u{301}.wav"), "Café.wav");
    let b = media_match(Path::new("/private/Media/Café.wav"), "Café.wav");
    assert!(unique_media(vec![a, b])
        .unwrap_err()
        .to_string()
        .contains("Different media files"));
    // Equal basenames in distinct module folders remain distinct archive paths.
    let a = media_match(Path::new("/private/Picture/Cafe\u{301}.png"), "Café.png");
    let b = media_match(Path::new("/private/Video/Cafe\u{301}.png"), "Café.png");
    assert_eq!(unique_media(vec![a, b]).unwrap().len(), 2);
}

#[test]
fn excluded_audio_does_not_discover_or_validate_media_aliases() {
    let f = Fixture::new();
    let sequence = f.core.paths.sequence_dir().join("NoAudio.tim");
    fs::write(&sequence, "<root><FileName>Café.wav</FileName></root>").unwrap();
    let candidates = vec![
        PathBuf::from("/private/Media/Cafe\u{301}.wav"),
        PathBuf::from("/private/Media/Café.wav"),
    ];
    assert!(discover(&sequence, &[], &candidates).is_err());
    assert!(discover(&sequence, &[], &[]).unwrap().1.is_empty());
    fs::write(
        f.core.paths.media_dir().join("Cafe\u{301}.wav"),
        b"excluded audio",
    )
    .unwrap();
    let output = f.root.join("without-audio.vpkg");
    let result = export(
        &f.core,
        &ExportSettings {
            output: output.to_string_lossy().into(),
            audio: false,
            sequences: vec![sequence.to_string_lossy().into()],
        },
        &AtomicBool::new(false),
        &Messages::default(),
    )
    .unwrap();
    assert_eq!(result.completed, 1);
    assert!(result.errors.is_empty());
    let mut z = ZipArchive::new(File::open(output).unwrap()).unwrap();
    let mut names = (0..z.len())
        .map(|i| z.by_index(i).unwrap().name().unwrap().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        ["ProfileInfo\\ElementTree.v3e", "Sequence\\NoAudio.tim"]
    );
}
