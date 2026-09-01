import requests
from bs4 import BeautifulSoup
import json
import re

URL = "https://eios.spbftu.ru/Rasp/Rasp.aspx?group=19244&sem=1"
OUTPUT_FILE = "schedule.json"

def parse_time(cell_text):
    # Пример: "1 пара 09:15<br />10:50"
    # Убираем лишние пробелы, извлекаем номер пары и время
    cell_text = cell_text.replace('\n', ' ').replace('\r', ' ')
    cell_text = re.sub(r'\s+', ' ', cell_text).strip()
    # Паттерн: "N пара HH:MM <br/> HH:MM"
    m = re.search(r'(\d+)\s*пара\s*(\d{1,2}):(\d{2})\s*<br\s*/?>\s*(\d{1,2}):(\d{2})', cell_text, re.IGNORECASE)
    if m:
        num = int(m.group(1))
        start_h = int(m.group(2))
        start_m = int(m.group(3))
        end_h = int(m.group(4))
        end_m = int(m.group(5))
        return f"{num} • {start_h:02d}:{start_m:02d}–{end_h:02d}:{end_m:02d}"
    # На случай если время уже в нужном формате (на всякий случай)
    if '•' in cell_text:
        return cell_text.strip()
    return cell_text.strip()

def parse_teacher_room(cell_text):
    # Ожидается "Преподаватель<br />ауд. X"
    parts = cell_text.split('<br />')
    teacher = parts[0].strip()
    room = parts[1].strip() if len(parts) > 1 else ''
    return teacher, room

def parse_schedule():
    response = requests.get(URL)
    response.raise_for_status()
    soup = BeautifulSoup(response.text, 'html.parser')

    # Находим основную таблицу с расписанием
    table = soup.find('table', {'id': 'ctl00_MainContent_ASPxGridView1'})
    if not table:
        raise Exception("Не удалось найти таблицу расписания")

    schedule = {}
    current_day = None
    current_time = None
    # Строки таблицы
    rows = table.find_all('tr')
    for row in rows:
        classes = row.get('class', [])
        # Групповая строка дня недели
        if 'dxgvGroupRow_MaterialCompact' in classes:
            # Внутри есть td с названием дня
            day_td = row.find('td', colspan=True)
            if day_td:
                day_name = day_td.get_text(strip=True)
                if day_name in ['Понедельник', 'Вторник', 'Среда', 'Четверг', 'Пятница', 'Суббота', 'Воскресенье']:
                    current_day = day_name
                    if current_day not in schedule:
                        schedule[current_day] = []
        # Строки данных
        elif 'dxgvDataRow_MaterialCompact' in classes:
            cells = row.find_all('td')
            # Пропускаем первую ячейку (indent)
            if len(cells) < 4:
                continue
            # Время может быть в первой или второй ячейке из-за rowspan
            time_cell = cells[1] if len(cells) > 1 else None
            subject_cell = cells[2] if len(cells) > 2 else None
            teacher_room_cell = cells[3] if len(cells) > 3 else None
            week_cell = cells[4] if len(cells) > 4 else None

            # Если время не пустое, обновляем current_time
            if time_cell and time_cell.get_text(strip=True).strip():
                current_time = parse_time(str(time_cell))
            # Если время пустое (rowspan), используем текущее

            subject = subject_cell.get_text(strip=True) if subject_cell else ''
            teacher_room_html = str(teacher_room_cell) if teacher_room_cell else ''
            teacher, room = parse_teacher_room(teacher_room_html)
            week = week_cell.get_text(strip=True) if week_cell else ''

            # Нормализуем неделю
            if week in ['чёт', 'нечет', 'нечёт']:
                week = 'чёт' if week == 'чёт' else 'нечёт'
            elif week == '*':
                pass
            else:
                week = '*'

            # Добавляем запись
            if current_day and current_time:
                schedule[current_day].append({
                    "time": current_time,
                    "subject": subject,
                    "teacher": teacher,
                    "room": room,
                    "week": week
                })
    return schedule

if __name__ == '__main__':
    data = parse_schedule()
    with open(OUTPUT_FILE, 'w', encoding='utf-8') as f:
        json.dump(data, f, ensure_ascii=False, indent=2)
    print(f"Расписание сохранено в {OUTPUT_FILE}")
