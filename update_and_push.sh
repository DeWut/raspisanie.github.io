#!/data/data/com.termux/files/usr/bin/bash
set -e
cd "$(dirname "$0")"
python update_schedule.py "$@"
git add data
git diff --cached --quiet || { git commit -m "Обновление расписания"; git push; }
