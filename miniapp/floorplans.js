/* Original vector schematics for campus building 2.
 * Geometry is redrawn from the user's reference photos; it is schematic, not a survey drawing. */
(function () {
  'use strict';
  function esc(s) { return String(s == null ? '' : s).replace(/[&<>"']/g, function (c) { return ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]); }); }

  function roomRect(x, y, w, h, label, edge) {
    return '<rect class="fp-room" x="' + x + '" y="' + y + '" width="' + w + '" height="' + h + '"/>';
  }
  function horizontalWing(x, y, w, h, count, name) {
    var corridorH = Math.max(26, Math.min(34, h * 0.25));
    var topH = (h - corridorH) / 2, cellW = w / count, out = '';
    out += '<rect class="fp-shell" x="' + x + '" y="' + y + '" width="' + w + '" height="' + h + '"/>';
    out += '<rect class="fp-corridor" x="' + x + '" y="' + (y + topH) + '" width="' + w + '" height="' + corridorH + '"/>';
    for (var i = 0; i < count; i++) {
      out += roomRect(x + i * cellW, y, cellW, topH, i % 2 === 0 ? 'помещение' : '');
      out += roomRect(x + i * cellW, y + topH + corridorH, cellW, topH, i % 2 === 1 ? 'помещение' : '');
    }
    out += '<text class="fp-wing-label" x="' + (x + w / 2) + '" y="' + (y + h + 17) + '">' + esc(name || 'крыло') + '</text>';
    return out;
  }
  function verticalWing(x, y, w, h, count, name, cap) {
    var corridorW = Math.max(26, Math.min(34, w * 0.24));
    var sideW = (w - corridorW) / 2, cellH = h / count, out = '';
    out += '<rect class="fp-shell" x="' + x + '" y="' + y + '" width="' + w + '" height="' + h + '"/>';
    out += '<rect class="fp-corridor" x="' + (x + sideW) + '" y="' + y + '" width="' + corridorW + '" height="' + h + '"/>';
    for (var i = 0; i < count; i++) {
      out += roomRect(x, y + i * cellH, sideW, cellH, i % 2 === 0 ? 'помещение' : '');
      out += roomRect(x + sideW + corridorW, y + i * cellH, sideW, cellH, i % 2 === 1 ? 'помещение' : '');
    }
    if (cap === 'rounded') {
      out += '<path class="fp-shell-cap" d="M ' + x + ' ' + y + ' Q ' + (x + w / 2) + ' ' + (y - 48) + ' ' + (x + w) + ' ' + y + '"/>';
    }
    out += '<text class="fp-wing-label" x="' + (x + w / 2) + '" y="' + (y + h + 17) + '">' + esc(name || 'крыло') + '</text>';
    return out;
  }
  function connector(x, y, w, h) {
    return '<rect class="fp-connector" x="' + x + '" y="' + y + '" width="' + w + '" height="' + h + '"/>';
  }
  function stair(x, y, w, h, label) {
    var out = '<rect class="fp-stair" x="' + x + '" y="' + y + '" width="' + w + '" height="' + h + '"/>';
    var count = Math.max(3, Math.floor((w > h ? w : h) / 12));
    for (var i = 1; i < count; i++) {
      if (w > h) out += '<path class="fp-step" d="M ' + (x + i * w / count) + ' ' + y + ' v ' + h + '"/>';
      else out += '<path class="fp-step" d="M ' + x + ' ' + (y + i * h / count) + ' h ' + w + '"/>';
    }
    out += '<text class="fp-stair-label" x="' + (x + w / 2) + '" y="' + (y + h / 2 + 3) + '">' + esc(label || 'ЛЕСТНИЦА') + '</text>';
    return out;
  }
  function createSvg(floor, cfg) {
    var out = '<svg class="floorplan-svg" viewBox="0 0 1000 760" role="img" aria-label="Схематический план второго корпуса, ' + floor + '-й этаж">';
    out += '<rect width="1000" height="760" fill="#fff"/>';
    out += '<text class="fp-heading" x="40" y="42">ПЛАН ' + floor + '-ГО ЭТАЖА</text>';
    out += '<text class="fp-subheading" x="40" y="66">КОРПУС № 2 · ИНСТИТУТСКИЙ ПЕРЕУЛОК, 5, ЛИТЕРА Б</text>';
    cfg.connectors.forEach(function (c) { out += connector(c[0],c[1],c[2],c[3]); });
    cfg.horizontal.forEach(function (w) { out += horizontalWing(w[0],w[1],w[2],w[3],w[4],w[5]); });
    cfg.vertical.forEach(function (w) { out += verticalWing(w[0],w[1],w[2],w[3],w[4],w[5],w[6]); });
    cfg.stairs.forEach(function (s) { out += stair(s[0],s[1],s[2],s[3],s[4]); });
    out += '<g class="fp-north"><path d="M 925 92 v 42 m -12 -12 12 12 12 -12"/><text x="925" y="82">СЕВЕР</text></g>';
    out += '<g class="fp-title-block"><path d="M40 670H960 M40 670V735H960V670 M650 670V735 M810 670V735"/><text x="55" y="695">СПРАВОЧНЫЙ ПЛАН ЭТАЖА</text><text x="55" y="718">Кабинеты и навигация · схема по фото</text><text x="670" y="695">КОРПУС № 2</text><text x="670" y="718">ЭТАЖ '+floor+'</text><text x="830" y="695">МАСШТАБ</text><text x="830" y="718">НЕ ЗАДАН</text></g>';
    out += '</svg>';
    return out;
  }

  var floors = {
    1: {
      title: 'Первый этаж',
      shape: 'Нижний поперечный коридор связывает западное крыло, два вытянутых верхних крыла и два южных ответвления.',
      connectors: [[390,305,95,70],[700,305,90,70],[245,370,145,75],[760,370,90,75],[95,380,90,90]],
      horizontal: [[75,345,815,110,10,'центральный коридор'],[40,370,125,100,2,'западная часть'],[220,95,170,120,4,'северо-западное крыло']],
      vertical: [[390,75,145,270,5,'северное крыло'],[690,125,145,220,4,'восточное крыло','rounded'],[245,440,145,205,4,'юго-западное крыло'],[755,440,145,205,4,'юго-восточное крыло']],
      stairs: [[105,365,38,48,'ЛЕСТНИЦА'],[285,590,55,42,'ЛЕСТНИЦА'],[800,590,55,42,'ЛЕСТНИЦА']]
    },
    2: {
      title: 'Второй этаж',
      shape: 'Поперечная галерея соединяет два северных продольных крыла; с южной стороны отходят два крыла с лестничными узлами.',
      connectors: [[205,285,110,70],[540,285,110,70],[275,365,100,70],[545,365,100,70],[110,345,100,65]],
      horizontal: [[100,325,790,105,10,'центральная галерея'],[65,340,150,95,3,'западное крыло']],
      vertical: [[165,85,145,245,5,'северо-западное крыло'],[520,85,145,245,5,'северо-восточное крыло'],[275,430,145,220,4,'юго-западное крыло'],[525,430,145,185,4,'южное крыло']],
      stairs: [[180,95,38,45,'ЛЕСТНИЦА'],[570,95,42,45,'ЛЕСТНИЦА'],[300,600,58,38,'ЛЕСТНИЦА'],[610,555,45,40,'ЛЕСТНИЦА']]
    },
    3: {
      title: 'Третий этаж',
      shape: 'Разветвлённая конфигурация с длинным центральным коридором, северным левым крылом и восточным крылом с закруглённым окончанием.',
      connectors: [[405,290,100,75],[700,300,95,70],[245,370,150,75],[760,370,95,75],[110,370,95,75]],
      horizontal: [[80,340,810,110,10,'центральный коридор'],[205,105,200,115,4,'северо-западное крыло'],[45,370,155,105,3,'западное крыло']],
      vertical: [[405,80,145,265,5,'северное крыло'],[700,125,145,220,4,'восточное крыло','rounded'],[250,440,145,210,4,'юго-западное крыло'],[765,440,145,210,4,'юго-восточное крыло']],
      stairs: [[95,390,42,48,'ЛЕСТНИЦА'],[285,590,58,42,'ЛЕСТНИЦА'],[800,590,58,42,'ЛЕСТНИЦА']]
    },
    4: {
      title: 'Четвёртый этаж',
      shape: 'Сохраняется центральная поперечная связь, северо-западное и северное крылья, восточная дугообразная часть и южное ответвление.',
      connectors: [[405,285,100,75],[710,305,95,65],[250,370,145,75],[780,370,95,75],[115,370,95,75]],
      horizontal: [[80,340,820,110,10,'центральный коридор'],[205,105,200,115,4,'северо-западное крыло'],[45,370,155,105,3,'западное крыло']],
      vertical: [[405,80,145,265,5,'северное крыло'],[710,135,145,210,4,'восточное крыло','rounded'],[255,440,145,205,4,'юго-западное крыло'],[780,440,130,205,4,'юго-восточное крыло']],
      stairs: [[95,390,42,48,'ЛЕСТНИЦА'],[290,590,58,42,'ЛЕСТНИЦА'],[805,590,58,42,'ЛЕСТНИЦА']]
    }
  };
  Object.keys(floors).forEach(function (floor) { floors[floor].svg = createSvg(floor, floors[floor]); });
  window.CAMPUS_FLOORPLANS = { '2': { floors: floors, note: 'Авторская схематическая реконструкция по предоставленным фотографиям. Пропорции и внутреннее деление помещений ориентировочные; не является техническим или эвакуационным планом.' } };
})();