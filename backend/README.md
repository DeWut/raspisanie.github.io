# SMART CAMPUS backend

## Local run

1. Install PostgreSQL 16 and pgvector.
2. Copy `backend/.env.example` to `backend/.env` (or, from the `backend` directory, run `cp .env.example .env`). Set `DATABASE_URL` and `TELOXIDE_TOKEN`. Set `OPENAI_API_KEY` too if you want to use RAG/AI features.
3. Create the database and ensure the PostgreSQL user can install/use the `vector` and `pg_trgm` extensions.
4. Run the service from this directory:

```bash
cargo run
```

The service applies SQLx migrations at startup, starts the HTTP API on `PORT` (default 8080), and starts Telegram long polling when `TELOXIDE_TOKEN` is configured.

## Telegram commands

- `/start`, `/help` — command overview
- `/group GROUP_NAME` — choose a group
- `/next` — next scheduled class
- `/today` — today's schedule
- `/week` — next seven days
- `/remind_on`, `/remind_off` — enable/disable reminders 10 minutes before classes
- `/settings` — show saved group and reminder state
- `/homework TEXT` — manually add a task to the selected group's task list
- `/clear` — ask the bot to delete up to 30 of its own recent replies in this chat

The bot intentionally does not ingest ordinary group-chat messages yet. Chat-based homework extraction needs explicit group opt-in, a privacy notice, and a student confirmation step before a task is published.

## Current limitations

- Schedule commands depend on the EIOS sync/import process having populated `groups` and `lessons`.
- Reminders are delivered by an in-process polling loop. For production, run a single bot worker (or move scheduling to a distributed job queue) to avoid duplicate polling workers.
- Telegram only permits deletion of eligible bot messages in chats where the bot has permission; deletion is best-effort.
- Configure the bot with a Telegram BotFather token and keep it secret. The RAG service uses `OPENAI_API_KEY`; basic Telegram commands do not require it. Both keys are read from environment variables / `.env`. Do not commit `.env`.
