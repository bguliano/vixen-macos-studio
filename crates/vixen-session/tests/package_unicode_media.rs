//! A Mac-resolvable canonical filename alias must survive an unchanged-TIM Windows package.
//! No application services, native windows, audio devices, controllers, or external networking.
#![cfg(target_os = "macos")]
use std::os::unix::fs::MetadataExt;
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};
use uuid::Uuid;
use vixen_core::TimeSpan;
use vixen_effects::framework::{ParamValue, Params};
use vixen_io::{DataPaths, SystemData};
use vixen_session::{
    app::AppCore,
    element_mapper::ElementMapState,
    sequence::SequenceSession,
    sequence_package::{self, ExportSettings, Listener},
};

struct Messages;
impl Listener for Messages {
    fn progress(&self, _: &str, _: u32, _: u32) {}
    fn overwrite_package(&self, _: &Path) -> bool {
        false
    }
    fn sequence_destination(&self, _: &str, p: &Path) -> Option<PathBuf> {
        Some(p.to_owned())
    }
    fn warning(&self, m: &str) {
        panic!("Unexpected package warning: {m}")
    }
}
struct TempRoot {
    path: PathBuf,
    retain: bool,
}
impl Drop for TempRoot {
    fn drop(&mut self) {
        if !self.retain {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
struct SettingsGuard(Option<std::ffi::OsString>);
impl Drop for SettingsGuard {
    fn drop(&mut self) {
        if let Some(v) = &self.0 {
            std::env::set_var("VIXEN_SETTINGS_DIR", v)
        } else {
            std::env::remove_var("VIXEN_SETTINGS_DIR")
        }
    }
}
fn wav() -> Vec<u8> {
    let mut bytes = Vec::new();
    let samples: [i16; 8] = [0, 1024, -1024, 2048, -2048, 512, -512, 0];
    bytes.extend(b"RIFF");
    bytes.extend((36u32 + 16).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(8000u32.to_le_bytes());
    bytes.extend(16000u32.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(16u32.to_le_bytes());
    for s in samples {
        bytes.extend(s.to_le_bytes())
    }
    bytes
}
fn same_file(a: &Path, b: &Path) -> bool {
    let a = fs::metadata(a).unwrap();
    let b = fs::metadata(b).unwrap();
    (a.dev(), a.ino()) == (b.dev(), b.ino())
}
fn picture(name: &str, node: Uuid) -> vixen_io::EffectNode {
    let descriptor = vixen_effect_registry::all()
        .iter()
        .find(|d| d.name == "Picture")
        .unwrap();
    let mut params = Params::defaults(descriptor.params);
    params.set("FileName", ParamValue::Str(name.into()));
    vixen_io::EffectNode {
        start: TimeSpan::ZERO,
        time_span: TimeSpan::from_ms_i(100),
        type_id: descriptor.type_id,
        instance_id: Uuid::new_v4(),
        targets: vec![(node, "Private pixel".into())],
        params,
    }
}
#[test]
fn package_keeps_loadable_unicode_media_and_reference_spelling() {
    let report = std::env::var_os("VIXEN_PACKAGE_UNICODE_REPORT_ROOT");
    let root = report.as_ref().map(PathBuf::from).unwrap_or_else(|| {
        std::env::temp_dir().join(format!("vixen-package-unicode-{}", Uuid::new_v4()))
    });
    let _root_guard = TempRoot {
        path: root.clone(),
        retain: report.is_some(),
    };
    assert!(
        root.is_absolute() && !root.exists(),
        "A fresh private output folder is required"
    );
    fs::create_dir(&root).unwrap();
    let prior = std::env::var_os("VIXEN_SETTINGS_DIR");
    std::env::set_var("VIXEN_SETTINGS_DIR", root.join("settings"));
    let _settings = SettingsGuard(prior);
    let paths = DataPaths::new(root.join("profile"));
    paths.ensure_directories().unwrap();
    let mut system = SystemData::default();
    let node = system.tree.add_element("Private pixel");
    let id = system.tree.node(node).id;
    vixen_session::web::store_settings(
        &mut system.module_store,
        vixen_session::web::Settings {
            port: 8888,
            enabled: false,
        },
    );
    vixen_io::save_system(&system, &paths).unwrap();
    assert!(system.controllers.is_empty() && system.patches.is_empty());
    let initial_system = fs::read(paths.system_config_file()).unwrap();
    let initial_store = fs::read(paths.module_store_file()).unwrap();
    let media_nfc = "Caf\u{e9}.wav";
    let media_nfd = "Cafe\u{301}.wav";
    let picture_nfc = "\u{c9}toile.png";
    let picture_nfd = "E\u{301}toile.png";
    fs::write(paths.media_dir().join(media_nfd), wav()).unwrap();
    let pictures = paths.module_data_dir().join("Picture");
    fs::create_dir_all(&pictures).unwrap();
    image::RgbaImage::from_pixel(1, 1, image::Rgba([17, 119, 203, 255]))
        .save(pictures.join(picture_nfd))
        .unwrap();
    let audio_alias = same_file(
        &paths.media_dir().join(media_nfc),
        &paths.media_dir().join(media_nfd),
    );
    let picture_alias = same_file(&pictures.join(picture_nfc), &pictures.join(picture_nfd));
    assert!(
        audio_alias && picture_alias,
        "This integration requires a normalization-insensitive Mac filesystem"
    );
    let png = fs::read(pictures.join(picture_nfd)).unwrap();
    assert_eq!(image::open(pictures.join(picture_nfc)).unwrap().width(), 1);
    let actual_media = fs::read_dir(paths.media_dir())
        .unwrap()
        .map(|x| x.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(actual_media, [media_nfd]);
    let core = AppCore::open(paths.root().to_owned(), true).unwrap();
    let mut reports = Vec::new();
    let mut all = true;
    // Literal requires the exact original NFC spelling in the archive, not merely the NFD disk name.
    // Wildcards are discovery-only controls: '?' stays syntax and must never become a ZIP filename.
    // Aliases require both exact spellings for unchanged source references on a case-sensitive/non-normalizing receiver.
    for (case, media_refs, picture_refs, expected_media, expected_pictures, playable) in [
        (
            "Literal",
            vec![media_nfc],
            vec![picture_nfc],
            vec![media_nfc],
            vec![picture_nfc],
            true,
        ),
        (
            "Wildcard",
            vec!["Caf?.wav"],
            vec!["?toile.png"],
            vec![media_nfd],
            vec![picture_nfd],
            false,
        ),
        (
            "Aliases",
            vec![media_nfc, media_nfd],
            vec![picture_nfc, picture_nfd],
            vec![media_nfc, media_nfd],
            vec![picture_nfc, picture_nfd],
            true,
        ),
    ] {
        let mut sequence = vixen_io::new_sequence();
        sequence.length = TimeSpan::from_ms_i(100);
        for name in &media_refs {
            sequence.media.push(vixen_io::MediaEntry {
                type_id: uuid::uuid!("fe460392-3fab-4c63-99dd-d76c48354150"),
                instance_id: Uuid::new_v4(),
                file_name: (*name).into(),
                data: None,
            })
        }
        for name in &picture_refs {
            sequence.effects.push(picture(name, id))
        }
        let tim = paths.sequence_dir().join(format!("{case}.tim"));
        vixen_io::save_sequence(&sequence, &tim).unwrap();
        let original = fs::read(&tim).unwrap();
        let audio_frames = if playable {
            let session = SequenceSession::open(&tim, &core.session_environment()).unwrap();
            let frames = session
                .audio()
                .expect("Current real sequence session loads NFC reference through Mac alias")
                .frames();
            assert_eq!(frames, 8);
            Some(frames)
        } else {
            None
        };
        let output = root.join(format!("{case}.vpkg"));
        let result = sequence_package::export(
            &core,
            &ExportSettings {
                output: output.to_string_lossy().into(),
                audio: true,
                sequences: vec![tim.to_string_lossy().into()],
            },
            &AtomicBool::new(false),
            &Messages,
        )
        .unwrap();
        assert_eq!(result.completed, 1);
        assert!(!result.cancelled && result.errors.is_empty());
        let mut zip = zip::ZipArchive::new(File::open(&output).unwrap()).unwrap();
        let mut names = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().unwrap().into_owned())
            .collect::<Vec<_>>();
        names.sort();
        let mut raw = Vec::new();
        zip.by_name(&format!("Sequence\\{case}.tim"))
            .unwrap()
            .read_to_end(&mut raw)
            .unwrap();
        assert_eq!(raw, original);
        assert_eq!(fs::read(&tim).unwrap(), original);
        let mut expected = vec![
            format!("Sequence\\{case}.tim"),
            "ProfileInfo\\ElementTree.v3e".into(),
        ];
        let mut payloads = true;
        for (folder, refs, bytes) in [
            ("Media", expected_media.clone(), wav()),
            (
                "Module Data Files\\Picture",
                expected_pictures.clone(),
                png.clone(),
            ),
        ] {
            for name in refs {
                let entry = format!("SequenceData\\{case}\\{folder}\\{name}");
                expected.push(entry.clone());
                if let Ok(mut z) = zip.by_name(&entry) {
                    let mut data = Vec::new();
                    z.read_to_end(&mut data).unwrap();
                    payloads &= data == bytes
                } else {
                    payloads = false
                }
            }
        }
        expected.sort();
        let passed = names == expected && payloads;
        all &= passed;
        // Import through the production mapper/sequence/media pipeline into a fresh profile.
        // The strict ZIP checks above prove each exact Windows reference spelling even
        // though this Mac coalesces canonical aliases when they are extracted.
        let imported = DataPaths::new(root.join(format!("import-{case}")));
        imported.ensure_directories().unwrap();
        let mut destination = SystemData::default();
        let dest_node = destination.tree.add_element("Imported pixel");
        let dest_id = destination.tree.node(dest_node).id;
        vixen_session::web::store_settings(
            &mut destination.module_store,
            vixen_session::web::Settings {
                port: 8888,
                enabled: false,
            },
        );
        vixen_io::save_system(&destination, &imported).unwrap();
        let dest_core = AppCore::open(imported.root().to_owned(), true).unwrap();
        let mut map = ElementMapState::new(
            "Unicode package".into(),
            vec![(id, "Private pixel".into())],
            Arc::new(destination.tree.clone()),
        );
        map.assign(id, Some(dest_id)).unwrap();
        let map_path = root.join(format!("{case}.vmap"));
        map.save(&map_path).unwrap();
        let import_result = sequence_package::import(
            &dest_core,
            &output,
            &map_path,
            &[case.into()],
            &AtomicBool::new(false),
            &Messages,
        )
        .unwrap();
        assert_eq!(import_result.completed, 1);
        assert!(!import_result.cancelled && import_result.errors.is_empty());
        let imported_tim = imported.sequence_dir().join(format!("{case}.tim"));
        let loaded = vixen_io::load_sequence(&imported_tim).unwrap();
        assert_eq!(
            loaded
                .media
                .iter()
                .map(|m| m.file_name.as_str())
                .collect::<Vec<_>>(),
            media_refs
        );
        assert_eq!(loaded.effects.len(), picture_refs.len());
        for (effect, name) in loaded.effects.iter().zip(&picture_refs) {
            assert_eq!(effect.targets, vec![(dest_id, "Imported pixel".into())]);
            assert_eq!(effect.params.string("FileName"), *name);
        }
        for name in &expected_media {
            assert_eq!(fs::read(imported.media_dir().join(name)).unwrap(), wav());
        }
        for name in &expected_pictures {
            assert_eq!(
                fs::read(imported.module_data_dir().join("Picture").join(name)).unwrap(),
                png
            );
        }
        let imported_audio_frames = if playable {
            let session =
                SequenceSession::open(&imported_tim, &dest_core.session_environment()).unwrap();
            let frames = session
                .audio()
                .expect("Imported literal reference loads actual PCM")
                .frames();
            assert_eq!(frames, 8);
            for name in &picture_refs {
                let image = image::open(imported.module_data_dir().join("Picture").join(name))
                    .unwrap()
                    .into_rgba8();
                assert_eq!(image.get_pixel(0, 0).0, [17, 119, 203, 255]);
            }
            Some(frames)
        } else {
            None
        };
        reports.push(serde_json::json!({"case":case,"passed":passed,"sequenceAudioFrames":audio_frames,"importedAudioFrames":imported_audio_frames,"productionImportCompleted":true,"importedReferencesAndTargetsExact":true,"importedPayloadsExact":true,"mediaReferences":media_refs,"pictureReferences":picture_refs,"actualEntries":names,"expectedEntries":expected,"exactReferencedPayloads":payloads,"rawSequenceUnchanged":true}));
    }
    assert_eq!(
        fs::read(paths.system_config_file()).unwrap(),
        initial_system
    );
    assert_eq!(fs::read(paths.module_store_file()).unwrap(), initial_store);
    let report = serde_json::json!({"passed":all,"sameFileAliases":{"media":audio_alias,"picture":picture_alias},"actualMediaDiskName":actual_media,"cases":reports,"scope":"Actual production package export and import, exact referenced archive names, target mapping, normal TIM and Mac audio loading. No Windows execution or claim that NTFS normalizes. Wildcard case checks discovery, not playback of a wildcard audio path.","noNativeOrHardware":true});
    fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("Private package evidence: {}", root.display());
    assert!(all,"Production exporter omitted resolvable media or lost exact reference spelling; retained result.json and ZIPs");
}
