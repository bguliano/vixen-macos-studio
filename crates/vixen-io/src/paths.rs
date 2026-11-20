//! Port of Vixen.Core/Sys/Paths.cs and the `[DataPath]` folders it creates under the data root.

use crate::error::IoError;
use std::path::{Path, PathBuf};

/// `Paths.VIXEN_DATA_DIR`.
pub const VIXEN_DATA_DIR: &str = "Vixen 3";

/// `XElementFileWriter.BackupFolder`.
pub const BACKUP_FOLDER: &str = "auto_backup";

/// Each module's `[ModuleDataPath]`, created under `Module Data Files` when the modules load.
const MODULE_DATA_FOLDERS: [&str; 5] = ["Glediator", "Picture", "Shapes", "Video", "VixenDisplayPreview"];

/// The folders of one Vixen profile. Nothing here touches the file system except
/// [`DataPaths::ensure_directories`], the port of `Paths._BuildDataDirectories`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DataPaths {
    root: PathBuf,
}

impl DataPaths {
    /// `Paths.DataRootPath = root`, without creating anything.
    pub fn new(root: impl Into<PathBuf>) -> DataPaths {
        DataPaths { root: root.into() }
    }

    /// `Paths.DefaultDataRootPath`: `~/Documents/Vixen 3`. `None` when `HOME` is not set.
    pub fn default_root() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Documents").join(VIXEN_DATA_DIR))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `SequenceService.SequenceDirectory`.
    pub fn sequence_dir(&self) -> PathBuf {
        self.root.join("Sequence")
    }

    /// `SequenceService.SequenceBackupDirectory`.
    pub fn sequence_backup_dir(&self) -> PathBuf {
        self.sequence_dir().join(BACKUP_FOLDER)
    }

    /// `MediaService.MediaDirectory`.
    pub fn media_dir(&self) -> PathBuf {
        self.root.join("Media")
    }

    /// `SystemConfig.Directory`.
    pub fn system_data_dir(&self) -> PathBuf {
        self.root.join("SystemData")
    }

    /// The `auto_backup` folder `XElementFileWriter` uses for system files.
    pub fn system_backup_dir(&self) -> PathBuf {
        self.system_data_dir().join(BACKUP_FOLDER)
    }

    /// `Paths.ModuleDataFilesPath`. An effect's files sit under `<this>/<EffectName>`.
    pub fn module_data_dir(&self) -> PathBuf {
        self.root.join("Module Data Files")
    }

    /// `Program.ProgramDirectory`.
    pub fn program_dir(&self) -> PathBuf {
        self.root.join("Program")
    }

    /// `ElementNodeTemplate.Directory`.
    pub fn element_template_dir(&self) -> PathBuf {
        self.root.join("Template").join("Element")
    }

    /// `EffectDefaultsService.Directory`.
    pub fn effect_defaults_dir(&self) -> PathBuf {
        self.root.join("Effect Defaults")
    }

    /// The folder fixture specifications load from.
    pub fn fixtures_dir(&self) -> PathBuf {
        self.root.join("Fixtures")
    }

    /// `Export.ExportDir`'s default.
    pub fn export_dir(&self) -> PathBuf {
        self.root.join("Export")
    }

    /// The folder the log configuration writes to.
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Where every profile's logs go: `~/Documents/Vixen 3/logs`, as `NLog.config` has it. With `VIXEN_DATA` set, which only tests do,
    /// it's that folder's `logs`, so a test never touches the real ones. `None` with neither `VIXEN_DATA` nor `HOME`: there is no file
    /// log then, rather than one relative to the working directory.
    pub fn shared_logs_dir() -> Option<PathBuf> {
        shared_logs_dir_from(|name| std::env::var_os(name))
    }

    /// `SystemConfig.DefaultFilePath`.
    pub fn system_config_file(&self) -> PathBuf {
        self.system_data_dir().join("SystemConfig.xml")
    }

    /// `ModuleStore.DefaultFilePath`.
    pub fn module_store_file(&self) -> PathBuf {
        self.system_data_dir().join("ModuleStore.xml")
    }

    /// `VixenApplicationData.DataFilepath`.
    pub fn application_data_file(&self) -> PathBuf {
        self.root.join("VixenApplicationData.xml")
    }

    /// `Paths._BuildDataDirectories`: creates the folders Vixen expects to exist.
    pub fn ensure_directories(&self) -> Result<(), IoError> {
        for dir in [
            self.root.clone(),
            self.sequence_dir(),
            self.sequence_backup_dir(),
            self.media_dir(),
            self.system_data_dir(),
            self.module_data_dir(),
            self.program_dir(),
            self.element_template_dir(),
            self.effect_defaults_dir(),
        ]
        .into_iter()
        .chain(MODULE_DATA_FOLDERS.iter().map(|name| self.module_data_dir().join(name)))
        {
            std::fs::create_dir_all(&dir).map_err(|e| IoError::io(&dir, e))?;
        }
        Ok(())
    }
}

fn shared_logs_dir_from(env: impl Fn(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf> {
    let root = env("VIXEN_DATA").map(PathBuf::from).or_else(|| env("HOME").map(|home| PathBuf::from(home).join("Documents").join(VIXEN_DATA_DIR)))?;
    Some(root.join("logs"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_are_where_paths_cs_puts_them() {
        let p = DataPaths::new("/x/Vixen 3");
        assert_eq!(p.sequence_dir(), PathBuf::from("/x/Vixen 3/Sequence"));
        assert_eq!(p.sequence_backup_dir(), PathBuf::from("/x/Vixen 3/Sequence/auto_backup"));
        assert_eq!(p.media_dir(), PathBuf::from("/x/Vixen 3/Media"));
        assert_eq!(p.system_data_dir(), PathBuf::from("/x/Vixen 3/SystemData"));
        assert_eq!(p.module_data_dir(), PathBuf::from("/x/Vixen 3/Module Data Files"));
        assert_eq!(p.program_dir(), PathBuf::from("/x/Vixen 3/Program"));
        assert_eq!(p.element_template_dir(), PathBuf::from("/x/Vixen 3/Template/Element"));
        assert_eq!(p.effect_defaults_dir(), PathBuf::from("/x/Vixen 3/Effect Defaults"));
        assert_eq!(p.system_config_file(), PathBuf::from("/x/Vixen 3/SystemData/SystemConfig.xml"));
        assert_eq!(p.module_store_file(), PathBuf::from("/x/Vixen 3/SystemData/ModuleStore.xml"));
        assert_eq!(p.application_data_file(), PathBuf::from("/x/Vixen 3/VixenApplicationData.xml"));
    }

    #[test]
    fn ensure_directories_makes_the_five_module_folders() {
        let dir = crate::test_dir("paths-ensure");
        let paths = DataPaths::new(dir.join("Vixen 3"));
        paths.ensure_directories().unwrap();
        for name in MODULE_DATA_FOLDERS {
            assert!(paths.module_data_dir().join(name).is_dir(), "{name}");
        }
        assert!(paths.sequence_backup_dir().is_dir());
        assert!(paths.element_template_dir().is_dir());
        assert!(paths.effect_defaults_dir().is_dir());
    }

    #[test]
    fn shared_logs_dir_is_never_relative() {
        let env = |pairs: &'static [(&'static str, &'static str)]| move |name: &str| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| v.into());
        assert_eq!(shared_logs_dir_from(env(&[("HOME", "/owner")])), Some(PathBuf::from("/owner/Documents/Vixen 3/logs")));
        assert_eq!(shared_logs_dir_from(env(&[("HOME", "/owner"), ("VIXEN_DATA", "/test")])), Some(PathBuf::from("/test/logs")));
        assert_eq!(shared_logs_dir_from(env(&[])), None);
    }
}
