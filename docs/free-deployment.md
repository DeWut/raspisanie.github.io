# Бесплатное размещение backend

Для Rust API подготовлен Render Blueprint в корне репозитория: `render.yaml`.

## Почему Render Free подходит без банковской карты

Render позволяет создать бесплатный Web Service без оплаты и банковской карты. Бесплатный экземпляр засыпает после 15 минут без входящих запросов и просыпается при следующем запросе; пробуждение может занять около минуты. Это не круглосуточный сервер с гарантированной задержкой.

В этом проекте включён режим Telegram webhook: Telegram отправляет обновления на HTTP endpoint, а не ждёт постоянного long-poll соединения. Поэтому входящее сообщение может разбудить спящий экземпляр. После пробуждения ответ может задержаться. Фоновые уведомления и напоминания во время сна не гарантируются.

## Развёртывание

1. Войти на Render через GitHub: https://dashboard.render.com/
2. Нажать **New → Blueprint** и выбрать репозиторий `DeWut/raspisanie.github.io`.
3. Выбрать ветку `feature/smart-campus-step-1`, если Render предлагает выбор ветки.
4. Render прочитает `render.yaml` и создаст бесплатный Rust Web Service в регионе Frankfurt.
5. В форме переменных окружения указать:
   - `DATABASE_URL` — pooled URL существующей Neon PostgreSQL.
   - `DATABASE_URL_UNPOOLED` — direct/unpooled URL той же Neon базы для миграций и advisory locks.
   - `TELOXIDE_TOKEN` — токен Telegram-бота из BotFather.
   - `WEBAPP_URL` — публичный адрес Mini App на GitHub Pages.
   - `ADMIN_TELEGRAM_ID` — Telegram ID главного администратора.
6. `TELEGRAM_WEBHOOK_SECRET` Render сгенерирует автоматически. Не публиковать его.
7. После успешного запуска скопировать URL сервиса Render, например `https://<service>.onrender.com`.
8. В `miniapp/index.html` заполнить meta-тег `smart-campus-api` этим URL и опубликовать изменение в той же feature-ветке. До этого Mini App не знает адрес API.

## Режим Telegram

Если присутствуют `TELEGRAM_WEBHOOK_SECRET` и `RENDER_EXTERNAL_URL` (Render выставляет его автоматически), backend сам регистрирует Telegram webhook на `/telegram/webhook`. Запросы проверяются по заголовку `X-Telegram-Bot-Api-Secret-Token`. Локальный запуск без этих переменных сохраняет прежний long polling.

Не запускайте одновременно локальный long-polling экземпляр с тем же токеном, пока Render работает в webhook-режиме: polling вызывает удаление webhook.

## Локальная модель

Бесплатный Render Web Service имеет только 512 MB RAM и не подходит для запуска LLM. Для полностью локального инференса можно использовать отдельный компьютер пользователя с Ollama и защищённым туннелем Tailscale Funnel. Это требует, чтобы компьютер был включён и подключён к интернету. Не выставляйте Ollama порт 11434 напрямую в публичный интернет: перед туннелем нужен небольшой proxy с Bearer-токеном, который пропускает только запросы backend.

Пока адрес и параметры локального узла не настроены, помощник должен продолжать работать в существующем rule-based режиме, без внешнего AI API.
