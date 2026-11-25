# raspisanie

Интерактивное расписание СПбГЛТУ: парсер собирает группы с eios.spbftu.ru в `data/`, `index.html` показывает любую.

## Termux
    pkg install python git -y && pip install -r requirements.txt
    python update_schedule.py --dump 19244   # проверить, что парсер видит занятия
    ./update_and_push.sh                      # собрать все группы и запушить

## GitHub Actions
`.github/workflows/update.yml` обновляет данные раз в сутки
