use crate::model::{Settings, project_state};
use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Debug, PartialEq, Eq)]
struct SessionFile {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

pub struct Sessions {
    root: PathBuf,
    directory: PathBuf,
    files: Vec<SessionFile>,
}

fn reject_link(path: &Path, metadata: &fs::Metadata) -> Result<()> {
    #[cfg(windows)]
    let is_link = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let is_link = metadata.file_type().is_symlink();
    if is_link {
        bail!("В каталоге сессий обнаружена ссылка: {}", path.display());
    }
    Ok(())
}

impl Sessions {
    pub fn inspect(settings: &Settings, project: &Path) -> Result<Self> {
        let root = settings.env_root();
        if !root.is_absolute() || !project.is_absolute() {
            bail!("Каталог окружения и путь проекта должны быть абсолютными.");
        }
        let mut sessions = Self {
            directory: project_state(settings, project).join("agent/sessions"),
            root,
            files: vec![],
        };
        sessions.files = sessions.scan()?;
        Ok(sessions)
    }

    pub fn count(&self) -> usize {
        self.files.len()
    }

    fn scan(&self) -> Result<Vec<SessionFile>> {
        // Validate every component below the configured environment before traversal.
        let relative = self.directory.strip_prefix(&self.root)?;
        let mut current = self.root.clone();
        for part in std::iter::once(None).chain(relative.components().map(Some)) {
            if let Some(part) = part {
                current.push(part);
            }
            let metadata = match fs::symlink_metadata(&current) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
                Err(error) => return Err(error).context("Не удалось прочитать каталог сессий"),
            };
            reject_link(&current, &metadata)?;
            if !metadata.is_dir() {
                bail!("Ожидалась папка: {}", current.display());
            }
        }
        let root = fs::canonicalize(&self.root)?;
        let directory = fs::canonicalize(&self.directory)?;
        if !directory.starts_with(&root) || directory == root {
            bail!("Каталог сессий находится вне окружения.");
        }
        let mut pending = vec![self.directory.clone()];
        let mut files = vec![];
        let mut visited = 0;
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                visited += 1;
                if visited > 250_000 {
                    bail!("Слишком много файлов в каталоге сессий.");
                }
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path)?;
                reject_link(&path, &metadata)?;
                if metadata.is_dir() {
                    pending.push(path);
                } else if metadata.is_file() && path.extension().is_some_and(|ext| ext == "jsonl") {
                    files.push(SessionFile {
                        path,
                        size: metadata.len(),
                        modified: metadata.modified()?,
                    });
                }
            }
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }

    pub fn delete(self) -> Result<usize> {
        if self.scan()? != self.files {
            bail!("Сессии изменились. Закройте pi и повторите удаление.");
        }
        let count = self.files.len();
        for (deleted, file) in self.files.iter().enumerate() {
            fs::remove_file(&file.path).with_context(|| {
                format!(
                    "Удалено {deleted} из {count} сессий. Не удалось удалить {}",
                    file.path.display()
                )
            })?;
        }
        Ok(count)
    }

    pub fn open(&self) -> Result<()> {
        self.scan()?;
        if !self.directory.is_dir() {
            bail!("У проекта пока нет папки сессий.");
        }
        #[cfg(windows)]
        {
            let windows = std::env::var_os("SystemRoot").context("SystemRoot не определён")?;
            std::process::Command::new(Path::new(&windows).join("explorer.exe"))
                .arg(&self.directory)
                .spawn()
                .context("Не удалось открыть папку сессий")?;
            Ok(())
        }
        #[cfg(not(windows))]
        bail!("Открытие Проводника поддерживается только в Windows.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Engine;

    #[test]
    fn deletion_is_scoped_to_project_engine_and_jsonl_files() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        let mut settings = Settings {
            root: temp.path().join("environment").display().to_string(),
            ..Default::default()
        };
        for engine in [Engine::Podman, Engine::Docker] {
            settings.engine = engine;
            let agent = project_state(&settings, &project).join("agent");
            fs::create_dir_all(agent.join("sessions/--workspace--")).unwrap();
            fs::write(agent.join("sessions/--workspace--/one.jsonl"), "session").unwrap();
            fs::write(agent.join("sessions/keep.txt"), "keep").unwrap();
            fs::write(agent.join("settings.json"), "{}").unwrap();
        }
        let sessions = Sessions::inspect(&settings, &project).unwrap();
        assert_eq!(sessions.count(), 1);
        assert_eq!(sessions.delete().unwrap(), 1);
        let agent = project_state(&settings, &project).join("agent");
        assert!(agent.join("settings.json").exists());
        assert!(agent.join("sessions/keep.txt").exists());
        settings.engine = Engine::Podman;
        assert_eq!(Sessions::inspect(&settings, &project).unwrap().count(), 1);
        assert_eq!(
            Sessions::inspect(&settings, &temp.path().join("other"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn changed_sessions_are_not_deleted_after_confirmation() {
        let temp = tempfile::tempdir().unwrap();
        let settings = Settings {
            root: temp.path().display().to_string(),
            ..Default::default()
        };
        let project = temp.path().join("project");
        let directory = project_state(&settings, &project).join("agent/sessions");
        fs::create_dir_all(&directory).unwrap();
        let file = directory.join("one.jsonl");
        fs::write(&file, "before").unwrap();
        let sessions = Sessions::inspect(&settings, &project).unwrap();
        fs::write(&file, "after confirmation").unwrap();
        assert!(sessions.delete().is_err());
        assert!(file.exists());
    }
}
