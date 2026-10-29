use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub const IMAGE: &str = "localhost/pi-launcher-agent:1.3.0";
pub const CADDY: &str = "localhost/pi-box-composer-proxy:2-alpine-nocap";
pub const GATEWAY: &str = "http://llama-gateway:8080";
pub const CONTAINERFILE: &str = include_str!("../assets/Containerfile");
pub const PROXY_CONTAINERFILE: &str = include_str!("../assets/ProxyContainerfile");

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum Engine {
    #[default]
    Podman,
    Docker,
}

impl Engine {
    pub fn cli(self) -> &'static str {
        match self {
            Self::Podman => "podman",
            Self::Docker => "docker",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Podman => "Podman",
            Self::Docker => "Docker Desktop",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum Access {
    #[default]
    ReadOnly,
    ReadWrite,
}

impl Access {
    pub fn mount_mode(self) -> &'static str {
        match self {
            Self::ReadOnly => "ro",
            Self::ReadWrite => "rw",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum Session {
    #[default]
    New,
    Continue,
    Resume,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum CommandAccess {
    Disabled,
    #[default]
    Ask,
    Allow,
}

impl CommandAccess {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Запретить",
            Self::Ask => "Спрашивать",
            Self::Allow => "Разрешить",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProjectPermissions {
    pub write: Option<CommandAccess>,
    pub commands: Option<CommandAccess>,
    pub rules: Vec<PermissionRule>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum RuleTarget {
    #[default]
    Read,
    Write,
    Command,
}

impl RuleTarget {
    pub fn label(self) -> &'static str {
        match self {
            Self::Read => "Чтение",
            Self::Write => "Запись",
            Self::Command => "Команда",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct PermissionRule {
    pub target: RuleTarget,
    pub pattern: String,
    pub action: CommandAccess,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Plugin {
    pub path: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct PiOptions {
    pub rules: Vec<PermissionRule>,
    pub default_access: Access,
    pub default_session: Session,
    pub commands: CommandAccess,
    pub confirm_edits: bool,
    pub light_theme: bool,
    pub auto_compact: bool,
    pub skills: bool,
    pub plugins: Vec<Plugin>,
}

impl Default for PiOptions {
    fn default() -> Self {
        Self {
            rules: vec![],
            default_access: Access::ReadOnly,
            default_session: Session::New,
            commands: CommandAccess::Ask,
            confirm_edits: true,
            light_theme: false,
            auto_compact: true,
            skills: false,
            plugins: vec![],
        }
    }
}

impl PiOptions {
    pub fn write_access(&self) -> CommandAccess {
        if self.default_access == Access::ReadOnly {
            CommandAccess::Disabled
        } else if self.confirm_edits {
            CommandAccess::Ask
        } else {
            CommandAccess::Allow
        }
    }

    pub fn set_write_access(&mut self, value: CommandAccess) {
        self.default_access = if value == CommandAccess::Disabled {
            Access::ReadOnly
        } else {
            Access::ReadWrite
        };
        self.confirm_edits = value != CommandAccess::Allow;
    }
}

impl Session {
    pub fn label(self) -> &'static str {
        match self {
            Self::New => "Новая",
            Self::Continue => "Продолжить",
            Self::Resume => "Выбрать сессию",
        }
    }
    pub fn flag(self) -> Option<&'static str> {
        match self {
            Self::New => None,
            Self::Continue => Some("--continue"),
            Self::Resume => Some("--resume"),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub engine: Engine,
    pub root: String,
    pub host: String,
    pub port: u16,
    pub model: String,
    pub context_window: u32,
    pub max_tokens: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            engine: Engine::Podman,
            root: default_data_dir()
                .join("environment")
                .to_string_lossy()
                .into_owned(),
            host: "192.168.1.195".into(),
            port: 8080,
            model: String::new(),
            context_window: 32768,
            max_tokens: 4096,
        }
    }
}

impl Settings {
    /// Keep the original Podman layout, and isolate Docker files/state beneath it.
    pub fn env_root(&self) -> PathBuf {
        let root = PathBuf::from(&self.root);
        match self.engine {
            Engine::Podman => root,
            Engine::Docker => root.join("docker"),
        }
    }
    pub fn validate(&self) -> Result<()> {
        // This value is also interpolated in a Caddyfile, so accept only a plain host/IP.
        if self.host.is_empty()
            || self.host.len() > 253
            || !self
                .host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
            || self.host.starts_with('-')
            || self.port == 0
        {
            bail!("Укажите IPv4-адрес или DNS-имя без http://, пути и пробелов; порт 1–65535.");
        }
        if self.context_window == 0 || self.max_tokens == 0 || self.max_tokens > self.context_window
        {
            bail!("Лимит ответа должен быть больше нуля и не превышать размер контекста.");
        }
        let root = Path::new(&self.root);
        if !root.is_absolute()
            || root.parent().is_none()
            || self.root.trim().is_empty()
            || root.components().any(|part| {
                matches!(
                    part,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            bail!("Каталог окружения должен быть абсолютным путём к отдельной папке.");
        }
        if self.root.contains([',', '\n', '\r', '\0']) {
            bail!("Путь окружения не должен содержать запятые или управляющие символы.");
        }
        Ok(())
    }

    pub fn names(&self) -> Names {
        let key = normalized_path(&self.env_root());
        let prefix = format!("pi-ui-{}", &hash(&key)[..12]);
        Names {
            internal: format!("{prefix}-internal"),
            uplink: format!("{prefix}-uplink"),
            proxy: format!("{prefix}-proxy"),
            secret: format!("{prefix}-key"),
            owner: prefix,
        }
    }

    pub fn caddyfile(&self) -> String {
        format!(
            r#":8080 {{
    @model path /v1/models /v1/chat/completions
    handle @model {{
        reverse_proxy http://{}:{} {{
            header_up Authorization "Bearer {{env.LLAMA_API_KEY}}"
        }}
    }}
    handle {{
        respond "Forbidden" 403
    }}
}}
"#,
            self.host, self.port
        )
    }
}

#[derive(Clone, Debug)]
pub struct Names {
    pub internal: String,
    pub uplink: String,
    pub proxy: String,
    pub secret: String,
    pub owner: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Recent {
    #[serde(default)]
    pub permissions: Option<ProjectPermissions>,
    pub path: String,
    pub access: Access,
    #[serde(default)]
    pub session: Session,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Saved {
    // Missing in older configs: enable the Explorer menu on the next normal launch.
    pub explorer_menu: Option<bool>,
    pub permissions: Option<ProjectPermissions>,
    pub settings: Settings,
    pub recent: Vec<Recent>,
    pub project: String,
    pub access: Access,
    pub session: Session,
    pub pi: PiOptions,
}

impl Saved {
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).context("Не удалось прочитать настройки launcher")
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).context("Не удалось открыть настройки launcher"),
        }
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        write_json(path, self)
    }
    pub fn remember(&mut self, project: &Path) {
        let path = project.to_string_lossy().into_owned();
        let pinned = self
            .recent
            .iter()
            .any(|p| p.pinned && normalized_path(Path::new(&p.path)) == normalized_path(project));
        self.recent
            .retain(|p| normalized_path(Path::new(&p.path)) != normalized_path(project));
        self.recent.insert(
            0,
            Recent {
                permissions: self.permissions.clone(),
                path: path.clone(),
                access: self.access,
                session: self.session,
                pinned,
            },
        );
        self.recent.sort_by_key(|p| !p.pinned);
        let mut unpinned = 0;
        self.recent.retain(|project| {
            if project.pinned {
                true
            } else {
                unpinned += 1;
                unpinned <= 10
            }
        });
        self.project = path;
    }

    pub fn select_project(&mut self, path: &str) {
        let path = path.trim().trim_matches('"').to_owned();
        if normalized_path(Path::new(&self.project)) == normalized_path(Path::new(&path)) {
            self.project = path;
            return;
        }
        let recent = self
            .recent
            .iter()
            .find(|p| normalized_path(Path::new(&p.path)) == normalized_path(Path::new(&path)));
        self.permissions = recent.map_or(Some(ProjectPermissions::default()), |p| {
            p.permissions.clone()
        });
        self.access = recent.map_or(self.pi.default_access, |p| p.access);
        self.session = recent.map_or(self.pi.default_session, |p| p.session);
        self.project = path;
    }

    pub fn effective_permissions(&self) -> (Access, PiOptions) {
        let mut options = self.pi.clone();
        let mut access = self.access;
        if let Some(permissions) = &self.permissions {
            let write = permissions.write.unwrap_or_else(|| self.pi.write_access());
            access = if write == CommandAccess::Disabled {
                Access::ReadOnly
            } else {
                Access::ReadWrite
            };
            options.confirm_edits = write == CommandAccess::Ask;
            options.commands = permissions.commands.unwrap_or(options.commands);
            options.rules.extend(permissions.rules.clone());
        }
        (access, options)
    }

    pub fn editable_permissions(&mut self) -> &mut ProjectPermissions {
        self.permissions.get_or_insert_with(|| ProjectPermissions {
            write: Some(if self.access == Access::ReadOnly {
                CommandAccess::Disabled
            } else if self.pi.confirm_edits {
                CommandAccess::Ask
            } else {
                CommandAccess::Allow
            }),
            commands: None,
            rules: vec![],
        })
    }

    pub fn remember_project_preferences(&mut self) {
        if let Some(recent) = self.recent.iter_mut().find(|p| {
            normalized_path(Path::new(&p.path)) == normalized_path(Path::new(&self.project))
        }) {
            recent.access = self.access;
            recent.permissions = self.permissions.clone();
            recent.session = self.session;
        }
    }
}

pub fn default_data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("PiLauncher")
}

pub fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub fn normalized_path(path: &Path) -> String {
    let value = path.to_string_lossy().replace('/', "\\");
    value
        .trim_start_matches("\\\\?\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn contains_path(parent: &Path, child: &Path) -> bool {
    let parent = normalized_path(parent);
    let child = normalized_path(child);
    child == parent || child.starts_with(&(parent + "\\"))
}

/// Resolve existing ancestors as well, so an environment placed under a junction
/// cannot be exposed by mounting the junction's target/parent.
pub fn canonical_target(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return fs::canonicalize(path).context("Не удалось разрешить путь");
    }
    let parent = path.parent().context("У пути нет родительской папки")?;
    let name = path.file_name().context("Некорректный путь")?;
    Ok(canonical_target(parent)?.join(name))
}

pub fn validate_project(project: &str, settings: &Settings, data_dir: &Path) -> Result<PathBuf> {
    settings.validate()?;
    let original = Path::new(project);
    if !original.is_absolute() {
        bail!("Выберите абсолютный путь к папке проекта.");
    }
    let canonical =
        fs::canonicalize(original).context("Папка проекта не найдена или недоступна")?;
    if !canonical.is_dir() {
        bail!("Проект должен быть папкой.");
    }
    let text = canonical.to_string_lossy();
    let plain = text.strip_prefix("\\\\?\\").unwrap_or(&text);
    if plain.starts_with("UNC\\")
        || plain.starts_with("\\\\")
        || canonical.parent().is_none()
        || plain.trim_end_matches('\\').ends_with(':')
    {
        bail!("Выберите папку проекта, а не корень диска или сетевой ресурс.");
    }
    if plain.contains([',', '\n', '\r', '\0'])
        || plain.contains(':') && !plain.get(1..2).is_some_and(|s| s == ":")
    {
        bail!("Путь содержит символы, несовместимые с монтированием контейнера.");
    }
    for protected in [Path::new(&settings.root), data_dir] {
        let protected = canonical_target(protected)?;
        if contains_path(&canonical, &protected) || contains_path(&protected, &canonical) {
            bail!(
                "Проект пересекается с каталогом окружения или данными launcher. Выберите отдельную папку."
            );
        }
    }
    // Reject nested links/junctions instead of guessing what the VM will expose.
    // The top-level project path itself is resolved above, and that target is mounted.
    let mut queue = vec![canonical.clone()];
    let mut visited = 0;
    while let Some(dir) = queue.pop() {
        for entry in
            fs::read_dir(&dir).with_context(|| format!("Не удалось проверить {}", dir.display()))?
        {
            let entry = entry?;
            visited += 1;
            if visited > 250_000 {
                bail!("В проекте больше 250 000 записей. Выберите более узкую папку.");
            }
            let metadata = fs::symlink_metadata(entry.path())?;
            #[cfg(windows)]
            let is_link = {
                use std::os::windows::fs::MetadataExt;
                metadata.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let is_link = metadata.file_type().is_symlink();
            if is_link {
                bail!(
                    "В проекте есть ссылка/junction: {}. Выберите папку без ссылок.",
                    entry.path().display()
                );
            }
            if metadata.is_dir() {
                queue.push(entry.path());
            }
        }
    }
    Ok(PathBuf::from(plain))
}

pub fn project_state(settings: &Settings, project: &Path) -> PathBuf {
    settings
        .env_root()
        .join("state/projects")
        .join(hash(&normalized_path(project)))
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("У файла нет каталога")?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path)
        .map_err(|e| e.error)
        .with_context(|| format!("Не удалось сохранить {}", path.display()))?;
    Ok(())
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    atomic_write(path, &serde_json::to_vec_pretty(value)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_projects_migrate_and_new_folders_do_not_inherit_write_access() {
        let mut saved: Saved = serde_json::from_str(r#"{"project":"C:\\code\\one","access":"ReadWrite","recent":[{"path":"C:\\code\\one","access":"ReadWrite"}]}"#).unwrap();
        assert_eq!(saved.pi, PiOptions::default());
        assert!(!saved.recent[0].pinned);
        assert_eq!(saved.recent[0].session, Session::New);
        saved.select_project(r#" "C:\code\two" "#);
        assert_eq!(saved.project, r"C:\code\two");
        assert_eq!(saved.access, Access::ReadOnly);
        saved.select_project(r"c:\CODE\one");
        assert_eq!(saved.access, Access::ReadWrite);
        assert_eq!(saved.effective_permissions().0, Access::ReadWrite);
        assert_eq!(saved.editable_permissions().write, Some(CommandAccess::Ask));
    }

    #[test]
    fn project_preferences_and_pins_survive_switches_and_serialization() {
        let mut saved = Saved::default();
        saved.select_project(r"C:\code\one");
        saved.access = Access::ReadWrite;
        saved.editable_permissions().write = Some(CommandAccess::Allow);
        saved.editable_permissions().commands = Some(CommandAccess::Disabled);
        saved.session = Session::Continue;
        saved.remember(Path::new(r"C:\code\one"));
        saved.recent[0].pinned = true;
        saved.select_project(r"C:\code\two");
        saved.pi.set_write_access(CommandAccess::Ask);
        assert_eq!(saved.effective_permissions().0, Access::ReadWrite);
        assert!(saved.effective_permissions().1.confirm_edits);
        saved.remember(Path::new(r"C:\code\two"));
        saved.select_project(r"C:\code\one");
        assert_eq!(saved.access, Access::ReadWrite);
        assert_eq!(saved.session, Session::Continue);
        saved.session = Session::Resume;
        saved.remember_project_preferences();
        saved.remember(Path::new(r"C:\code\one"));
        let restored: Saved = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert!(restored.recent[0].pinned);
        assert_eq!(restored.recent[0].session, Session::Resume);
        assert_eq!(restored.recent.len(), 2);
        let (access, options) = restored.effective_permissions();
        assert_eq!(access, Access::ReadWrite);
        assert!(!options.confirm_edits);
        assert_eq!(options.commands, CommandAccess::Disabled);
    }

    #[test]
    fn legacy_settings_default_to_podman_and_keep_existing_paths() {
        let settings: Settings =
            serde_json::from_str(r#"{"root":"C:\\Work\\ai","model":"local"}"#).unwrap();
        assert_eq!(settings.engine, Engine::Podman);
        assert_eq!(settings.env_root(), PathBuf::from(r"C:\Work\ai"));
        let docker = Settings {
            engine: Engine::Docker,
            ..settings.clone()
        };
        assert_eq!(docker.env_root(), Path::new(r"C:\Work\ai").join("docker"));
        assert_ne!(settings.names().proxy, docker.names().proxy);
    }

    #[test]
    fn rejects_caddy_injection_and_invalid_limits() {
        let mut s = Settings {
            host: "localhost {\n respond hacked".into(),
            ..Settings::default()
        };
        assert!(s.validate().is_err());
        s.host = "127.0.0.1".into();
        s.max_tokens = s.context_window + 1;
        assert!(s.validate().is_err());
    }

    #[test]
    fn path_comparison_respects_directory_boundaries() {
        assert!(contains_path(
            Path::new(r"C:\work\ai"),
            Path::new(r"c:\WORK\ai\secrets")
        ));
        assert!(!contains_path(
            Path::new(r"C:\work\ai"),
            Path::new(r"C:\work\ai-other")
        ));
        assert_eq!(
            normalized_path(Path::new(r"\\?\C:\Work\Ai\")),
            r"c:\work\ai"
        );
    }

    #[test]
    fn saves_recent_without_token_and_keeps_latest_access() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("launcher.json");
        let mut saved = Saved::default();
        saved.remember(Path::new(r"D:\My Project"));
        saved.access = Access::ReadWrite;
        saved.remember(Path::new(r"d:\my project"));
        assert_eq!(saved.recent.len(), 1);
        saved.save(&path).unwrap();
        saved.save(&path).unwrap(); // replacement must work on Windows too
        let loaded = Saved::load(&path).unwrap();
        assert_eq!(loaded.recent[0].access, Access::ReadWrite);
        let json: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert!(json.get("token").is_none());
        assert!(json["settings"].get("api_token").is_none());
    }

    #[test]
    fn validates_projects_and_rejects_infrastructure_and_ancestors() {
        let temp = tempfile::tempdir().unwrap();
        let env = temp.path().join("env");
        let data = temp.path().join("launcher");
        let project = temp.path().join("project");
        fs::create_dir_all(&env).unwrap();
        fs::create_dir_all(&project).unwrap();
        let s = Settings {
            root: env.to_string_lossy().into_owned(),
            ..Settings::default()
        };
        assert!(validate_project(&project.to_string_lossy(), &s, &data).is_ok());
        assert!(validate_project(&env.to_string_lossy(), &s, &data).is_err());
        assert!(validate_project(&temp.path().to_string_lossy(), &s, &data).is_err());
        assert!(validate_project("relative", &s, &data).is_err());
        fs::write(project.join("file"), "x").unwrap();
        assert!(validate_project(&project.join("file").to_string_lossy(), &s, &data).is_err());
    }
}
