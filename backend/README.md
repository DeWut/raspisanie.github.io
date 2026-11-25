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

## Telegram Mini App

The static Mini App lives in `../miniapp/` and reads the published `../data/groups.json` and `../data/<group-id>.json` schedule files. It works without a backend API URL; the selected group is saved locally in the Telegram WebView/browser. In Telegram, a user can explicitly send the selected EIOS group ID to the bot so personal schedule commands and reminders use the same group. The bot accepts that action only in the sender's private chat and resolves the ID against active database groups. Set `WEBAPP_URL` to the public HTTPS URL of `miniapp/` to show the launch button in the bot's private-chat keyboard. The example and local HTTP URLs intentionally do not activate that button. Styling uses local/system font fallbacks; Telegram's official Web App bridge is the only external script.

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
- `/warn` and `/warnings` — moderators can issue and review chat warnings (reply to a participant or use their Telegram ID).
- `/mute 30m reason`, `/unmute`, `/ban`, and `/unban TELEGRAM_ID` — timed restrictions, bans, and restoration; reply to a participant where possible. The bot needs Telegram administrator rights for the corresponding actions.

The bot intentionally does not ingest ordinary group-chat messages yet. Chat-based homework extraction needs explicit group opt-in, a privacy notice, and a student confirmation step before a task is published.

## Current limitations

- Schedule commands depend on the EIOS sync/import process having populated `groups` and `lessons`.
- Reminders are delivered by an in-process polling loop. For production, run a single bot worker (or move scheduling to a distributed job queue) to avoid duplicate polling workers.
- Telegram only permits deletion of eligible bot messages in chats where the bot has permission; deletion is best-effort.
- Configure the bot with a Telegram BotFather token and keep it secret. The RAG service uses `OPENAI_API_KEY`; basic Telegram commands do not require it. Both keys are read from environment variables / `.env`. Do not commit `.env`.

## Data handling status (internal engineering note)

SMART CAMPUS is a separate application from the university Moodle site. The public [Moodle data-retention summary](https://edu.spbftu.ru/admin/tool/dataprivacy/summary.php) describes Moodle's default categories and does not define SMART CAMPUS's purposes or retention periods. Its visible default summary currently says that no retention period is defined. This is not an approved privacy notice or authorization for SMART CAMPUS processing.

The backend schema can persist:

- Telegram numeric IDs and profile fields (username, first/last name, photo URL), plus selected group and settings.
- Per-chat Telegram IDs, cached usernames, SMART CAMPUS roles, moderation reasons, temporary mute permissions, and role/moderation audit actor/target IDs. The saved pre-mute permissions are removed after unmute or mute expiry; moderation audit entries currently remain.
- Message IDs needed by `/clear` (at most the latest 30 tracked replies per sender and chat).
- Feature data tied to accounts, including task progress, team applications, queue entries, help posts, and Moodle-deadline records when those features are used.

The bot does not ingest ordinary group messages. A self-service account export/deletion flow and an application retention schedule are not implemented. Role audit data and account-linked feature records currently have no application-level expiry. Before production rollout, the university/service owner should approve the purposes, retention periods, deletion behavior (including audit records and backups), and a user-facing notice. Do not copy the Moodle summary into a SMART CAMPUS notice as if it applied to this backend.
