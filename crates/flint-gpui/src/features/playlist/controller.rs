use std::path::{Path, PathBuf};

const MEDIA_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "webm", "flv", "ts", "m2ts", "mpg", "mpeg", "wmv", "ogv", "m4v",
    "rmvb", "mp3", "flac", "wav", "aac", "ogg", "m4a", "opus",
];

pub struct PlaylistController {
    items: Vec<PathBuf>,
    current: Option<usize>,
}

impl PlaylistController {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            current: None,
        }
    }

    pub fn items(&self) -> &[PathBuf] {
        &self.items
    }

    pub fn current(&self) -> Option<usize> {
        self.current
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn has_prev(&self, shuffle: bool) -> bool {
        self.step_target(-1, false, shuffle).is_some()
    }

    pub fn has_next(&self) -> bool {
        self.step_target(1, false, false).is_some()
    }

    pub fn set_items(&mut self, items: Vec<PathBuf>) {
        self.items = items;
    }

    pub fn focus(&mut self, path: &Path) {
        let index = match self.items.iter().position(|item| item == path) {
            Some(index) => index,
            None => {
                self.items.push(path.to_path_buf());
                self.items.len() - 1
            }
        };
        self.current = Some(index);
    }

    pub fn add_files(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> usize {
        let mut added = 0;
        for path in paths {
            if self.items.contains(&path) {
                continue;
            }
            self.items.push(path);
            added += 1;
        }
        added
    }

    pub fn add_folder(&mut self, dir: &Path) -> usize {
        let Ok(read) = std::fs::read_dir(dir) else {
            return 0;
        };
        let mut files: Vec<PathBuf> = read
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && is_media_file(path))
            .collect();
        files.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
        self.add_files(files)
    }

    pub fn sort_by_name(&mut self, reverse: bool) {
        let current = self.current.and_then(|index| self.items.get(index).cloned());
        self.items.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
        if reverse {
            self.items.reverse();
        }
        self.current = current.and_then(|path| self.items.iter().position(|item| item == &path));
    }

    pub fn select(&mut self, index: usize) -> Option<PathBuf> {
        let path = self.items.get(index)?.clone();
        self.current = Some(index);
        Some(path)
    }

    pub fn step(&mut self, delta: isize, wrap: bool, shuffle: bool) -> Option<PathBuf> {
        let index = self.step_target(delta, wrap, shuffle)?;
        self.current = Some(index);
        Some(self.items[index].clone())
    }

    pub fn clear(&mut self) -> usize {
        let Some(current) = self.current.filter(|index| *index < self.items.len()) else {
            let count = self.items.len();
            self.items.clear();
            return count;
        };
        let keep = self.items[current].clone();
        let removed = self.items.len().saturating_sub(1);
        self.items = vec![keep];
        self.current = Some(0);
        removed
    }

    fn step_target(&self, delta: isize, wrap: bool, shuffle: bool) -> Option<usize> {
        let len = self.items.len();
        if len == 0 {
            return None;
        }
        if shuffle && len > 1 {
            let current = self.current.unwrap_or(0);
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.subsec_nanos() as usize)
                .unwrap_or(0);
            return Some((current + 1 + seed % (len - 1)) % len);
        }
        let current = self.current?;
        let next = current as isize + delta;
        if next < 0 || next >= len as isize {
            return wrap.then(|| next.rem_euclid(len as isize) as usize);
        }
        Some(next as usize)
    }
}

fn sort_key(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn is_media_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| MEDIA_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}
