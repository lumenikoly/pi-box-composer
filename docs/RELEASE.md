# Релиз pi-box-composer

Релиз запускается вручную из main через workflow **Release executable**.
Автоматического запуска по push или созданию тега нет. После успешной сборки
workflow создаёт тег v<version> на исходном коммите и публикует GitHub Release.

## Подготовка

1. Обновите версию в Cargo.toml, записи пакета pi-box-composer в Cargo.lock
   и заголовке CHANGELOG.md. Текущая версия — 0.0.5.
2. Закоммитьте исходники в main и отправьте ветку на GitHub.
3. Откройте **Actions → Release executable → Run workflow**, выберите main
   и укажите 0.0.5 без префикса v.

С GitHub CLI тот же запуск выполняется так:

    gh workflow run release.yml --ref main -f version=0.0.5

Workflow проверяет ветку, имя и версии пакета, release notes и отсутствие
существующего тега. Затем Windows runner собирает приложение командой:

    cargo build --release --locked --target x86_64-pc-windows-msvc

Все jobs используют один исходный коммит, выбранный при ручном запуске.
Публикация разрешена только после успешной сборки. Тестовые jobs в workflow
не добавлены. Используется штатный GITHUB_TOKEN; отдельный PAT не требуется.
Право contents: write выдано только job публикации.

## Файлы релиза

    pi-box-composer_0.0.5_windows_x64.exe
    SHA256SUMS

EXE не подписан. В релиз не входят Rust, Podman, Docker Desktop, контейнерные
образы или сервер модели; движок нужно установить отдельно. Linux и macOS
не публикуются: отдельный интерактивный терминал приложения реализован для Windows.

Имя приложения и EXE изменено на pi-box-composer. Каталог %LOCALAPPDATA%\PiLauncher,
внутренние имена контейнеров и labels сохранены для совместимости с настройками.

## Локальная сборка

    cargo build --release --locked
    .\target\release\pi-box-composer.exe

Каталоги target/, .local/ и release/ исключены из Git.
