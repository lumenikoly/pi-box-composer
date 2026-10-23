use crate::{
    model::*,
    process::{Cmd, Runner, open_terminal},
};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

pub const STEPS: [(&str, &str); 7] = [
    (
        "Проверка движка",
        "Доступность и режим контейнерного движка",
    ),
    ("Файлы окружения", "Containerfile и конфигурация Caddy"),
    ("Сборка образа", "Pi 1.3.0 и расширение разрешений"),
    ("Сети контейнеров", "Внутренняя сеть Pi и сеть прокси"),
    (
        "Запуск прокси",
        "Сборка Caddy без file capabilities, токен и запуск",
    ),
    (
        "Проверка модели",
        "Список моделей и проверка прямого соединения",
    ),
    (
        "Сохранение настроек",
        "Модель, лимиты и политика разрешений",
    ),
];

pub enum StepResult {
    Done,
    Models(Vec<String>),
}

fn cmd(args: &[&str]) -> Cmd {
    Cmd::new(args.iter().copied())
}
fn build(settings: &Settings) -> Cmd {
    build_image(settings, IMAGE, "image")
}

fn build_proxy(settings: &Settings) -> Cmd {
    build_image(settings, CADDY, "proxy/image")
}

fn build_image(settings: &Settings, image: &str, directory: &str) -> Cmd {
    let mut args = vec!["build".to_owned()];
    match settings.engine {
        Engine::Podman => args.push("--pull=missing".into()),
        Engine::Docker => {
            args.extend(["--pull=false", "--progress=plain", "--load"].map(String::from))
        }
    }
    args.extend([
        "-t".into(),
        image.into(),
        "-f".into(),
        settings
            .env_root()
            .join(directory)
            .join("Containerfile")
            .display()
            .to_string(),
        settings.env_root().join(directory).display().to_string(),
    ]);
    let mut command = Cmd::new(args);
    command.timeout = Duration::from_secs(30 * 60);
    command.allow_truncated_output = true;
    command
}

fn no_proxy(engine: Engine) -> Vec<String> {
    match engine {
        Engine::Podman => vec!["--http-proxy=false".into()],
        Engine::Docker => [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "FTP_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "http_proxy",
            "https_proxy",
            "ftp_proxy",
            "all_proxy",
            "no_proxy",
        ]
        .into_iter()
        .flat_map(|key| ["--env".into(), format!("{key}=")])
        .collect(),
    }
}

pub fn engine_info_command(engine: Engine) -> Cmd {
    cmd(&[
        "info",
        "--format",
        if engine == Engine::Docker {
            "{{json .}}"
        } else {
            "json"
        },
    ])
}

pub fn start_engine_command(engine: Engine) -> Cmd {
    match engine {
        Engine::Podman => cmd(&["machine", "start"]),
        Engine::Docker => cmd(&["desktop", "start", "--timeout", "60"]),
    }
}

fn network_create(name: &str, internal: bool, owner: &str) -> Cmd {
    let mut args = vec!["network", "create", "--label", owner];
    if internal {
        args.push("--internal");
    }
    args.push(name);
    cmd(&args)
}

fn proxy_run(settings: &Settings) -> Cmd {
    let n = settings.names();
    let mount = format!(
        "type=bind,src={},dst=/etc/caddy/Caddyfile,readonly",
        settings.env_root().join("proxy/Caddyfile").display()
    );
    let mut args: Vec<String> = [
        "run",
        "-d",
        "--name",
        &n.proxy,
        "--pull=never",
        "--label",
        &format!("pi-launcher.owner={}", n.owner),
        "--network",
        &n.uplink,
    ]
    .into_iter()
    .map(String::from)
    .collect();
    args.extend(no_proxy(settings.engine));
    args.extend(
        [
            "--user",
            "1000:1000",
            "--read-only",
            "--cap-drop=all",
            "--security-opt=no-new-privileges",
            "--tmpfs=/config:rw,uid=1000,gid=1000,mode=0700",
            "--tmpfs=/data:rw,uid=1000,gid=1000,mode=0700",
        ]
        .map(String::from),
    );
    match settings.engine {
        Engine::Podman => args.extend([
            "--network".into(),
            format!("{}:alias=llama-gateway", n.internal),
            "--secret".into(),
            format!("source={},type=env,target=LLAMA_API_KEY", n.secret),
        ]),
        // The value comes from the CLI process environment, never from argv.
        Engine::Docker => args.extend(["--env".into(), "LLAMA_API_KEY".into()]),
    }
    args.extend(["--mount".into(), mount, CADDY.into()]);
    Cmd::new(args)
}

fn connect_proxy(s: &Settings) -> Cmd {
    let n = s.names();
    cmd(&[
        "network",
        "connect",
        "--alias",
        "llama-gateway",
        &n.internal,
        &n.proxy,
    ])
}

fn probe(settings: &Settings, direct: bool) -> Cmd {
    let n = settings.names();
    let url = if direct {
        format!("http://{}:{}/v1/models", settings.host, settings.port)
    } else {
        format!("{GATEWAY}/v1/models")
    };
    let mut args = ["run", "--rm", "--pull=never", "--network", &n.internal]
        .map(String::from)
        .to_vec();
    args.extend(no_proxy(settings.engine));
    args.extend(
        [
            "--cap-drop=all",
            "--security-opt=no-new-privileges",
            "--read-only",
            "--entrypoint",
            "curl",
            IMAGE,
            "-sS",
            "--max-time",
            if direct { "4" } else { "15" },
        ]
        .map(String::from),
    );
    if !direct {
        args.push("--fail".into());
    }
    args.push(url);
    let mut command = Cmd::new(args);
    command.output_limit = MAX_MODEL_RESPONSE_BYTES;
    command
}

pub fn step_instructions(step: usize, s: &Settings) -> Vec<crate::instructions::Instruction> {
    use crate::instructions::Instruction as I;
    let n = s.names();
    let command = |title: &str, description: &str, command: Cmd| {
        I::command(title, description, command.formatted(s.engine))
    };
    match step {
        0 => vec![command("Проверка подключения", "Показывает сведения о выбранном движке. Launcher дополнительно проверяет rootless для Podman или Linux containers для Docker Desktop.", engine_info_command(s.engine))],
        1 => vec![
            I::file("Образ Pi", "Launcher создаёт этот файл автоматически. Он описывает Node.js, Pi и расширение разрешений. Кнопка копирует содержимое файла, а не команду PowerShell.", s.env_root().join("image/Containerfile").display().to_string(), CONTAINERFILE.into()),
            I::file("Настройки прокси", "Разрешает только запросы списка моделей и чата к выбранному llama.cpp. Токен читается из окружения прокси, его значения в файле нет.", s.env_root().join("proxy/Caddyfile").display().to_string(), s.caddyfile()),
            I::file("Образ прокси", "Отдельный образ Caddy: удаляет file capability, несовместимую с --cap-drop=all. Образ Pi/Node не меняется.", s.env_root().join("proxy/image/Containerfile").display().to_string(), PROXY_CONTAINERFILE.into()),
        ],
        2 => vec![command("Сборка образа Pi", "Выполняйте после создания Containerfile. Первый запуск скачивает базовый образ и npm-пакеты. Дождитесь успешного завершения сборки.", build(s))],
        3 => vec![
            command("Внутренняя сеть агента", "Pi подключается только к этой сети. Флаг --internal ограничивает внешние соединения.", network_create(&n.internal, true, &format!("pi-launcher.owner={}", n.owner))).when("Только если сеть ещё не создана"),
            command("Внешняя сеть прокси", "Эта сеть позволяет Caddy обращаться к серверу модели. Сам агент Pi к ней не подключается.", network_create(&n.uplink, false, &format!("pi-launcher.owner={}", n.owner))).when("Только если сеть ещё не создана"),
        ],
        4 => {
            let mut items = vec![
                I::file("Образ прокси", "Launcher записывает этот файл перед сборкой, в том числе при повторе шага 5 в существующем окружении.", s.env_root().join("proxy/image/Containerfile").display().to_string(), PROXY_CONTAINERFILE.into()),
                command("Сборка образа Caddy", "Выполните до удаления старого прокси. Удаляет file capability бинарника; ограничения пользователя, файловой системы и capabilities контейнера сохраняются.", build_proxy(s)),
            ];
            if s.engine == Engine::Docker {
                items.push(I::command("Токен для ручного запуска", "Введите токен локальной модели в PowerShell. Следующая команда запуска прокси передаст эту переменную контейнеру. При запуске из UI launcher делает это сам.", "$env:LLAMA_API_KEY = Read-Host 'Токен llama.cpp'").when("Только при ручном выполнении команд"));
            } else {
                items.push(I::note("Токен Podman", "Введите токен в поле API-токен мастера и выполните этот шаг из launcher. Он создаст Podman secret через stdin. Пустое поле повторно использует уже созданный секрет."));
            }
            items.push(command("Удаление старого прокси", "Останавливает и удаляет только указанный контейнер прокси перед пересозданием. Не удаляет проект или историю Pi. Не выполняйте во время активного запроса к модели.", cmd(&["rm", "-f", &n.proxy])).when("Только при повторной настройке существующего прокси"));
            items.push(command("Запуск Caddy", "Выполняйте после настройки сетей и токена. Прокси работает без записи в корневую файловую систему и без дополнительных capabilities.", proxy_run(s)));
            if s.engine == Engine::Docker {
                items.push(command("Подключение к сети Pi", "Добавляет прокси во внутреннюю сеть с именем llama-gateway. Выполните после запуска Caddy, до проверки модели.", connect_proxy(s)));
            }
            items.push(command("Проверка конфигурации Caddy", "Успешное завершение означает, что конфигурация корректна. Доступность самой модели проверяется на следующем шаге.", cmd(&["exec", &n.proxy, "caddy", "validate", "--config", "/etc/caddy/Caddyfile"])));
            items
        }
        5 => vec![
            command("Модели через прокси", "Ожидается JSON со списком моделей в data. Если запрос не проходит, проверьте адрес сервера и токен.", probe(s, false)),
            command("Прямое соединение с сервером", "Здесь ожидается ошибка curl: код 7 (нет соединения) или 28 (таймаут). Успешный ответ означает, что прямой доступ открыт. Ошибка самого движка не подтверждает изоляцию.", probe(s, true)),
        ],
        6 => vec![
            I::note("Выбор модели", "Выберите модель в мастере и нажмите «Сохранить». Shell-команда для этого не нужна."),
            I::note("Файлы конфигурации", format!("Launcher сохранит models.json, settings.json, permissions.json и ready.json в {}. Токен в эти файлы не записывается.", s.env_root().join("config").display())),
        ],
        _ => vec![],
    }
}

pub fn execute_step(step: usize, s: &Settings, token: &str, runner: &Runner) -> Result<StepResult> {
    if runner.engine != s.engine {
        bail!("Выбран другой контейнерный движок.");
    }
    if step != 0 {
        s.validate()?;
    }
    match step {
        0 => check_engine(runner)?,
        1 => prepare(s)?,
        2 => {
            runner.checked(&build(s))?;
        }
        3 => {
            let n = s.names();
            ensure_network(&n.internal, true, &n.owner, runner)?;
            ensure_network(&n.uplink, false, &n.owner, runner)?;
        }
        4 => start_proxy(s, token, runner)?,
        5 => return Ok(StepResult::Models(check_models(s, runner)?)),
        6 => save_config(s)?,
        _ => bail!("Неизвестный шаг"),
    }
    Ok(StepResult::Done)
}

pub fn check_engine(runner: &Runner) -> Result<()> {
    let raw = runner
        .checked(&engine_info_command(runner.engine))
        .with_context(|| {
            format!(
                "{} недоступен. Запустите приложение и проверьте выбранное подключение.",
                runner.engine.label()
            )
        })?;
    validate_engine_info(runner.engine, &raw)
}

pub fn validate_engine_info(engine: Engine, raw: &str) -> Result<()> {
    let info: Value = serde_json::from_str(raw).context("Движок вернул некорректный JSON")?;
    match engine {
        Engine::Podman => {
            if info
                .pointer("/host/security/rootless")
                .and_then(Value::as_bool)
                != Some(true)
            {
                bail!(
                    "Нужна rootless Machine. Остановите Machine, выполните podman machine set --rootful=false и запустите её снова."
                );
            }
        }
        Engine::Docker => {
            if info.get("OSType").and_then(Value::as_str) != Some("linux") {
                bail!("Переключите Docker Desktop в режим Linux containers.");
            }
            if !info
                .get("OperatingSystem")
                .and_then(Value::as_str)
                .is_some_and(|s| s.to_lowercase().contains("docker desktop"))
            {
                bail!(
                    "Текущий Docker context не указывает на Docker Desktop. Проверьте docker context ls и выберите подключение Docker Desktop."
                );
            }
        }
    }
    Ok(())
}

fn prepare(s: &Settings) -> Result<()> {
    for part in ["image", "proxy/image", "config", "state/projects"] {
        fs::create_dir_all(s.env_root().join(part))?;
    }
    // Any change to the generated files invalidates a previous ready marker.
    invalidate_ready(s)?;
    atomic_write(
        &s.env_root().join("image/Containerfile"),
        CONTAINERFILE.as_bytes(),
    )?;
    atomic_write(
        &s.env_root().join("proxy/image/Containerfile"),
        PROXY_CONTAINERFILE.as_bytes(),
    )?;
    atomic_write(
        &s.env_root().join("proxy/Caddyfile"),
        s.caddyfile().as_bytes(),
    )?;
    Ok(())
}

fn invalidate_ready(s: &Settings) -> Result<()> {
    match fs::remove_file(s.env_root().join("config/ready.json")) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[derive(Clone, Copy)]
enum Resource {
    Network,
    Container,
    Image,
}

fn resource_exists(runner: &Runner, kind: Resource, name: &str) -> Result<bool> {
    if runner.engine == Engine::Podman {
        let kind = match kind {
            Resource::Network => "network",
            Resource::Container => "container",
            Resource::Image => "image",
        };
        return exists(runner, &cmd(&[kind, "exists", name]));
    }
    let command = match kind {
        Resource::Network => cmd(&[
            "network",
            "ls",
            "--filter",
            &format!("name={name}"),
            "--format",
            "{{.Name}}",
        ]),
        Resource::Container => cmd(&[
            "container",
            "ls",
            "--all",
            "--filter",
            &format!("name=^/{name}$"),
            "--format",
            "{{.Names}}",
        ]),
        Resource::Image => cmd(&[
            "image",
            "ls",
            "--filter",
            &format!("reference={name}"),
            "--format",
            "{{.Repository}}:{{.Tag}}",
        ]),
    };
    let names = runner.checked(&command)?;
    Ok(names.lines().any(|line| line.trim() == name))
}

fn exists(runner: &Runner, command: &Cmd) -> Result<bool> {
    let result = runner.run(command, None)?;
    match result.code {
        0 => Ok(true),
        1 => Ok(false),
        _ => {
            result.require()?;
            unreachable!()
        }
    }
}

fn ensure_network(name: &str, internal: bool, owner: &str, runner: &Runner) -> Result<()> {
    if resource_exists(runner, Resource::Network, name)? {
        let raw = runner.checked(&cmd(&["network", "inspect", name]))?;
        let list: Vec<Value> = serde_json::from_str(&raw)?;
        let net = list.first().context("Нет данных о сети")?;
        if net
            .get(if runner.engine == Engine::Docker {
                "Internal"
            } else {
                "internal"
            })
            .and_then(Value::as_bool)
            != Some(internal)
            || net
                .pointer(if runner.engine == Engine::Docker {
                    "/Labels/pi-launcher.owner"
                } else {
                    "/labels/pi-launcher.owner"
                })
                .and_then(Value::as_str)
                != Some(owner)
        {
            bail!("Сеть {name} имеет другую конфигурацию или владельца. Она не изменена.");
        }
    } else {
        runner.checked(&network_create(
            name,
            internal,
            &format!("pi-launcher.owner={owner}"),
        ))?;
    }
    Ok(())
}

fn start_proxy(s: &Settings, token: &str, runner: &Runner) -> Result<()> {
    let n = s.names();
    invalidate_ready(s)?;
    let has_proxy = resource_exists(runner, Resource::Container, &n.proxy)?;
    if has_proxy {
        verify_proxy_owner(&n, runner)?;
    }
    // Repeating step 5 also upgrades environments created by older launchers.
    // Build first so a build failure leaves the existing proxy and token intact.
    atomic_write(
        &s.env_root().join("proxy/image/Containerfile"),
        PROXY_CONTAINERFILE.as_bytes(),
    )?;
    runner.checked(&build_proxy(s))?;
    let docker_token = if s.engine == Engine::Docker {
        let value = if !token.is_empty() {
            token.to_owned()
        } else if has_proxy {
            let output = runner.private_output(&cmd(&[
                "inspect",
                "--format",
                "{{range .Config.Env}}{{println .}}{{end}}",
                &n.proxy,
            ]))?;
            extract_proxy_token(&output.stdout)?
        } else {
            bail!("Введите API-токен для первого запуска Docker-прокси.");
        };
        Some(zeroize::Zeroizing::new(value))
    } else {
        if !token.is_empty() {
            runner
                .run(
                    &cmd(&["secret", "create", "--replace", &n.secret, "-"]),
                    Some(token),
                )?
                .require()?;
        } else if !exists(runner, &cmd(&["secret", "exists", &n.secret]))? {
            bail!("Введите API-токен llama.cpp.");
        }
        None
    };
    if has_proxy {
        runner.checked(&cmd(&["rm", "-f", &n.proxy]))?;
    }
    if let Some(token) = docker_token {
        runner.with_token(&proxy_run(s), &token)?.require()?;
        runner.checked(&connect_proxy(s))?;
    } else {
        runner.checked(&proxy_run(s))?;
    }
    runner.checked(&cmd(&[
        "exec",
        &n.proxy,
        "caddy",
        "validate",
        "--config",
        "/etc/caddy/Caddyfile",
    ]))?;
    Ok(())
}

fn extract_proxy_token(output: &str) -> Result<String> {
    output
        .lines()
        .find_map(|line| line.strip_prefix("LLAMA_API_KEY="))
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .context("В существующем Docker-прокси нет токена. Введите его заново.")
}

fn verify_proxy_owner(n: &Names, runner: &Runner) -> Result<()> {
    let raw = runner.checked(&cmd(&[
        "inspect",
        "--format",
        "{{json .Config.Labels}}",
        &n.proxy,
    ]))?;
    let labels: Value = serde_json::from_str(&raw)?;
    if labels.get("pi-launcher.owner").and_then(Value::as_str) != Some(n.owner.as_str()) {
        bail!(
            "Контейнер {} не принадлежит этому окружению. Он не изменён.",
            n.proxy
        );
    }
    Ok(())
}

fn check_models(s: &Settings, runner: &Runner) -> Result<Vec<String>> {
    let n = s.names();
    verify_proxy_owner(&n, runner)?;
    let mut response = None;
    for attempt in 0..3 {
        let output = runner.run(&probe(s, false), None)?;
        if output.code == 0 {
            response = Some(output.stdout);
            break;
        }
        if attempt == 2 {
            output.require().context("Модель недоступна через прокси")?;
        }
        thread::sleep(Duration::from_millis(500));
    }
    let models = parse_models(&response.context("Модель не ответила")?, &runner.cancel)?;
    if runner.cancel.load(Ordering::Relaxed) {
        bail!("Операция отменена.");
    }
    let direct = runner.run(&probe(s, true), None)?;
    validate_isolation_exit(direct.code)?;
    (runner.log)("Прокси отвечает. Прямое соединение с указанным llama.cpp заблокировано.");
    Ok(models)
}

const MAX_MODEL_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_MODELS: usize = 4096;
const MAX_MODEL_ID_BYTES: usize = 1024;

pub fn parse_models(raw: &str, cancel: &AtomicBool) -> Result<Vec<String>> {
    if cancel.load(Ordering::Relaxed) {
        bail!("Операция отменена.");
    }
    if raw.len() > MAX_MODEL_RESPONSE_BYTES {
        bail!("Список моделей превышает допустимый размер (1 МиБ).");
    }
    let reply: Value =
        serde_json::from_str(raw).context("Сервер вернул некорректный JSON моделей")?;
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    if cancel.load(Ordering::Relaxed) {
        bail!("Операция отменена.");
    }
    if let Some(models) = reply.get("data").and_then(Value::as_array) {
        if models.len() > MAX_MODELS {
            bail!("Сервер вернул больше {MAX_MODELS} записей моделей.");
        }
        for model in models {
            if cancel.load(Ordering::Relaxed) {
                bail!("Операция отменена.");
            }
            if let Some(id) = model.get("id").and_then(Value::as_str)
                && id.len() > MAX_MODEL_ID_BYTES
            {
                bail!("Идентификатор модели превышает {MAX_MODEL_ID_BYTES} байт.");
            }
            if let Some(id) = model.get("id").and_then(Value::as_str)
                && !id.trim().is_empty()
                && seen.insert(id)
            {
                ids.push(id.to_owned());
            }
        }
    }
    if ids.is_empty() {
        bail!("Сервер не вернул ни одной модели.");
    }
    Ok(ids)
}

pub fn validate_isolation_exit(code: i32) -> Result<()> {
    match code {
        7 | 28 => Ok(()),
        0 => bail!(
            "Прямое соединение с llama.cpp доступно. Запуск заблокирован; проверьте внутреннюю сеть."
        ),
        _ => bail!(
            "Проверка изоляции не выполнена: код {code}. Ошибка движка/curl не считается подтверждением изоляции."
        ),
    }
}

fn policy() -> Value {
    // serde_json's default map ordering is alphabetical; explicit preserve_order is
    // enabled in Cargo.toml because permission pattern precedence must be retained.
    json!({"permission": {
        "*": "ask", "read": "allow", "edit": "ask", "write": "ask",
        "bash": {"*": "ask", "rm -rf *": "deny", "sudo *": "deny", "git push*": "deny"},
        "path": {"*": "allow", "*.env": "deny", "*.env.*": "deny", "*.env.example": "allow"},
        "external_directory": "deny", "mcp": "deny"
    }, "yoloMode": false})
}

fn save_config(s: &Settings) -> Result<()> {
    if s.model.trim().is_empty() {
        bail!("Выберите модель из списка после шага 6.");
    }
    let config = s.env_root().join("config");
    invalidate_ready(s)?;
    write_json(
        &config.join("models.json"),
        &json!({"providers": {"llama-lan": {
            "baseUrl": format!("{GATEWAY}/v1"), "api": "openai-completions",
            "apiKey": "local-placeholder", "authHeader": false,
            "compat": {"supportsDeveloperRole": false, "supportsReasoningEffort": false},
            "models": [{"id": s.model, "reasoning": false, "contextWindow": s.context_window, "maxTokens": s.max_tokens}]
        }}}),
    )?;
    write_json(
        &config.join("settings.json"),
        &json!({
            "defaultProvider": "llama-lan", "defaultModel": s.model, "defaultThinkingLevel": "off",
            "defaultProjectTrust": "never", "enableInstallTelemetry": false, "enableAnalytics": false,
            "defaultTools": ["read", "bash", "edit", "write"]
        }),
    )?;
    write_json(&config.join("permissions.json"), &policy())?;
    write_json(&config.join("ready.json"), s)?;
    Ok(())
}

pub fn is_configured(s: &Settings) -> bool {
    fs::read(s.env_root().join("config/ready.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Settings>(&b).ok())
        .is_some_and(|ready| ready == *s)
}

pub fn load_engine_profile(settings: &Settings) -> Settings {
    fs::read(settings.env_root().join("config/ready.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
        .filter(|profile| profile.engine == settings.engine && profile.root == settings.root)
        .unwrap_or_else(|| settings.clone())
}

pub fn run_args(s: &Settings, project: &Path, access: Access, session: Session) -> Vec<String> {
    let state = project_state(s, project);
    let n = s.names();
    let mut args: Vec<String> = [
        "run",
        "--rm",
        "-it",
        "--pull=never",
        "--network",
        &n.internal,
        "--read-only",
        "--cap-drop=all",
        "--security-opt=no-new-privileges",
        "--pids-limit=256",
        "--tmpfs=/tmp:rw,nosuid,nodev,size=256m",
        "-e",
        "HOME=/state",
        "-e",
        "PI_CODING_AGENT_DIR=/state/agent",
        "-e",
        "PI_OFFLINE=1",
        "-e",
        "PI_SKIP_VERSION_CHECK=1",
        "-e",
        "PI_TELEMETRY=0",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.extend(no_proxy(s.engine));
    match s.engine {
        Engine::Podman => args.push("--userns=keep-id".into()),
        Engine::Docker => args.extend(["--user".into(), "1000:1000".into()]),
    }
    args.push("--mount".into());
    let mode = match (s.engine, access) {
        (Engine::Podman, access) => format!(",{}", access.mount_mode()),
        (Engine::Docker, Access::ReadOnly) => ",readonly".into(),
        (Engine::Docker, Access::ReadWrite) => String::new(),
    };
    args.push(format!(
        "type=bind,src={},dst=/workspace{mode}",
        project.display()
    ));
    args.push("--mount".into());
    args.push(format!(
        "type=bind,src={},dst=/state{}",
        state.display(),
        if s.engine == Engine::Podman {
            ",rw"
        } else {
            ""
        }
    ));
    args.extend(
        [
            "-w",
            "/workspace",
            IMAGE,
            "--no-extensions",
            "-e",
            "/usr/local/lib/node_modules/@gotgenes/pi-permission-system",
            "--no-mcp",
            "--no-approve",
            "--no-skills",
            "--tools",
            if access == Access::ReadOnly {
                "read,bash"
            } else {
                "read,bash,edit,write"
            },
        ]
        .iter()
        .map(|s| s.to_string()),
    );
    if let Some(flag) = session.flag() {
        args.push(flag.into());
    }
    args
}

pub fn launch(
    s: &Settings,
    project: &str,
    data_dir: &Path,
    access: Access,
    session: Session,
    runner: &Runner,
) -> Result<PathBuf> {
    if runner.engine != s.engine {
        bail!("Выбран другой контейнерный движок.");
    }
    let project = validate_project(project, s, data_dir)?;
    if !is_configured(s) {
        bail!(
            "Завершите настройку окружения: параметры изменились или настройка ещё не выполнена."
        );
    }
    check_engine(runner)?;
    let n = s.names();
    ensure_network(&n.internal, true, &n.owner, runner)?;
    verify_proxy_owner(&n, runner)?;
    let running = runner.checked(&cmd(&[
        "inspect",
        "--format",
        "{{.State.Running}}",
        &n.proxy,
    ]))?;
    if running.trim() != "true" {
        // No automatic replacement: a stopped owned proxy can be safely restarted.
        runner.checked(&cmd(&["start", &n.proxy]))?;
    }
    if !resource_exists(runner, Resource::Image, IMAGE)? {
        bail!("Образ Pi отсутствует. Повторите шаг сборки.");
    }
    let models = check_models(s, runner)?;
    if !models.contains(&s.model) {
        bail!("Сохранённая модель отсутствует на сервере. Выберите модель заново.");
    }
    let agent = project_state(s, &project).join("agent");
    fs::create_dir_all(agent.join("extensions/pi-permission-system"))?;
    for name in ["models.json", "settings.json"] {
        let bytes = fs::read(s.env_root().join("config").join(name))?;
        atomic_write(&agent.join(name), &bytes)?;
    }
    let bytes = fs::read(s.env_root().join("config/permissions.json"))?;
    atomic_write(
        &agent.join("extensions/pi-permission-system/config.json"),
        &bytes,
    )?;
    let args = run_args(s, &project, access, session);
    (runner.log)(&Cmd::new(args.clone()).preview(s.engine));
    open_terminal(s.engine, &args)?;
    Ok(project)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docker_has_portable_flags_and_only_the_internal_agent_network() {
        let s = Settings {
            engine: Engine::Docker,
            ..Settings::default()
        };
        let project = Path::new(r"D:\Project with spaces");
        let ro = run_args(&s, project, Access::ReadOnly, Session::New);
        assert!(ro.iter().any(|a| a.ends_with("dst=/workspace,readonly")));
        assert!(ro.windows(2).any(|a| a == ["--user", "1000:1000"]));
        assert!(ro.iter().any(|a| a == "HTTP_PROXY="));
        assert!(ro.iter().any(|a| a == "https_proxy="));
        assert!(!ro.iter().any(|a| a.contains("--http-proxy")
            || a.contains("keep-id")
            || a.contains("LLAMA_API_KEY")));
        assert_eq!(ro.iter().filter(|a| *a == "--network").count(), 1);
        assert!(!ro.iter().any(|a| a == &s.names().uplink));
        let rw = run_args(&s, project, Access::ReadWrite, Session::Continue);
        assert!(rw.iter().any(|a| a.ends_with("dst=/workspace")));
        assert_eq!(rw.last().unwrap(), "--continue");
    }

    #[test]
    fn docker_proxy_uses_env_name_without_token_or_swarm() {
        let s = Settings {
            engine: Engine::Docker,
            ..Settings::default()
        };
        let args = proxy_run(&s).args;
        assert_eq!(args.last().unwrap(), CADDY);
        assert!(args.iter().any(|arg| arg == "--pull=never"));
        assert!(args.iter().any(|arg| arg == "--cap-drop=all"));
        assert!(
            args.iter()
                .any(|arg| arg == "--security-opt=no-new-privileges")
        );
        assert!(args.windows(2).any(|a| a == ["--env", "LLAMA_API_KEY"]));
        assert!(
            !args
                .iter()
                .any(|a| a.contains("--secret") || a.contains("--http-proxy"))
        );
        assert_eq!(args.iter().filter(|a| *a == "--network").count(), 1);
        assert_eq!(
            connect_proxy(&s).args,
            vec![
                "network",
                "connect",
                "--alias",
                "llama-gateway",
                &s.names().internal,
                &s.names().proxy
            ]
        );
        assert!(!build(&s).args.iter().any(|a| a == "--pull=missing"));
        assert!(build(&s).args.iter().any(|a| a == "--load"));
        for step in 0..7 {
            let preview = step_instructions(step, &s)
                .into_iter()
                .filter_map(|item| item.snippet)
                .map(|snippet| snippet.text)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(!preview.contains("podman "));
            assert!(!preview.contains("docker 'secret'"));
        }
    }

    #[test]
    fn preflight_rejects_windows_containers_remote_engine_and_rootful_podman() {
        assert!(
            validate_engine_info(
                Engine::Docker,
                r#"{"OSType":"linux","OperatingSystem":"Docker Desktop"}"#
            )
            .is_ok()
        );
        assert!(
            validate_engine_info(
                Engine::Docker,
                r#"{"OSType":"windows","OperatingSystem":"Docker Desktop"}"#
            )
            .is_err()
        );
        assert!(
            validate_engine_info(
                Engine::Docker,
                r#"{"OSType":"linux","OperatingSystem":"Ubuntu"}"#
            )
            .is_err()
        );
        assert!(validate_engine_info(Engine::Docker, "{}").is_err());
        assert!(
            validate_engine_info(Engine::Podman, r#"{"host":{"security":{"rootless":true}}}"#)
                .is_ok()
        );
        assert!(
            validate_engine_info(
                Engine::Podman,
                r#"{"host":{"security":{"rootless":false}}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn docker_reuses_only_the_exact_nonempty_token_variable() {
        assert_eq!(
            extract_proxy_token("OTHER_KEY=x\nLLAMA_API_KEY=local=value\nHOME=/root\n").unwrap(),
            "local=value"
        );
        assert!(extract_proxy_token("NOT_LLAMA_API_KEY=other\nLLAMA_API_KEY=\n").is_err());
    }

    #[test]
    fn engine_configs_and_history_remain_separate() {
        let temp = tempfile::tempdir().unwrap();
        let podman = Settings {
            root: temp.path().to_string_lossy().into_owned(),
            model: "podman-model".into(),
            ..Settings::default()
        };
        save_config(&podman).unwrap();
        let docker = Settings {
            engine: Engine::Docker,
            model: "docker-model".into(),
            ..podman.clone()
        };
        prepare(&docker).unwrap();
        assert!(is_configured(&podman));
        assert!(!is_configured(&docker));
        save_config(&docker).unwrap();
        assert!(is_configured(&podman) && is_configured(&docker));
        assert_ne!(
            project_state(&podman, Path::new(r"D:\Project")),
            project_state(&docker, Path::new(r"D:\Project"))
        );
        assert_ne!(podman.names().owner, docker.names().owner);
        let switch_back = Settings {
            engine: Engine::Podman,
            ..docker.clone()
        };
        assert_eq!(load_engine_profile(&switch_back), podman);
        assert_eq!(load_engine_profile(&docker), docker);
    }

    #[test]
    fn readonly_is_enforced_at_mount_and_tool_levels() {
        let s = Settings::default();
        let project = Path::new(r"D:\Bob's $project");
        let ro = run_args(&s, project, Access::ReadOnly, Session::Continue);
        assert!(ro.iter().any(|v| v.ends_with("dst=/workspace,ro")));
        assert!(ro.iter().any(|v| v == "read,bash"));
        assert_eq!(ro.last().unwrap(), "--continue");
        assert!(!ro.iter().any(|v| v.contains("LLAMA_API_KEY")));
        assert!(ro.iter().any(|v| v == "--network"));
        assert!(!ro.iter().any(|v| v == &s.names().uplink));
        let rw = run_args(&s, project, Access::ReadWrite, Session::Resume);
        assert!(rw.iter().any(|v| v.ends_with("dst=/workspace,rw")));
        assert_eq!(rw.last().unwrap(), "--resume");
    }

    #[test]
    fn runtime_errors_do_not_pass_isolation_check() {
        assert!(validate_isolation_exit(7).is_ok());
        assert!(validate_isolation_exit(28).is_ok());
        for code in [0, 1, 6, 22, 125, 126, 127, -1] {
            assert!(validate_isolation_exit(code).is_err());
        }
    }

    #[test]
    fn parses_only_nonempty_unique_model_ids() {
        let cancel = AtomicBool::new(false);
        assert_eq!(
            parse_models(
                r#"{"data":[{"id":"a"},{"id":"a"},{"id":""},{"id":"b"}]}"#,
                &cancel
            )
            .unwrap(),
            vec!["a", "b"]
        );
        assert!(parse_models("{}", &cancel).is_err());
        assert!(parse_models("not json", &cancel).is_err());
        assert_eq!(
            parse_models(
                r#"{"data":[{"id":" модель "},{"id":"  "},{"id":3},{"id":" модель "},{"id":"b"}]}"#,
                &cancel
            )
            .unwrap(),
            vec![" модель ", "b"]
        );
    }

    #[test]
    fn model_limits_reject_oversized_data_without_truncating_it() {
        let cancel = AtomicBool::new(false);
        assert!(
            parse_models(&" ".repeat(MAX_MODEL_RESPONSE_BYTES + 1), &cancel)
                .unwrap_err()
                .to_string()
                .contains("1 МиБ")
        );
        let too_many = json!({"data": vec![json!({"id":"same"}); MAX_MODELS + 1]}).to_string();
        assert!(
            parse_models(&too_many, &cancel)
                .unwrap_err()
                .to_string()
                .contains("записей")
        );
        let long_id = json!({"data": [{"id": "я".repeat(MAX_MODEL_ID_BYTES)}]}).to_string();
        assert!(
            parse_models(&long_id, &cancel)
                .unwrap_err()
                .to_string()
                .contains("Идентификатор")
        );
        let valid = json!({"data": (0..MAX_MODELS).map(|i| json!({"id": format!("model-{i}")})).collect::<Vec<_>>()} ).to_string();
        let parsed = parse_models(&valid, &cancel).unwrap();
        assert_eq!(parsed.len(), MAX_MODELS);
        assert_eq!(parsed[0], "model-0");
        assert_eq!(parsed.last().unwrap(), &format!("model-{}", MAX_MODELS - 1));
        let id = "x".repeat(MAX_MODEL_ID_BYTES);
        assert_eq!(
            parse_models(&json!({"data":[{"id":id}]}).to_string(), &cancel).unwrap(),
            vec![id]
        );
        cancel.store(true, Ordering::Relaxed);
        assert!(
            parse_models("not json", &cancel)
                .unwrap_err()
                .to_string()
                .contains("отменена")
        );
    }

    #[test]
    fn only_builds_may_accept_truncated_output() {
        for engine in [Engine::Docker, Engine::Podman] {
            let settings = Settings {
                engine,
                ..Settings::default()
            };
            assert!(build(&settings).allow_truncated_output);
            let proxy_build = build_proxy(&settings);
            assert!(proxy_build.allow_truncated_output);
            assert!(
                proxy_build
                    .args
                    .windows(2)
                    .any(|args| args == ["-t", CADDY])
            );
            assert_eq!(
                Path::new(proxy_build.args.last().unwrap()),
                settings.env_root().join("proxy/image")
            );
            assert!(PROXY_CONTAINERFILE.contains("RUN setcap -r /usr/bin/caddy"));
            for direct in [false, true] {
                let command = probe(&settings, direct);
                assert!(!command.allow_truncated_output);
                assert_eq!(command.output_limit, MAX_MODEL_RESPONSE_BYTES);
            }
            assert!(!engine_info_command(engine).allow_truncated_output);
        }
    }

    #[test]
    fn changed_settings_invalidate_ready_marker() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Settings {
            root: dir.path().to_string_lossy().into_owned(),
            model: "test".into(),
            ..Settings::default()
        };
        save_config(&s).unwrap();
        assert!(is_configured(&s));
        s.port = 9999;
        assert!(!is_configured(&s));
        s.port = 8080;
        prepare(&s).unwrap();
        assert!(!is_configured(&s));
    }
}
