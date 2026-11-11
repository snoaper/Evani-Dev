// The HTML report: the displacement-ratio chart, the frame-time chart and a
// table view, with "Before" (baseline) and "After" (candidate) overlaid.
// Ported from APEX Velocity's chart.mjs (2026-11-11). Self-contained: inline
// SVG, no network, no libraries. Pure: `html(...)` returns a string.

const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

/**
 * `runs`: `[{ key: 'before'|'after', name, legend, series, summary, verdict }]`
 * (one or two). `meta`: `{ title, subtitle, window_s, target_ms }`.
 */
export function html(runs, meta) {
  const data = JSON.stringify(runs.map((r) => ({ key: r.key, name: r.name, ratio: r.series.ratio, ft: r.series.ft }))).replace(/</g, '\\u003c');
  const legend = runs.map((r) => `<span><i class="sw-${r.key}"></i>${esc(r.legend)}</span>`).join('');
  const cell = (v, unit = '') => (v === null || v === undefined || Number.isNaN(v) ? '–' : `${v}${unit}`);
  const rows = runs
    .map((r) => {
      const m = r.summary.motion || {};
      const f = r.summary.frames;
      const v = r.verdict;
      return `<tr><td>${esc(r.name)}</td><td>${cell(m.uneven_pct, '%')}</td><td>${cell(m.ratio_p5)}</td><td>${cell(m.ratio_p95)}</td><td>${cell(m.ratio_sd)}</td><td>${cell(m.ratio_dev_p99)}</td><td>${cell(f.p50_ms, ' ms')}</td><td>${cell(f.p99_ms, ' ms')}</td><td>${cell(f.max_ms, ' ms')}</td><td>${cell(f.hitches)}</td><td>${v.pass ? 'pass' : 'fail'}</td></tr>`;
    })
    .join('');
  const fails = runs
    .filter((r) => !r.verdict.pass)
    .map((r) => `<li>${esc(r.name)}: ${r.verdict.failures.map((x) => `${esc(x.text)} ${esc(x.value)} (limit ${esc(x.limit)})`).join('; ')}</li>`)
    .join('');
  const target = meta.target_ms || 16.7;
  const ftTicks = [0, target, 2 * target, 3 * target].map((v) => Math.round(v * 10) / 10);
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${esc(meta.title)}</title>
<style>
:root{--surface:#fcfcfb;--ink:#0b0b0b;--ink2:#52514e;--grid:#e6e5e1;--s1:#2a78d6;--s2:#eb6834}
@media (prefers-color-scheme:dark){:root{--surface:#1a1a19;--ink:#f0efec;--ink2:#c3c2b7;--grid:#383835;--s1:#3987e5;--s2:#d95926}}
body{margin:0;padding:24px;background:var(--surface);color:var(--ink);font:14px/1.45 system-ui,Segoe UI,sans-serif;max-width:1100px}
h1{font-size:20px;margin:0 0 4px}h2{font-size:15px;margin:22px 0 2px}p{margin:2px 0 8px;color:var(--ink2)}
.legend{display:flex;flex-wrap:wrap;gap:18px;margin:6px 0;color:var(--ink2)}.legend i{display:inline-block;width:14px;height:3px;border-radius:2px;vertical-align:middle;margin-right:6px}
.sw-before{background:var(--s1)}.sw-after{background:var(--s2)}
svg{width:100%;height:auto;display:block;overflow:visible}.tip{position:fixed;pointer-events:none;background:var(--surface);color:var(--ink);border:1px solid var(--grid);border-radius:6px;padding:6px 8px;font-size:12px;box-shadow:0 2px 8px #0002;display:none}
.scroll{overflow-x:auto}table{border-collapse:collapse;margin-top:8px;font-size:13px}td,th{padding:3px 10px;text-align:right;border-bottom:1px solid var(--grid);white-space:nowrap}th:first-child,td:first-child{text-align:left}
details{margin-top:14px;color:var(--ink2)}ul{margin:4px 0 0 18px;padding:0}
</style></head><body>
<h1>${esc(meta.title)}</h1>
<p>${esc(meta.subtitle)}</p>
<div class="legend">${legend}</div>
<h2>How evenly the tracked object moves on screen, frame by frame</h2>
<p>Distance it moves each frame, against the average of its neighbouring frames. Steady motion stays at 1. ${esc(meta.window_s)} seconds of motion.</p>
<svg id="m" viewBox="0 0 1000 300" role="img" aria-label="Per-frame motion steadiness"></svg>
<h2>Frame times</h2>
<p>Time between displayed frames over the whole run (target: ${esc(Math.round(target * 10) / 10)} ms).</p>
<svg id="f" viewBox="0 0 1000 220" role="img" aria-label="Frame times"></svg>
${fails ? `<p><b>Over the limits:</b></p><ul>${fails}</ul>` : ''}
<details${meta.open_table ? ' open' : ''}><summary>Table view</summary><div class="scroll"><table id="tbl"><tr><th></th><th>Uneven frames</th><th>Steadiness p5</th><th>p95</th><th>SD</th><th>Deviation p99</th><th>Frame p50</th><th>p99</th><th>Longest</th><th>Hitches</th><th>Result</th></tr>${rows}</table></div></details>
<div class="tip" id="tip"></div>
<script>
const D=${data};
const css=(v)=>getComputedStyle(document.documentElement).getPropertyValue(v).trim();
const COL={before:'--s1',after:'--s2'};
function chart(id,key,yMax,yTicks,fmt,unit){
  const svg=document.getElementById(id); const W=1000,H=+svg.getAttribute('viewBox').split(' ')[3],L=48,R=110,T=10,Bm=26;
  const xs=D.flatMap(r=>r[key].map(p=>p[0])); const xMax=Math.max(1e-3,...xs);
  const X=v=>L+(W-L-R)*v/xMax, Y=v=>T+(H-T-Bm)*(1-Math.min(v,yMax)/yMax);
  let g='';
  for(const t of yTicks) g+='<line x1="'+L+'" x2="'+(W-R)+'" y1="'+Y(t)+'" y2="'+Y(t)+'" stroke="'+css('--grid')+'" stroke-width="1"/><text x="'+(L-6)+'" y="'+(Y(t)+4)+'" text-anchor="end" font-size="11" fill="'+css('--ink2')+'">'+fmt(t)+'</text>';
  const step=Math.max(1,Math.round(xMax/6));
  for(let s=0;s<=xMax;s+=step) g+='<text x="'+X(s)+'" y="'+(H-6)+'" text-anchor="middle" font-size="11" fill="'+css('--ink2')+'">'+s+' s</text>';
  const path=(pts)=>pts.map((p,i)=>(i?'L':'M')+X(p[0]).toFixed(1)+' '+Y(p[1]).toFixed(1)).join('');
  let prevY=null;
  for(const r of D){
    const pts=r[key]; if(!pts.length) continue;
    g+='<path d="'+path(pts)+'" fill="none" stroke="'+css(COL[r.key])+'" stroke-width="2" stroke-linejoin="round"/>';
    const last=pts[pts.length-1]; let y=Y(last[1])+4; if(prevY!==null&&Math.abs(y-prevY)<14) y=prevY+14; prevY=y;
    g+='<text x="'+(W-R+8)+'" y="'+y+'" font-size="12" fill="'+css('--ink')+'">'+r.name+'</text>';
  }
  g+='<line id="'+id+'x" y1="'+T+'" y2="'+(H-Bm)+'" stroke="'+css('--ink2')+'" stroke-width="1" visibility="hidden"/>';
  g+='<rect x="'+L+'" y="'+T+'" width="'+(W-L-R)+'" height="'+(H-T-Bm)+'" fill="transparent" id="'+id+'h"/>';
  svg.innerHTML=g;
  const tip=document.getElementById('tip'), hit=document.getElementById(id+'h'), cross=document.getElementById(id+'x');
  const near=(pts,x)=>pts.reduce((b,p)=>Math.abs(p[0]-x)<Math.abs(b[0]-x)?p:b,pts[0]||[0,null]);
  hit.addEventListener('mousemove',e=>{const r=svg.getBoundingClientRect(); const vx=(e.clientX-r.left)/r.width*W; const x=(vx-L)/(W-L-R)*xMax;
    cross.setAttribute('x1',X(x)); cross.setAttribute('x2',X(x)); cross.setAttribute('visibility','visible');
    tip.style.display='block'; tip.style.left=(e.clientX+12)+'px'; tip.style.top=(e.clientY+12)+'px';
    tip.innerHTML=x.toFixed(2)+' s'+D.map(s=>'<br>'+s.name+': '+(near(s[key],x)[1]??'–')+unit).join('');});
  hit.addEventListener('mouseleave',()=>{tip.style.display='none'; cross.setAttribute('visibility','hidden');});
}
chart('m','ratio',2.6,[0,0.5,1,1.5,2,2.5],v=>v.toFixed(1),'');
const fmax=Math.max(${JSON.stringify(ftTicks[3])},...D.flatMap(r=>r.ft.map(p=>p[1])).filter(v=>v<200));
chart('f','ft',Math.ceil(fmax/10)*10,${JSON.stringify(ftTicks)},v=>v.toFixed(1),' ms');
</script></body></html>`;
}
