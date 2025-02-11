use log::{error, trace};
use std::path::{Path, PathBuf};
use std::{fmt, fs, os::unix};
use thiserror::Error;

/// Error in [`Directory`] adapter.
#[derive(Error, Clone, Debug)]
pub enum DirectoryError {
    /// Failed to create adapter.
    #[error("failed to read directory {path}: {message}")]
    CreateError { path: PathBuf, message: String },

    /// Failed to get size of the content.
    #[error("failed to get size of {path}: {message}")]
    GetSizeError { path: PathBuf, message: String },

    /// Failed to copy content.
    #[error("failed to copy directory content {src_path} -> {dst_path}: {message}")]
    CopyDirectoryContentError {
        src_path: PathBuf,
        dst_path: PathBuf,
        message: String,
    },

    /// Unknown type of the file.
    #[error("unknown type of the file: {path}")]
    UnknownFileType { path: PathBuf },
}

/// Directory adapter.
///
/// Provides some helpful methods to operate on directories.
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct Directory<'a> {
    path: &'a Path,
}

impl<'a> Directory<'a> {
    /// Create new adapter for directory at given path.
    ///
    /// # Errors
    ///
    /// - Returns error if `path` is not a directory.
    pub fn from_path(path: &'a Path) -> Result<Self, DirectoryError> {
        let metadata = fs::metadata(path).map_err(|e| DirectoryError::CreateError {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        if metadata.is_dir() {
            Ok(Self { path })
        } else {
            Err(DirectoryError::CreateError {
                path: path.to_path_buf(),
                message: "not a directory".to_string(),
            })
        }
    }

    /// Path to directory.
    pub fn path(&self) -> &'a Path {
        self.path
    }

    /// Size of all content in directory in bytes.
    pub fn size(&self) -> Result<u64, DirectoryError> {
        fs_extra::dir::get_size(&self.path).map_err(|e| DirectoryError::GetSizeError {
            path: self.path.to_path_buf(),
            message: e.to_string(),
        })
    }

    /// Copy all the content of the directory into another directory.
    ///
    /// This function **DOES NOT** follow symlinks.
    /// They are copied as they are.
    /// So it is possible to copy a symlink to non-existing file.
    pub fn copy_content(&self, target_dir: &Path) -> Result<(), DirectoryError> {
        let mut stack = Vec::new();
        stack.push(self.path.to_path_buf());

        let output_root = target_dir.to_path_buf();
        let input_root = self.path.to_path_buf().components().count();

        let io_err_handler = |err: std::io::Error| DirectoryError::CopyDirectoryContentError {
            src_path: self.path.to_path_buf(),
            dst_path: target_dir.to_path_buf(),
            message: err.to_string(),
        };
        let cannot_get_filename = DirectoryError::CopyDirectoryContentError {
            src_path: self.path.to_path_buf(),
            dst_path: target_dir.to_path_buf(),
            message: "cannot get filename".to_string(),
        };

        while let Some(working_path) = stack.pop() {
            trace!("entering: {}", working_path.display());
            let dest = output_root.join(
                working_path
                    .components()
                    .skip(input_root)
                    .collect::<PathBuf>(),
            );

            if !dest.is_dir() {
                trace!(" mkdir: {}", dest.display());
                fs::create_dir_all(&dest).map_err(io_err_handler)?;
            }

            for entry in fs::read_dir(&working_path).map_err(io_err_handler)? {
                let entry = entry.map_err(io_err_handler)?;
                let path = entry.path();

                if path.is_dir() {
                    stack.push(path);
                } else if path.is_file() {
                    let filename = path.file_name().ok_or(cannot_get_filename.clone())?;
                    let dest_path = dest.join(filename);
                    trace!("  copy: {} -> {}", path.display(), dest_path.display());
                    fs::copy(&path, &dest_path).map_err(io_err_handler)?;
                } else if path.is_symlink() {
                    let target = fs::read_link(&path).map_err(io_err_handler)?;
                    let filename = path.file_name().ok_or(cannot_get_filename.clone())?;
                    let dest_path = dest.join(filename);
                    trace!("  symlink {} -> {}", dest_path.display(), target.display());
                    // NOTE: unix module is used here, so this won't work on Windows.
                    // It's not a problem for now, since we are not targeting Windows for now.
                    unix::fs::symlink(target, dest_path).map_err(io_err_handler)?;
                } else {
                    return Err(DirectoryError::UnknownFileType { path });
                }
            }
            trace!("exiting: {}", working_path.display());
        }
        Ok(())
    }
}

impl<'a> fmt::Debug for Directory<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.path.fmt(f)
    }
}

impl<'a> fmt::Display for Directory<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format!("{}", self.path.display()))
    }
}

impl<'a> From<&'a Path> for Directory<'a> {
    fn from(value: &'a Path) -> Self {
        Self { path: value }
    }
}

#[cfg(test)]
mod tests {
    use super::{Directory, DirectoryError};
    use anyhow::Result;
    use std::fs;
    use std::io::{Read, Write};
    use std::path::Path;
    use tempdir::TempDir;

    #[test]
    fn test_from_path_ok() -> Result<()> {
        let tmp = TempDir::new("test-dirs")?;
        let _ = Directory::from_path(tmp.path())?;
        Ok(())
    }

    #[test]
    fn test_from_path_fail() -> Result<()> {
        let res = Directory::from_path(Path::new("/foo/bar/baz"));
        assert!(res.is_err());
        assert!(matches!(
            res.err().unwrap(),
            DirectoryError::CreateError { .. }
        ));
        Ok(())
    }

    #[test]
    fn test_size() -> Result<()> {
        let tmp = TempDir::new("test-dirs")?;
        let mut file = fs::File::create_new(tmp.path().join("tmp"))?;
        file.write_all(b"123")?;
        let directory = Directory::from_path(tmp.path())?;
        assert_eq!(directory.size()?, 3);
        Ok(())
    }

    #[test]
    fn test_copy() -> Result<()> {
        let tmp1 = TempDir::new("test-dirs")?;
        let tmp2 = TempDir::new("test-dirs")?;
        let mut file = fs::File::create_new(tmp1.path().join("tmp"))?;
        file.write_all(b"123")?;
        let directory = Directory::from_path(tmp1.path())?;
        directory.copy_content(tmp2.path())?;
        let mut file = fs::File::open(tmp2.path().join("tmp"))?;
        let mut buf = vec![];
        file.read_to_end(&mut buf)?;
        assert_eq!(buf, b"123");
        Ok(())
    }
}
