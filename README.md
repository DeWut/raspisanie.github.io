# SPBFTUIT — расписание и навигация

Расписание и навигация SPBFTUIT для СПбГЛТУ: парсер собирает группы с eios.spbftu.ru в `data/`, `index.html` показывает любую.

## Termux
    pkg install python git -y && pip install -r requirements.txt
    python update_schedule.py --dump 19244   # проверить, что парсер видит занятия
    ./update_and_push.sh                      # собрать все группы и запушить

## Если парсер что-то не нашёл
Запусти с `--dump` и пришли файлы из `dump/` (start.html и 19244.html) — подстрою под реальную разметку.
Группы можно вписать в `data/groups.json` вручную или передать ID аргументами.

## GitHub Actions
`.github/workflows/update.yml` обновляет данные раз в сутки (если сайт вуза пускает серверы GitHub; иначе — Termux).
