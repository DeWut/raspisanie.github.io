/* Procedural interior model for the main teaching building.
 * Geometry is redrawn as vector rooms/corridors from the supplied paper plans;
 * no photographs are used at runtime. Dimensions are schematic, not surveyed. */
(function (root) {
  'use strict';
  var FLOOR_DATA = {
    1: { label:'1 этаж', rooms:['1','2','2а','3','4,5,6','7,8,9','10','13','14','16','17','18','19','20','21','22','23','24','25','26','27','28','29','31','32','33','34','35','36','39','40','41','42','43А','43Б','44','45','46','47','49','50','51','52','54','55','56','57','58','59','60','61','62','63','64','65','69','70','71','72','73','74','75','76','77','93','94','95','96','97','98','99','100','101','102'] },
    2: { label:'2 этаж', rooms:['201','203','204','205','206','206А','208','209','210','211','212','213','214','215','216','217','218','219','220','221','222','223','226','227','228','229','230','231','232','233','238','239','242','243','245','246','247','248','250','251','252','253','254','256','264','265','277'] },
    3: { label:'3 этаж', rooms:['307','308','309','310','311','312','313','314','315','316','317','318','319','320','321','322','323','324','327','328','329','330','331','333','334','335','336','337','338','339','340','341','342','343','344','345','346','346а','347','348'] },
    4: { label:'4 этаж', rooms:['401','402','404','405','406','407','408','409','410','411','411а','412','413','414','415','417','418','419','420','421','422','423','424','425','426','427','428','428а','429','430','431','431а','432','433','434','435','435а','436','437','438','439'] }
  };
  function roomLayout(floor) {
    var ids = FLOOR_DATA[floor].rooms, cells = [], i = 0;
    var topCount = Math.ceil(ids.length * 0.36);
    var bottomCount = Math.ceil(ids.length * 0.36);
    var sideCount = Math.ceil((ids.length - topCount - bottomCount) / 2);
    function row(count, x, y, dx, w, h) {
      for (var n=0;n<count && i<ids.length;n++,i++) cells.push({id:ids[i],x:x+n*dx,y:y,w:w,h:h});
    }
    function side(count, x, y, dy, w, h) {
      for (var n=0;n<count && i<ids.length;n++,i++) cells.push({id:ids[i],x:x,y:y+n*dy,w:w,h:h});
    }
    // The supplied plans show a perimeter/courtyard arrangement: long north and south wings,
    // joined by narrower east and west wings. This keeps the model's topology recognizable.
    var topW = 720 / topCount, bottomW = 720 / bottomCount;
    row(topCount, 140, 55, topW, topW-3, 72);
    row(bottomCount, 140, 505, bottomW, bottomW-3, 72);
    side(sideCount, 42, 145, 54, 82, 51);
    side(ids.length-i, 876, 145, 54, 82, 51);
    return cells;
  }
  function draw(canvas, floor, mode) {
    var ctx=canvas.getContext('2d'), w=canvas.width=1000, h=canvas.height=660;
    ctx.clearRect(0,0,w,h);
    var rooms=roomLayout(floor), z=mode==='3d'?38:0, iso=mode==='3d';
    function project(x,y,zz) { return iso ? [500+(x-y)*.58,95+(x+y)*.29-zz] : [x,y+25]; }
    function poly(points,fill,stroke) {
      ctx.beginPath(); points.forEach(function(p,i){var q=project(p[0],p[1],p[2]||0); if(!i)ctx.moveTo(q[0],q[1]);else ctx.lineTo(q[0],q[1]);});
      ctx.closePath(); ctx.fillStyle=fill; ctx.fill(); ctx.strokeStyle=stroke||'#35434b'; ctx.lineWidth=2; ctx.stroke();
    }
    // Main green circulation spine and cross-corridors.
    var corridors=[[120,135,760,28],[120,470,760,28],[115,135,28,363],[850,135,28,363]];
    corridors.forEach(function(c){var x=c[0],y=c[1],cw=c[2],ch=c[3];poly([[x,y,0],[x+cw,y,0],[x+cw,y+ch,0],[x,y+ch,0]],'#d9e8d0','#8aa17d');});
    rooms.forEach(function(r){
      if(iso){
        poly([[r.x,r.y,0],[r.x+r.w,r.y,0],[r.x+r.w,r.y+r.h,0],[r.x,r.y+r.h,0]],'#f7f5ef','#39454d');
        poly([[r.x,r.y,0],[r.x+r.w,r.y,0],[r.x+r.w,r.y,z],[r.x,r.y,z]],'#d7dce0','#39454d');
        poly([[r.x+r.w,r.y,0],[r.x+r.w,r.y+r.h,0],[r.x+r.w,r.y+r.h,z],[r.x+r.w,r.y,z]],'#b9c3ca','#39454d');
        var p=project(r.x+r.w/2,r.y+r.h/2,1);ctx.fillStyle='#1e2933';ctx.font='bold 13px system-ui';ctx.textAlign='center';ctx.fillText(r.id,p[0],p[1]);
      } else {
        var p=project(r.x,r.y,0);ctx.fillStyle='#fff';ctx.fillRect(p[0],p[1],r.w,r.h);ctx.strokeStyle='#303c44';ctx.lineWidth=2;ctx.strokeRect(p[0],p[1],r.w,r.h);
        ctx.fillStyle='#26323a';ctx.font='bold 14px system-ui';ctx.textAlign='center';ctx.fillText(r.id,p[0]+r.w/2,p[1]+r.h/2+5);
      }
    });
    ctx.textAlign='left';ctx.fillStyle='#50606b';ctx.font='13px system-ui';ctx.fillText('Схематическая векторная реконструкция · этаж '+floor,24,635);
  }
  root.InteriorModel={floors:FLOOR_DATA,draw:draw};
})(window);
