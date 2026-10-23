use crate::model::Engine;
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub type Logger = Arc<dyn Fn(&str) + Send + Sync>;
pub const MAX_LOG_BYTES: usize = 4096;
const MAX_CAPTURE_BYTES: usize = 8 * 1024 * 1024;

pub fn log_text(text: &str) -> String {
    if text.len() <= MAX_LOG_BYTES {
        return text.to_owned();
    }
    const MARKER: &str = " …[сокращено]";
    let mut end = MAX_LOG_BYTES - MARKER.len();
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{MARKER}", &text[..end])
}

#[derive(Clone, Debug)]
pub struct Cmd {
    pub args: Vec<String>,
    pub timeout: Duration,
    pub output_limit: usize,
    pub allow_truncated_output: bool,
}

impl Cmd {
    pub fn new(args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            args: args.into_iter().map(Into::into).collect(),
            timeout: Duration::from_secs(90),
            output_limit: MAX_CAPTURE_BYTES,
            allow_truncated_output: false,
        }
    }
    pub fn preview(&self, engine: Engine) -> String {
        format!(
            "{} {}",
            engine.cli(),
            self.args
                .iter()
                .map(|s| ps_quote(s))
                .collect::<Vec<_>>()
                .join(" ")
        )
    }

    pub fn formatted(&self, engine: Engine) -> String {
        let mut text = engine.cli().to_owned();
        let mut column = text.len();
        for arg in &self.args {
            let arg = ps_quote(arg);
            if column + arg.chars().count() + 1 > 78 {
                text.push_str(" `\n    ");
                column = 4;
            } else {
                text.push(' ');
                column += 1;
            }
            text.push_str(&arg);
            column += arg.chars().count();
        }
        text
    }
}

pub fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn require(self) -> Result<String> {
        if self.code != 0 {
            bail!(
                "Команда завершилась с кодом {}: {}",
                self.code,
                self.stderr.trim()
            );
        }
        Ok(self.stdout)
    }
}

pub struct Runner {
    pub engine: Engine,
    pub log: Logger,
    pub cancel: Arc<AtomicBool>,
}

struct Captured {
    text: String,
    truncated: bool,
}

fn read_stream(
    mut reader: impl Read,
    log: Logger,
    hidden: bool,
    limit: usize,
    overflow: Arc<AtomicBool>,
    allow_truncated: bool,
) -> Result<Captured> {
    // Drain pipes even after the capture fills. Only builds may accept truncation.
    let mut captured = Vec::new();
    let mut pending = Vec::new();
    let mut chunk = [0_u8; 8192];
    let mut truncated = false;
    loop {
        let count = match reader.read(&mut chunk) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            break;
        }
        let remaining = limit.saturating_sub(captured.len());
        captured.extend_from_slice(&chunk[..count.min(remaining)]);
        truncated |= count > remaining;
        if truncated && !allow_truncated {
            overflow.store(true, Ordering::Relaxed);
        }
        if !hidden {
            for &byte in &chunk[..count] {
                pending.push(byte);
                if byte == b'\n' || pending.len() >= MAX_LOG_BYTES {
                    // Keep a partial UTF-8 character for the next fragment.
                    let end = match std::str::from_utf8(&pending) {
                        Err(error) if error.error_len().is_none() => error.valid_up_to(),
                        _ => pending.len(),
                    };
                    if end != 0 {
                        log(&log_text(
                            String::from_utf8_lossy(&pending[..end]).trim_end(),
                        ));
                        pending.drain(..end);
                    }
                }
            }
        }
    }
    if !hidden && !pending.is_empty() {
        log(&log_text(String::from_utf8_lossy(&pending).trim_end()));
    }
    let mut text = String::from_utf8_lossy(&captured).into_owned();
    // Lossy decoding may expand invalid input. Bound the decoded representation too.
    if text.len() > limit {
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        truncated = true;
    }
    if truncated && !allow_truncated {
        overflow.store(true, Ordering::Relaxed);
    }
    Ok(Captured { text, truncated })
}

impl Runner {
    pub fn run(&self, cmd: &Cmd, stdin: Option<&str>) -> Result<Output> {
        self.run_inner(cmd, stdin, None, stdin.is_some())
    }

    pub fn private_output(&self, cmd: &Cmd) -> Result<Output> {
        self.run_inner(cmd, None, None, true)
    }

    pub fn with_token(&self, cmd: &Cmd, token: &str) -> Result<Output> {
        self.run_inner(cmd, None, Some(token), true)
    }

    fn run_inner(
        &self,
        cmd: &Cmd,
        stdin: Option<&str>,
        token: Option<&str>,
        hidden: bool,
    ) -> Result<Output> {
        if self.cancel.load(Ordering::Relaxed) {
            bail!("Операция отменена.");
        }
        (self.log)(&format!(
            "> {}{}",
            cmd.preview(self.engine),
            if stdin.is_some() {
                "  # секрет передаётся через stdin"
            } else {
                ""
            }
        ));
        let mut command = Command::new(self.engine.cli());
        command
            .args(&cmd.args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
        if let Some(token) = token {
            command.env("LLAMA_API_KEY", token);
        }
        hide_window(&mut command);
        let mut child = command.spawn().with_context(|| {
            format!(
                "Не удалось запустить {}. Установите {} и добавьте CLI в PATH.",
                self.engine.cli(),
                self.engine.label()
            )
        })?;
        let out = child
            .stdout
            .take()
            .context("Нет stdout дочернего процесса")?;
        let err = child
            .stderr
            .take()
            .context("Нет stderr дочернего процесса")?;
        let overflow = Arc::new(AtomicBool::new(false));
        let logger = self.log.clone();
        let limit = cmd.output_limit;
        let allow_truncated = cmd.allow_truncated_output;
        let flag = overflow.clone();
        let stdout =
            thread::spawn(move || read_stream(out, logger, hidden, limit, flag, allow_truncated));
        let logger = self.log.clone();
        let flag = overflow.clone();
        let stderr =
            thread::spawn(move || read_stream(err, logger, hidden, limit, flag, allow_truncated));
        if let Some(input) = stdin {
            let result = child
                .stdin
                .take()
                .context("Нет stdin дочернего процесса")?
                .write_all(input.as_bytes());
            if let Err(error) = result {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout.join();
                let _ = stderr.join();
                return Err(error).context("Не удалось передать токен контейнерному движку");
            }
        }
        let started = Instant::now();
        let mut aborted = None;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if self.cancel.load(Ordering::Relaxed)
                || started.elapsed() > cmd.timeout
                || overflow.load(Ordering::Relaxed)
            {
                aborted = Some(if self.cancel.load(Ordering::Relaxed) {
                    "Операция отменена"
                } else if overflow.load(Ordering::Relaxed) {
                    "Вывод команды превышает допустимый размер"
                } else {
                    "Превышено время ожидания команды"
                });
                let _ = child.kill();
                break child.wait()?;
            }
            thread::sleep(Duration::from_millis(80));
        };
        let out = stdout
            .join()
            .map_err(|_| anyhow::anyhow!("Ошибка чтения stdout"))??;
        let err = stderr
            .join()
            .map_err(|_| anyhow::anyhow!("Ошибка чтения stderr"))??;
        if let Some(reason) = aborted {
            bail!("{reason}. Выполненные шаги сохранены; проверьте состояние перед повтором.");
        }
        if !allow_truncated && (out.truncated || err.truncated) {
            bail!("Вывод команды превышает допустимый размер ({limit} байт).");
        }
        let code = status.code().unwrap_or(-1);
        if hidden && code != 0 {
            bail!("Команда завершилась с кодом {code}. Вывод с токеном скрыт.");
        }
        Ok(Output {
            code,
            stdout: out.text,
            stderr: err.text,
        })
    }

    pub fn checked(&self, cmd: &Cmd) -> Result<String> {
        self.run(cmd, None)?.require()
    }
}

#[cfg(windows)]
pub fn hide_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW for background operations only
}

#[cfg(not(windows))]
pub fn hide_window(_: &mut Command) {}

pub fn encoded_powershell(script: &str) -> String {
    STANDARD.encode(
        script
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
}

pub fn terminal_script(engine: Engine, args: &[String]) -> String {
    let executable = engine.cli();
    let values = args
        .iter()
        .map(|s| ps_quote(s))
        .collect::<Vec<_>>()
        .join(",\n    ");
    format!(
        r#"$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$Host.UI.RawUI.WindowTitle = 'Pi — isolated workspace'
$piArgs = @(
    {values}
)
& {executable} @piArgs
if ($LASTEXITCODE -ne 0) {{ Write-Host "Pi exited with code $LASTEXITCODE" -ForegroundColor Red }}
"#
    )
}

pub fn open_terminal(engine: Engine, args: &[String]) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let windows = std::env::var_os("SystemRoot").context("SystemRoot не определён")?;
        let exe =
            std::path::Path::new(&windows).join("System32/WindowsPowerShell/v1.0/powershell.exe");
        Command::new(exe)
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NoExit",
                "-EncodedCommand",
                &encoded_powershell(&terminal_script(engine, args)),
            ])
            .creation_flags(0x00000010) // CREATE_NEW_CONSOLE: user explicitly requested an interactive terminal
            .spawn()
            .context("Не удалось открыть терминал PowerShell")?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (engine, args);
        bail!("Запуск отдельного терминала поддерживается только в Windows.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_output_is_bounded_drained_and_flagged() {
        use std::sync::atomic::AtomicUsize;
        for allow_truncated in [false, true] {
            let mut input = std::io::Cursor::new(vec![b'x'; 9 * 1024 * 1024]);
            let maximum = Arc::new(AtomicUsize::new(0));
            let count = maximum.clone();
            let log: Logger = Arc::new(move |line| {
                count.fetch_max(line.len(), Ordering::Relaxed);
            });
            let overflow = Arc::new(AtomicBool::new(false));
            let result = read_stream(
                &mut input,
                log,
                false,
                MAX_CAPTURE_BYTES,
                overflow.clone(),
                allow_truncated,
            )
            .unwrap();
            assert_eq!(input.position(), 9 * 1024 * 1024);
            assert_eq!(result.text.len(), MAX_CAPTURE_BYTES);
            assert!(result.truncated);
            assert_eq!(overflow.load(Ordering::Relaxed), !allow_truncated);
            assert!(maximum.load(Ordering::Relaxed) <= MAX_LOG_BYTES);
        }
    }

    #[test]
    fn capture_preserves_split_unicode_and_hidden_output() {
        struct Bytewise(std::io::Cursor<Vec<u8>>);
        impl Read for Bytewise {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                self.0.read(&mut buffer[..1])
            }
        }
        let text = format!("{}😀\nостаток", "x".repeat(MAX_LOG_BYTES - 1));
        let lines = Arc::new(std::sync::Mutex::new(Vec::new()));
        let collected = lines.clone();
        let log: Logger = Arc::new(move |line| collected.lock().unwrap().push(line.to_owned()));
        let overflow = Arc::new(AtomicBool::new(false));
        let result = read_stream(
            Bytewise(std::io::Cursor::new(text.as_bytes().to_vec())),
            log,
            false,
            text.len(),
            overflow.clone(),
            false,
        )
        .unwrap();
        assert_eq!(result.text, text);
        assert!(!result.truncated);
        assert!(!overflow.load(Ordering::Relaxed));
        assert!(
            lines
                .lock()
                .unwrap()
                .iter()
                .all(|line| line.len() <= MAX_LOG_BYTES && !line.contains('\u{fffd}'))
        );
        let log: Logger = Arc::new(|_| panic!("hidden output reached logger"));
        let result = read_stream(
            std::io::Cursor::new(b"secret"),
            log,
            true,
            6,
            overflow,
            false,
        )
        .unwrap();
        assert_eq!(result.text, "secret");
        assert!(!result.truncated);
    }

    #[test]
    fn lossy_decoding_and_log_truncation_stay_within_byte_limits() {
        let overflow = Arc::new(AtomicBool::new(false));
        let result = read_stream(
            std::io::Cursor::new(vec![0xff; 100]),
            Arc::new(|_| {}),
            false,
            100,
            overflow.clone(),
            false,
        )
        .unwrap();
        assert!(result.text.len() <= 100);
        assert!(result.truncated);
        assert!(overflow.load(Ordering::Relaxed));
        let line = log_text(&"😀".repeat(MAX_LOG_BYTES));
        assert!(line.len() <= MAX_LOG_BYTES);
        assert!(line.ends_with("[сокращено]"));
    }

    #[test]
    fn terminal_and_preview_use_the_selected_cli() {
        let command = Cmd::new(["run", "--rm", "test-image"]);
        assert!(command.preview(Engine::Docker).starts_with("docker "));
        assert!(command.preview(Engine::Podman).starts_with("podman "));
        let script = terminal_script(Engine::Docker, &command.args);
        assert!(script.contains("& docker @piArgs"));
        assert!(!script.contains("& podman"));
        assert!(!script.contains("LLAMA_API_KEY"));
    }

    #[test]
    fn quotes_paths_as_literal_powershell_arguments() {
        assert_eq!(
            ps_quote("D:\\Bob's $project` ; $(whoami)"),
            "'D:\\Bob''s $project` ; $(whoami)'"
        );
        let args = vec![
            "run".into(),
            "type=bind,src=D:\\Bob's $project,dst=/workspace,ro".into(),
        ];
        let script = terminal_script(Engine::Podman, &args);
        let encoded = STANDARD.decode(encoded_powershell(&script)).unwrap();
        let words: Vec<u16> = encoded
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .collect();
        assert_eq!(String::from_utf16(&words).unwrap(), script);
        assert!(script.contains("Bob''s $project"));
        assert!(script.contains("& podman @piArgs"));
    }
}
