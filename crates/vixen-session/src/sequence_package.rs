//! Sequence Package Export/Import (TimedSequenceMapper). Packages retain Windows
//! entry names and raw source sequences; imports remap only an effect's first target.
use crate::{app::AppCore, element_mapper::ElementMapState, SessionError};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use unicode_normalization::UnicodeNormalization;
use uuid::{uuid, Uuid};
use vixen_core::ElementTree;
use vixen_io::{DataPaths, ModuleEntry, ModuleStore, TimedSequence};
use vixen_xml::{XElement, ARRAYS_NS};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

pub const MODULE_TYPE: Uuid = uuid!("b13997c2-d4b5-4b5a-9047-1dced97ad99b");
const NS: &str = "http://schemas.datacontract.org/2004/07/VixenModules.App.TimedSequenceMapper";
const LOGGER: &str = "VixenModules.App.TimedSequenceMapper";
pub mod wizard;
fn error(e: impl std::fmt::Display) -> SessionError {
    SessionError::Other(e.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportSettings {
    pub output: String,
    pub audio: bool,
    pub sequences: Vec<String>,
}
impl ExportSettings {
    pub fn load(store: &ModuleStore, paths: &DataPaths) -> Self {
        let d = store.type_data(MODULE_TYPE).and_then(ModuleEntry::data);
        Self {
            output: d
                .and_then(|d| d.child_text("ExportOutputFile"))
                .unwrap_or_else(|| paths.root().join("Export").to_string_lossy().into()),
            audio: d
                .and_then(|d| d.child_text("ExportIncludeAudio"))
                .map_or(true, |s| s == "true"),
            sequences: d
                .and_then(|d| d.child("ExportSequenceFiles"))
                .map(|d| d.elements().map(XElement::text).collect())
                .unwrap_or_default(),
        }
    }
    fn xml(&self) -> XElement {
        let mut d = XElement::new(NS, "TimedSequencePackagerData");
        d.push(XElement::with_text(
            "",
            "ModuleInstanceId",
            MODULE_TYPE.to_string(),
        ));
        d.push(XElement::with_text(
            "",
            "ModuleTypeId",
            MODULE_TYPE.to_string(),
        ));
        d.push(XElement::with_text(
            NS,
            "ExportIncludeAudio",
            self.audio.to_string(),
        ));
        d.push(XElement::with_text(NS, "ExportOutputFile", &self.output));
        let mut list = XElement::new(NS, "ExportSequenceFiles");
        for path in &self.sequences {
            list.push(XElement::with_text(ARRAYS_NS, "string", path));
        }
        d.push(list);
        d
    }
    pub fn store(&self, store: &mut ModuleStore, paths: &DataPaths) {
        let old = Self::load(store, paths).xml();
        if let Some(entry) = store.type_data_mut(MODULE_TYPE) {
            entry.merge_data(&old, self.xml());
        } else {
            store.set_type_data(ModuleEntry::new("VixenModules.App.TimedSequenceMapper.TimedSequencePackagerData, TimedSequenceMapper",&MODULE_TYPE,&MODULE_TYPE,"Module.App.TimedSequenceMapper",self.xml()));
        }
    }
    pub fn enter_output(&mut self, paths: &DataPaths, profile: Option<&str>) {
        let last = self.output.rsplit(['/', '\\']).next().unwrap_or_default();
        if Path::new(last).extension().is_none() {
            let folder = if Path::new(&self.output).is_dir() {
                PathBuf::from(&self.output)
            } else {
                paths.root().join("Export")
            };
            self.output = folder
                .join(format!(
                    "{}.vpkg",
                    profile.filter(|s| !s.is_empty()).unwrap_or("show")
                ))
                .to_string_lossy()
                .into();
        }
    }
    pub fn output_valid(&self) -> bool {
        let p = Path::new(&self.output);
        p.extension().is_some() && p.parent().is_some_and(Path::is_dir)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageSequence {
    pub name: String,
    pub entry: usize,
    pub selected: bool,
}
#[derive(Clone, Debug)]
pub struct PackageIndex {
    pub sequences: Vec<PackageSequence>,
    pub tree: Option<String>,
}

/// Normalize both Windows and Unix package names before matching/extracting. Reject
/// traversal and drive/absolute names even for entries the UI would otherwise ignore.
fn safe_parts(name: &str) -> Result<Vec<String>, SessionError> {
    if name.starts_with(['/', '\\']) || name.contains('\0') {
        return Err(error("Package contains an absolute or invalid path."));
    }
    let parts: Vec<_> = name
        .split(['/', '\\'])
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect();
    if parts
        .iter()
        .any(|p| p == ".." || p == "." || p.contains(':'))
    {
        return Err(error("Package contains an unsafe path."));
    }
    Ok(parts)
}
pub fn inspect(path: &Path) -> Result<PackageIndex, SessionError> {
    let mut zip = ZipArchive::new(File::open(path).map_err(error)?).map_err(error)?;
    let mut sequences = vec![];
    let mut tree = None;
    let mut names = HashSet::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(error)?;
        let parts = safe_parts(&entry.name().map_err(error)?)?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(error("Package contains a symbolic link."));
        }
        if parts.len() == 2
            && parts[0] == "Sequence"
            && parts[1].ends_with(".tim")
            && !entry.is_dir()
        {
            let name = parts[1].strip_suffix(".tim").unwrap().to_string();
            if !names.insert(name.clone()) {
                return Err(error(format!(
                    "Package contains duplicate sequence name: {name}"
                )));
            }
            sequences.push(PackageSequence {
                name,
                entry: i,
                selected: true,
            });
        }
        if parts == ["ProfileInfo", "ElementTree.v3e"] {
            let mut text = String::new();
            entry.read_to_string(&mut text).map_err(error)?;
            tree = Some(text);
        }
    }
    if sequences.is_empty() {
        return Err(error("The package contains no timed sequences."));
    }
    Ok(PackageIndex { sequences, tree })
}

/// Runs only on the worker. Prompts block that worker, never the UI actor.
pub trait Listener: Send + Sync {
    fn progress(&self, task: &str, task_percent: u32, overall_percent: u32);
    fn overwrite_package(&self, path: &Path) -> bool;
    /// Existing sequence: original path for overwrite, another path for Save As, None to skip.
    fn sequence_destination(&self, name: &str, path: &Path) -> Option<PathBuf>;
    fn warning(&self, message: &str);
}
#[derive(Clone, Debug, Default)]
pub struct RunResult {
    pub cancelled: bool,
    pub completed: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}
struct TempPath(PathBuf);
impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn cancelled(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Acquire)
}

/// `Directory.GetFiles` search masks, case-insensitive. No path components are
/// accepted: upstream aborts the remaining discovery on a rooted file name.
fn wildcard(pattern: &str, name: &str) -> bool {
    // APFS/HFS+ can resolve either canonical spelling of a filename. Match the
    // same spelling here, including one canonical scalar per '?' position.
    // NFC deliberately does not remove accents or fold compatibility characters.
    let p: Vec<_> = pattern.to_lowercase().nfc().collect();
    let n: Vec<_> = name.to_lowercase().nfc().collect();
    let (mut i, mut j, mut star, mut retry) = (0, 0, None, 0);
    while j < n.len() {
        if i < p.len() && (p[i] == '?' || p[i] == n[j]) {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == '*' {
            star = Some(i);
            i += 1;
            retry = j;
        } else if let Some(s) = star {
            retry += 1;
            j = retry;
            i = s + 1;
        } else {
            return false;
        }
    }
    while i < p.len() && p[i] == '*' {
        i += 1;
    }
    i == p.len()
}
fn files(folder: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut found = vec![];
    let mut pending = vec![folder.to_owned()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for e in entries.flatten() {
            let Ok(kind) = e.file_type() else { continue };
            if kind.is_dir() && recursive {
                pending.push(e.path());
            } else if kind.is_file() {
                found.push(e.path());
            }
        }
    }
    found.sort();
    found
}
fn media_patterns(root: &XElement) -> Vec<String> {
    let mut out = vec![];
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if matches!(n.local.as_str(), "FileName" | "Glediator_FileName") && !n.text().is_empty() {
            out.push(n.text());
        }
        stack.extend(n.elements().collect::<Vec<_>>().into_iter().rev());
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PackageMedia {
    source: PathBuf,
    archive_name: String,
}

fn media_match(source: &Path, pattern: &str) -> PackageMedia {
    let name = source.file_name().unwrap().to_string_lossy();
    // The raw TIM is retained. A literal reference whose canonical spelling
    // differs therefore needs that exact archive basename for Windows, where
    // NFC/NFD names need not identify the same file. Case-only matches keep the
    // existing disk name; wildcard syntax must never become an archive name.
    let alias = !pattern.contains(['*', '?']) && pattern.to_lowercase() != name.to_lowercase();
    PackageMedia {
        source: source.to_owned(),
        archive_name: if alias {
            pattern.to_owned()
        } else {
            name.into_owned()
        },
    }
}

fn unique_media(files: Vec<PackageMedia>) -> Result<Vec<PackageMedia>, SessionError> {
    let mut sources = HashMap::new();
    let mut result = Vec::new();
    for file in files {
        let alias = file.source.with_file_name(&file.archive_name);
        if let Some(source) = sources.get(&alias) {
            if source != &file.source {
                return Err(error(format!(
                    "Different media files map to the same package entry: {}",
                    alias.display()
                )));
            }
        } else {
            sources.insert(alias, file.source.clone());
            result.push(file);
        }
    }
    Ok(result)
}
fn discover(
    path: &Path,
    module_files: &[PathBuf],
    media_files: &[PathBuf],
) -> Result<(Vec<PackageMedia>, Vec<PackageMedia>), SessionError> {
    let Ok(root) = XElement::from_file(path) else {
        crate::logging::warn(LOGGER, "Unable to parse sequence while discovering media.");
        return Ok((vec![], vec![]));
    };
    let (mut modules, mut media) = (vec![], vec![]);
    for pattern in media_patterns(&root) {
        if pattern.contains(['/', '\\', ':']) {
            crate::logging::warn(
                LOGGER,
                "Rooted media name stops package media discovery for this sequence.",
            );
            break;
        }
        for (input, output) in [(module_files, &mut modules), (media_files, &mut media)] {
            for p in input {
                if wildcard(
                    &pattern,
                    p.file_name().and_then(|s| s.to_str()).unwrap_or_default(),
                ) {
                    output.push(media_match(p, &pattern));
                }
            }
        }
    }
    // Keep distinct literal aliases of one source, but reject an archive-name
    // collision between different sources instead of silently dropping bytes.
    Ok((unique_media(modules)?, unique_media(media)?))
}
fn add_file(
    zip: &mut ZipWriter<File>,
    path: &Path,
    name: &str,
    output: &Path,
    cancel: &AtomicBool,
) -> Result<(), SessionError> {
    if path == output {
        return Ok(());
    }
    zip.start_file(
        name,
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
    )
    .map_err(error)?;
    copy_cancellable(&mut File::open(path).map_err(error)?, zip, cancel)?;
    Ok(())
}
fn copy_cancellable(
    input: &mut impl Read,
    output: &mut impl Write,
    cancel: &AtomicBool,
) -> Result<(), SessionError> {
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if cancelled(cancel) {
            return Err(error("Package operation canceled."));
        }
        let n = input.read(&mut buffer).map_err(error)?;
        if n == 0 {
            return Ok(());
        }
        output.write_all(&buffer[..n]).map_err(error)?;
    }
}
pub fn export(
    core: &AppCore,
    settings: &ExportSettings,
    cancel: &AtomicBool,
    listener: &dyn Listener,
) -> Result<RunResult, SessionError> {
    let output = Path::new(&settings.output);
    if output.exists() && !listener.overwrite_package(output) {
        return Ok(RunResult {
            cancelled: true,
            ..Default::default()
        });
    }
    if cancelled(cancel) {
        return Ok(RunResult {
            cancelled: true,
            ..Default::default()
        });
    }
    let temp = TempPath(PathBuf::from(format!(
        "{}.part-{}",
        output.display(),
        Uuid::new_v4()
    )));
    let mut zip = ZipWriter::new(File::create(&temp.0).map_err(error)?);
    let module_root = core.paths.module_data_dir();
    let media_root = core.paths.media_dir();
    let module_files = files(&module_root, true);
    let media_files = if settings.audio {
        files(&media_root, false)
    } else {
        vec![]
    };
    let total = settings.sequences.len() * 4 + 2;
    let mut done = 0;
    let mut result = RunResult::default();
    for file in &settings.sequences {
        if cancelled(cancel) {
            result.cancelled = true;
            return Ok(result);
        }
        let path = Path::new(file);
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| error("Invalid sequence filename."))?;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(filename);
        listener.progress(&format!("Packaging {name}"), 0, (done * 100 / total) as u32);
        add_file(
            &mut zip,
            path,
            &format!("Sequence\\{filename}"),
            output,
            cancel,
        )?;
        done += 1;
        listener.progress(
            &format!("Discovering any media for {name}"),
            25,
            (done * 100 / total) as u32,
        );
        let (modules, media) = discover(path, &module_files, &media_files)?;
        done += 1;
        if cancelled(cancel) {
            result.cancelled = true;
            return Ok(result);
        }
        listener.progress(
            &format!("Packaging effect media for {name}"),
            50,
            (done * 100 / total) as u32,
        );
        for file in modules {
            let rel = file.source.strip_prefix(&module_root).map_err(error)?;
            let rel = rel
                .with_file_name(&file.archive_name)
                .to_string_lossy()
                .replace('/', "\\");
            add_file(
                &mut zip,
                &file.source,
                &format!("SequenceData\\{name}\\Module Data Files\\{rel}"),
                output,
                cancel,
            )?;
        }
        done += 1;
        listener.progress(
            &format!("Packaging audio for {name}"),
            75,
            (done * 100 / total) as u32,
        );
        if settings.audio {
            for file in media {
                let filename = &file.archive_name;
                add_file(
                    &mut zip,
                    &file.source,
                    &format!("SequenceData\\{name}\\Media\\{filename}"),
                    output,
                    cancel,
                )?;
            }
        }
        done += 1;
        result.completed += 1;
        listener.progress(
            &format!("Packaging {name}"),
            100,
            (done * 100 / total) as u32,
        );
    }
    if cancelled(cancel) {
        result.cancelled = true;
        return Ok(result);
    }
    listener.progress(
        "Creating Profile Element Map",
        0,
        (done * 100 / total) as u32,
    );
    let tree = crate::setup::tree::SetupTree::from_element_tree(&core.system.read().tree);
    let tree = crate::setup::export::element_tree_json(&tree, Uuid::new_v4());
    done += 1;
    listener.progress(
        "Archiving Profile Element Map",
        50,
        (done * 100 / total) as u32,
    );
    zip.start_file(
        "ProfileInfo\\ElementTree.v3e",
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
    )
    .map_err(error)?;
    zip.write_all(tree.as_bytes()).map_err(error)?;
    zip.finish().map_err(error)?.sync_all().map_err(error)?;
    if cancelled(cancel) {
        result.cancelled = true;
        return Ok(result);
    }
    fs::rename(&temp.0, output).map_err(error)?;
    listener.progress("Completed", 100, 100);
    Ok(result)
}

pub fn remap_sequence(sequence: &mut TimedSequence, pairs: &[(Uuid, Uuid)], tree: &ElementTree) {
    let map: HashMap<_, _> = pairs.iter().copied().collect();
    sequence.effects.retain_mut(|effect| {
        let Some(target) = effect
            .targets
            .first()
            .and_then(|(id, _)| map.get(id))
            .and_then(|id| tree.find(id))
        else {
            return false;
        };
        let node = tree.node(target);
        effect.targets = vec![(node.id, node.name.clone())];
        true
    });
}
fn safe_destination(root: &Path, parts: &[String]) -> Result<PathBuf, SessionError> {
    let mut path = root.to_owned();
    // Never follow existing symlinks during archive extraction.
    if root
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(error("Package destination is a symbolic link."));
    }
    for p in parts {
        path.push(p);
        if path
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(error("Package destination contains a symbolic link."));
        }
    }
    Ok(path)
}
fn extract_media(
    zip: &mut ZipArchive<File>,
    name: &str,
    kind: &str,
    root: &Path,
    cancel: &AtomicBool,
) -> Result<(), SessionError> {
    for i in 0..zip.len() {
        if cancelled(cancel) {
            return Ok(());
        }
        let mut entry = zip.by_index(i).map_err(error)?;
        let parts = safe_parts(&entry.name().map_err(error)?)?;
        if parts.len() < 4
            || parts[0] != "SequenceData"
            || parts[1] != name
            || parts[2] != kind
            || entry.is_dir()
        {
            continue;
        }
        let target = safe_destination(root, &parts[3..])?;
        if target.exists() {
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(error)?;
        }
        let temp = TempPath(target.with_file_name(format!(".vixen-import-{}", Uuid::new_v4())));
        let mut out = File::create(&temp.0).map_err(error)?;
        copy_cancellable(&mut entry, &mut out, cancel)?;
        out.sync_all().map_err(error)?;
        if cancelled(cancel) {
            return Ok(());
        }
        fs::rename(&temp.0, target).map_err(error)?;
    }
    Ok(())
}
pub fn import(
    core: &AppCore,
    package: &Path,
    map_path: &Path,
    selected: &[String],
    cancel: &AtomicBool,
    listener: &dyn Listener,
) -> Result<RunResult, SessionError> {
    let index = inspect(package)?;
    let tree = Arc::new(core.system.read().tree.clone());
    let mut map = ElementMapState::new(String::new(), vec![], Arc::clone(&tree));
    map.load(map_path)?;
    let pairs = map.accepted_pairs();
    let store = core.system.read().module_store.clone();
    let mut zip = ZipArchive::new(File::open(package).map_err(error)?).map_err(error)?;
    let total = index.sequences.len() * 6;
    let mut done = 0;
    let mut result = RunResult::default();
    for seq in index
        .sequences
        .iter()
        .filter(|s| selected.contains(&s.name))
    {
        if cancelled(cancel) {
            result.cancelled = true;
            break;
        }
        let initial = core.paths.sequence_dir().join(format!("{}.tim", seq.name));
        let destination = if initial.exists() {
            match listener.sequence_destination(&seq.name, &initial) {
                Some(path) => path,
                None => {
                    result.skipped += 1;
                    continue;
                }
            }
        } else {
            initial
        };
        let run = (|| -> Result<(), SessionError> {
            listener.progress(
                &format!("Extracting {} from package", seq.name),
                0,
                (done * 100 / total) as u32,
            );
            let temp = TempPath(
                core.paths
                    .sequence_dir()
                    .join(format!(".vixen-import-{}.tim", Uuid::new_v4())),
            );
            let mut entry = zip.by_index(seq.entry).map_err(error)?;
            let mut out = File::create(&temp.0).map_err(error)?;
            copy_cancellable(&mut entry, &mut out, cancel)?;
            drop(out);
            drop(entry);
            done += 1;
            listener.progress(
                &format!("Loading {}", seq.name),
                16,
                (done * 100 / total) as u32,
            );
            let mut sequence = vixen_io::sequence::load_sequence_with_store(&temp.0, &store)?;
            done += 1;
            listener.progress(
                &format!("Remapping {}", seq.name),
                33,
                (done * 100 / total) as u32,
            );
            remap_sequence(&mut sequence, &pairs, &tree);
            done += 1;
            if cancelled(cancel) {
                return Ok(());
            }
            listener.progress(
                &format!("Saving mapped sequence {}", seq.name),
                50,
                (done * 100 / total) as u32,
            );
            // The original SequenceService.Load runs the version 0 media copy
            // before saving. Restore reachable sources before their absolute
            // paths disappear from the migrated document, keeping the package
            // destination's existing symlink boundary.
            let media_dir = safe_destination(&core.paths.media_dir(), &[])?;
            sequence.restore_migrated_media(&media_dir)?;
            let text = vixen_io::sequence::sequence_to_document(&sequence)?;
            // Parse/map in a private file before replacing the destination. This also
            // avoids creating migration backups that upstream immediately deletes.
            vixen_io::file::write_atomic(&destination, text.as_bytes())?;
            done += 1;
            listener.progress(
                &format!("Extracting module data for {}", seq.name),
                66,
                (done * 100 / total) as u32,
            );
            extract_media(
                &mut zip,
                &seq.name,
                "Module Data Files",
                &core.paths.module_data_dir(),
                cancel,
            )?;
            done += 1;
            listener.progress(
                &format!("Extracting media/audio for {}", seq.name),
                83,
                (done * 100 / total) as u32,
            );
            extract_media(
                &mut zip,
                &seq.name,
                "Media",
                &core.paths.media_dir(),
                cancel,
            )?;
            done += 1;
            result.completed += 1;
            Ok(())
        })();
        if let Err(e) = run {
            if cancelled(cancel) {
                result.cancelled = true;
                break;
            }
            let message = format!("An error occurred importing sequence {}: {e}", seq.name);
            listener.warning(&message);
            result.errors.push(message);
        }
    }
    result.cancelled = cancelled(cancel);
    if !result.cancelled {
        listener.progress("Completed", 100, 100)
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
