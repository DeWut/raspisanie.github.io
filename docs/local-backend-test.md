# Полностью локальный тест backend + Ollama (без Render)

Это самый простой путь для первичного тестирования, если Render просит банковскую карту. В этом режиме Rust backend, Telegram long polling и Ollama работают на одном Windows-компьютере. Tailscale Funnel даёт Mini App публичный HTTPS-доступ к локальному backend. Платная инфраструктура не нужна.

## Перед запуском

- Установлены Rust toolchain, Cargo, доступ к Neon PostgreSQL и Ollama.
- В backend/.env настроены существующие секреты проекта. Не коммить этот файл и не отправляй его содержимое в чат.
- Ollama запущен, модель скачана командой ollama pull lfm3.
- Для локального backend используй следующие дополнительные переменные:

```dotenv
PORT=8080
OLLAMA_BASE_URL=http://127.0.0.1:11434
OLLAMA_API_KEY=local-development-only
OLLAMA_MODEL=lfm3
CORS_ALLOWED_ORIGINS=https://dewut.github.io
DEV_BYPASS_AUTH=false
```

В том же файле должны быть реальные значения DATABASE_URL, TELOXIDE_TOKEN, WEBAPP_URL и ADMIN_TELEGRAM_ID. При наличии DATABASE_URL_UNPOOLED backend использует его для миграций и блокировки long polling. Не публикуй значения этих переменных.

## Запуск

1. Останови Render Web Service, если он уже запущен с тем же Telegram bot token. Локальный long polling удаляет webhook и перехватывает обновления; два экземпляра одновременно работать не должны.
2. В PowerShell:

```powershell
cd backend
cargo run --release
```

3. В другом PowerShell окне включи Funnel:

```powershell
tailscale funnel --bg 8080
tailscale funnel status
```

4. Проверь публичный адрес https://<имя-устройства>.<tailnet>.ts.net/health. Должен вернуться успешный ответ health endpoint.
5. Передай мне только публичный HTTPS URL Funnel — без токенов, .env и других секретов. Я смогу прописать его в miniapp/index.html как API URL в feature-ветке. После публикации Pages Mini App будет обращаться к backend на твоём компьютере.

## Ограничения

Компьютер должен оставаться включённым, не переходить в сон и иметь интернет. Если он выключен, Telegram-бот и API будут недоступны. Funnel публикует только порт backend, а Ollama остаётся на 127.0.0.1:11434 и напрямую в интернет не выставляется.

Для остановки Funnel выполни tailscale funnel reset. Для остановки backend нажми Ctrl+C в окне Cargo. Для возврата к Render останови локальный backend, затем запусти Render service; Render заново установит webhook.

## Если Cargo не найден

Установи Rust через https://rustup.rs, перезапусти PowerShell и проверь cargo --version. Не запускай backend с DEV_BYPASS_AUTH=true на публичном Funnel.
