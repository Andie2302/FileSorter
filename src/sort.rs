// src/main.rs
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use log::{info, warn, error};
use clap::Parser;

// ── CLI ──────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(about = "Dateien sortieren und Projektordner schützen")]
pub struct Cli {
    /// Quellordner zum Durchsuchen
    pub(crate) source: PathBuf,

    /// Zielordner (Basis) – darf NICHT im Quellordner liegen
    pub(crate) dest: PathBuf,

    /// Nur anzeigen, nichts wirklich verschieben
    #[arg(long)]
    pub(crate) dry_run: bool,
}

// ── Dateikategorien ──────────────────────────────────────────────────────────
pub enum FileCategory {
    Image,
    Document,
    Video,
    Audio,
    Archive,
    Unknown,
}
pub fn categorize(path: &Path) -> FileCategory {
    if let Ok(Some(kind)) = infer::get_from_path(path) {
        return match kind.mime_type() {
            m if m.starts_with("image/") => FileCategory::Image,
            m if m.starts_with("video/") => FileCategory::Video,
            m if m.starts_with("audio/") => FileCategory::Audio,
            "application/pdf" => FileCategory::Document,
            m if m.contains("zip") || m.contains("tar") || m.contains("gzip")
            => FileCategory::Archive,
            _ => FileCategory::Unknown,
        };
    }
    match path.extension().and_then(|e| e.to_str()) {
        Some("jpg" | "jpeg" | "png" | "gif" | "webp" | "heic") => FileCategory::Image,
        Some("pdf" | "doc" | "docx" | "odt") => FileCategory::Document,
        Some("mp4" | "mkv" | "avi" | "mov") => FileCategory::Video,
        Some("mp3" | "flac" | "wav" | "ogg") => FileCategory::Audio,
        Some("zip" | "tar" | "gz" | "rar" | "7z") => FileCategory::Archive,
        _ => FileCategory::Unknown,
    }
}

// ── Projektschutz ────────────────────────────────────────────────────────────
pub fn is_project_dir(dir: &Path) -> bool {
    let file_markers = [
        "Cargo.toml", "go.mod", "package.json",
        "pyproject.toml", "setup.py", "pom.xml", "build.gradle",
    ];
    let dir_markers = [".git", ".idea", ".gradle", "node_modules"];

    for m in &file_markers {
        if dir.join(m).is_file() { return true; }
    }
    for m in &dir_markers {
        if dir.join(m).is_dir() { return true; }
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".sln") || name.ends_with(".csproj")
                || name.ends_with(".fsproj") || name.ends_with(".iml") {
                return true;
            }
        }
    }
    false
}

// ── Fix #2: Ancestor-Prüfung ─────────────────────────────────────────────────
pub fn is_ancestor_of(potential_ancestor: &Path, path: &Path) -> bool {
    // Canonicalize um Symlinks / .. aufzulösen
    let Ok(ancestor) = potential_ancestor.canonicalize() else { return false };
    let Ok(child) = path.canonicalize()               else { return false };
    child.starts_with(&ancestor)
}

// ── Sortierer ────────────────────────────────────────────────────────────────

pub struct Sorter {
    source: PathBuf,
    // Alle Zielordner als Vec – einfach erweiterbar
    dest_dirs: Vec<PathBuf>,
    // Ziel je Kategorie
    dest_images: PathBuf,
    dest_documents: PathBuf,
    dest_videos: PathBuf,
    dest_audio: PathBuf,
    dest_archives: PathBuf,
    dest_projects: PathBuf,
    dest_unknown: PathBuf,
    visited: HashSet<u64>,
    dry_run: bool,
}
impl Sorter {
    pub(crate) fn new(source: PathBuf, dest: PathBuf, dry_run: bool) -> Self {
        let mk = |name: &str| dest.join(name);
        let dest_images = mk("bilder");
        let dest_documents = mk("dokumente");
        let dest_videos = mk("videos");
        let dest_audio = mk("audio");
        let dest_archives = mk("archive");
        let dest_projects = mk("projekte");
        let dest_unknown = mk("sonstiges");

        // Alle Zielordner sammeln für den Ancestor-Check
        let dest_dirs = vec![
            dest_images.clone(), dest_documents.clone(), dest_videos.clone(),
            dest_audio.clone(), dest_archives.clone(), dest_projects.clone(),
            dest_unknown.clone(),
        ];

        Self {
            source,
            dest_dirs,
            dest_images,
            dest_documents,
            dest_videos,
            dest_audio,
            dest_archives,
            dest_projects,
            dest_unknown,
            visited: HashSet::new(),
            dry_run,
        }
    }

    pub fn run(&mut self) {
        let source = self.source.clone();
        self.scan(&source);
    }
pub fn scan(&mut self, dir: &Path) {
        // Fix #2: Zielordner niemals selbst scannen
        for dest in &self.dest_dirs {
            if is_ancestor_of(dest, dir) {
                info!("Zielordner übersprungen: {:?}", dir);
                return;
            }
        }

        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                error!("Kann {:?} nicht lesen: {}", dir, e);
                return;
            }
        };

        for entry in entries.flatten() {
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(e) => {
                    error!("Metadaten fehlen für {:?}: {}", entry.path(), e);
                    continue;
                }
            };

            // Symlink überspringen
            if meta.file_type().is_symlink() {
                warn!("Symlink übersprungen: {:?}", entry.path());
                continue;
            }

            // Inode-Check
            #[cfg(unix)] {
                use std::os::unix::fs::MetadataExt;
                if !self.visited.insert(meta.ino()) {
                    warn!("Bereits besucht (Hardlink?): {:?}", entry.path());
                    continue;
                }
            }

            let path = entry.path();

            if meta.is_dir() {
                if is_project_dir(&path) {
                    let relative = path.strip_prefix(&self.source).unwrap();
                    let dest = self.dest_projects.join(relative);
                    // Fix #3: Ordner verschieben, nicht Datei entfernen
                    self.move_dir(&path, &dest);
                } else {
                    self.scan(&path);
                }
            } else {
                let relative = path.strip_prefix(&self.source).unwrap();
                let dest_base = match categorize(&path) {
                    FileCategory::Image => &self.dest_images,
                    FileCategory::Document => &self.dest_documents,
                    FileCategory::Video => &self.dest_videos,
                    FileCategory::Audio => &self.dest_audio,
                    FileCategory::Archive => &self.dest_archives,
                    FileCategory::Unknown => &self.dest_unknown,
                };
                let dest = dest_base.join(relative);
                self.move_file(&path, &dest);
            }
        }
    }
pub fn move_file(&self, src: &Path, dest: &Path) {
        if self.dry_run {
            println!("[DRY-RUN] Datei: {:?}  →  {:?}", src, dest);
            return;
        }
        self.ensure_parent(dest);
        if let Err(_) = fs::rename(src, dest) {
            // Cross-device: kopieren + löschen
            match fs::copy(src, dest) {
                Ok(_) => {
                    if let Err(e) = fs::remove_file(src) {
                        error!("Kopiert aber Original nicht löschbar {:?}: {}", src, e);
                    } else {
                        info!("Verschoben (cross-device): {:?} → {:?}", src, dest);
                    }
                }
                Err(e) => error!("Verschieben fehlgeschlagen {:?}: {}", src, e),
            }
        } else {
            info!("Verschoben: {:?} → {:?}", src, dest);
        }
    }

    // Fix #3: Eigene Funktion für Verzeichnisse
pub fn move_dir(&self, src: &Path, dest: &Path) {
        if self.dry_run {
            println!("[DRY-RUN] Projekt: {:?}  →  {:?}", src, dest);
            return;
        }
        self.ensure_parent(dest);

        if let Err(_) = fs::rename(src, dest) {
            // Erst in temporären Ordner kopieren (gleiche Partition wie Ziel)
            // Dann atomar umbenennen – so liegt nie ein halbfertiger Ordner am Ziel
            let staging = dest.with_extension("__tmp");

            match copy_dir_recursive(src, &staging) {
                Ok(_) => {
                    // rename von staging → dest ist atomar (gleiche Partition)
                    if let Err(e) = fs::rename(&staging, dest) {
                        error!("Staging-rename fehlgeschlagen {:?}: {}", dest, e);
                        fs::remove_dir_all(&staging).ok(); // staging aufräumen
                        return;
                    }
                    // Erst jetzt Original entfernen – Ziel ist garantiert vollständig
                    if let Err(e) = fs::remove_dir_all(src) {
                        error!("Quelle nicht löschbar {:?}: {}", src, e);
                    }
                }
                Err(e) => {
                    error!("Staging-Kopie fehlgeschlagen {:?}: {}", src, e);
                    fs::remove_dir_all(&staging).ok(); // halbfertiges staging aufräumen
                }
            }
        } else {
            info!("Projekt verschoben: {:?} → {:?}", src, dest);
        }
    }
pub fn ensure_parent(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                error!("Ordner anlegen fehlgeschlagen {:?}: {}", parent, e);
            }
        }
    }
}

// Rekursive Kopie für cross-device Projektordner
pub fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dest)?;

    for entry in fs::read_dir(src)?.flatten() {
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let meta = entry.metadata()?;

        if meta.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            fs::copy(&src_path, &dest_path)?;

            // Berechtigungen übertragen (Unix-only, für .git / node_modules wichtig)
            #[cfg(unix)]
            fs::set_permissions(&dest_path, meta.permissions())?;
        }
    }

    // Verzeichnis-Permissions ebenfalls übertragen
    #[cfg(unix)]
    fs::set_permissions(dest, fs::metadata(src)?.permissions())?;

    Ok(())
}

