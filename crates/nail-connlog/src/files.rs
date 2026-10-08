//! Size-capped rotating text files: the connection logs on disk and
//! `app.log` (ADR 0190). `name.log` is appended to; at the cap it becomes
//! `name.log.1` (the older ones shift up to [`LOG_FILE_GENERATIONS`]).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub use nail_api::connlog::LOG_FILE_GENERATIONS;

/// One rotating log file.
#[derive(Debug)]
pub struct RotatingFile {
    path: PathBuf,
    max_bytes: u64,
    file: Option<File>,
    size: u64,
}

/// The path of generation `n` (`0` = the file itself).
fn generation(path: &Path, n: usize) -> PathBuf {
    if n == 0 {
        return path.to_owned();
    }
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{n}"));
    PathBuf::from(name)
}

impl RotatingFile {
    /// A file at `path` rotated at `max_bytes` (created on the first line).
    pub fn new(path: PathBuf, max_bytes: u64) -> Self {
        Self {
            path,
            max_bytes,
            file: None,
            size: 0,
        }
    }

    /// The file's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn open(&mut self) -> io::Result<&mut File> {
        if self.file.is_none() {
            if let Some(dir) = self.path.parent() {
                fs::create_dir_all(dir)?;
            }
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            self.size = file.metadata()?.len();
            self.file = Some(file);
        }
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("log file not open"))
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file = None;
        for n in (1..=LOG_FILE_GENERATIONS).rev() {
            let from = generation(&self.path, n - 1);
            if from.exists() {
                fs::rename(&from, generation(&self.path, n))?;
            }
        }
        self.size = 0;
        Ok(())
    }

    /// Appends one line (a line break is added) and flushes it.
    pub fn append_line(&mut self, line: &str) -> io::Result<()> {
        let bytes = u64::try_from(line.len()).unwrap_or(u64::MAX) + 1;
        self.open()?;
        if self.size > 0 && self.size.saturating_add(bytes) > self.max_bytes {
            self.rotate()?;
        }
        let file = self.open()?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        file.flush()?;
        self.size = self.size.saturating_add(bytes);
        Ok(())
    }

    /// Deletes the file and its older generations.
    pub fn remove(mut self) -> io::Result<()> {
        self.file = None;
        remove_generations(&self.path)
    }
}

/// Deletes `path` and its older generations (missing files are fine).
pub fn remove_generations(path: &Path) -> io::Result<()> {
    for n in 0..=LOG_FILE_GENERATIONS {
        match fs::remove_file(generation(path, n)) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => return Err(err),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_rotate_and_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join("a.log");
        let mut file = RotatingFile::new(path.clone(), 25);
        for n in 0..10 {
            file.append_line(&format!("line {n:04}")).unwrap();
        }
        let current = fs::read_to_string(&path).unwrap();
        assert_eq!(current, "line 0008\nline 0009\n");
        assert_eq!(
            fs::read_to_string(generation(&path, 1)).unwrap(),
            "line 0006\nline 0007\n"
        );
        assert!(generation(&path, 2).exists());
        assert!(!generation(&path, 3).exists(), "only two older files");
        // Reopening continues the size count.
        let mut again = RotatingFile::new(path.clone(), 25);
        again.append_line("line 0010").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "line 0010\n");
        again.remove().unwrap();
        assert!(!path.exists() && !generation(&path, 1).exists());
        remove_generations(&path).unwrap();
    }
}
