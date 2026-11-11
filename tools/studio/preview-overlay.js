// Studio preview: a small "Studio preview" badge, a Library panel and a "what's new" bell,
// added to the served page by tools/studio/preview-refresh.mjs. It is NOT part of the engine
// or the studio. Files next to the page: studio-preview-data.json (the whole material library
// and asset catalogue of the commit this page was built from) and whats-new.json (the
// cumulative list of things that became built, newest first; polled about once a minute).
//
// Hook for the studio page: clicking a bell entry fires window event 'studio-preview-select'
// with detail {kind, id} (kind: material | asset | style | lighting | feature). The page may
// listen and select that item. Until it does, a material is also opened in the viewer's own
// library list when that list is on screen.
(async () => {
  const fetchJson = async name => (await fetch('./' + name + '?t=' + Date.now(), { cache: 'no-store' })).json()
  let d
  try {
    d = await fetchJson('studio-preview-data.json')
  } catch (e) {
    console.warn('studio preview: no data', e)
    return
  }
  const host = document.createElement('div')
  host.id = 'studio-preview-library'
  host.style.cssText = 'position:fixed;right:8px;bottom:8px;z-index:2147483000;font:12px/1.35 system-ui,Segoe UI,sans-serif;max-width:calc(100vw - 16px)'
  document.body.appendChild(host)
  const root = host.attachShadow({ mode: 'open' })
  const mats = d.materials
  const assets = d.assets
  const mReady = mats.filter(m => m[3] === 'ready').length
  const mPlan = mats.length - mReady
  const aReady = assets.filter(a => a[3] === 'ready').length
  const aPlan = assets.length - aReady
  root.innerHTML = `<div class="wrap"><style>
    *{box-sizing:border-box}
    button,input{font:inherit}
    .wrap{display:flex;flex-direction:column-reverse;align-items:flex-end}
    .bar{display:flex;gap:6px;align-items:stretch;max-width:100%}
    .short{display:none}
    @media (max-width:1700px){.badge span,.badge b.full{display:none}.short{display:inline}}
    .badge{display:flex;gap:8px;align-items:center;min-width:0;flex:0 1 auto;max-width:min(86vw,520px);background:#111c;color:#eee;border:1px solid #fff3;border-radius:8px;padding:5px 9px;cursor:pointer;backdrop-filter:blur(6px);text-align:left}
    .badge b{color:#9fe;flex:none}
    .badge span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0}
    .bell{position:relative;flex:none;background:#111c;color:#eee;border:1px solid #fff3;border-radius:8px;padding:3px 9px;cursor:pointer;backdrop-filter:blur(6px);display:flex;align-items:center}
    .bell svg{width:17px;height:17px;fill:none;stroke:currentColor;stroke-width:1.8;stroke-linecap:round;stroke-linejoin:round}
    .bell.lit{border-color:#fb6;color:#fb6;box-shadow:0 0 10px #fb64}
    .bell i{position:absolute;top:-7px;right:-6px;min-width:17px;height:17px;border-radius:9px;background:#e33;color:#fff;font:700 10px/17px system-ui;text-align:center;padding:0 4px;font-style:normal;display:none}
    .bell.lit i{display:block}
    .panel{margin-bottom:6px;width:min(92vw,440px);max-height:min(72vh,720px);display:none;flex-direction:column;background:#101418ee;color:#e8e8e8;border:1px solid #fff3;border-radius:10px;overflow:hidden;backdrop-filter:blur(8px)}
    .panel.open{display:flex}
    .meta{padding:8px 10px;color:#aab;border-bottom:1px solid #fff2;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
    .tabs{display:flex;gap:4px;padding:6px 8px;flex-wrap:wrap;align-items:center}
    .tabs button{background:#fff1;color:#ddd;border:1px solid #fff2;border-radius:6px;padding:3px 8px;cursor:pointer}
    .tabs button.on{background:#2a6;color:#fff;border-color:#2a6}
    input{flex:1 1 120px;min-width:90px;background:#0006;color:#fff;border:1px solid #fff3;border-radius:6px;padding:3px 7px}
    .list{overflow:auto;padding:2px 8px 8px}
    .row{display:flex;gap:8px;justify-content:space-between;align-items:baseline;padding:3px 2px;border-bottom:1px solid #fff1}
    .row .n{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0}
    .row .c{color:#889;font-size:11px;flex:none;max-width:40%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
    .row.planned .n{color:#9aa;font-style:italic}
    .row.flash{animation:fl 2.4s ease-out}
    @keyframes fl{0%,60%{background:#fb64}100%{background:transparent}}
    .tag{flex:none;font-size:10px;border-radius:4px;padding:0 5px;border:1px solid}
    .tag.ready{color:#7e9;border-color:#7e95}
    .tag.planned,.tag.building{color:#fb6;border-color:#fb65}
    .tag.kind{color:#9cf;border-color:#9cf5}
    .new{display:block;width:100%;text-align:left;background:none;color:inherit;border:0;border-bottom:1px solid #fff1;padding:6px 6px;cursor:pointer;border-radius:0}
    .new:hover{background:#fff1}
    .new.unseen{background:#fb62;border-left:3px solid #fb6}
    .new .l1{display:flex;gap:8px;align-items:baseline;min-width:0}
    .new .l1 .n{flex:1 1 auto;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
    .new .l2{color:#99a;font-size:11px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
    .hint{margin:6px 8px 0;padding:5px 8px;border-radius:6px;background:#2a6;color:#fff;cursor:pointer;display:none}
    .hint.on{display:block}
    .empty{padding:12px 10px;color:#99a}
  </style>
  <div class="bar">
   <button class="badge" id="b" title="The whole material library and asset catalogue of this build"><b class="full">Studio preview</b><b class="short">Preview</b>
    <span id="bs">${mReady} built, ${mPlan} planned materials · ${aReady} ready, ${aPlan} planned assets</span></button>
   <button class="bell" id="bell" aria-label="What's new" title="What's new since you last looked">
    <svg viewBox="0 0 24 24"><path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"/><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/></svg><i id="cnt">0</i></button>
  </div>
  <div class="panel" id="p"><div class="meta" id="meta"></div>
   <div class="tabs"><button data-t="a" class="on">Assets</button><button data-t="m">Materials</button>
    <button data-s="" class="on">All</button><button data-s="ready">Built</button><button data-s="planned">Planned</button>
    <input id="q" placeholder="Search" type="search"></div><div class="list" id="l"></div></div>
  <div class="panel" id="np"><div class="meta">What's new (newest first)</div>
   <div class="hint" id="hint">A new version of the studio is ready. Click to reload.</div>
   <div class="list" id="nl"></div></div></div>`
  const $ = id => root.getElementById(id)
  $('meta').textContent = `${mReady} built, ${mPlan} planned materials · ${aReady} ready, ${aPlan} planned assets · ${d.sha.slice(0, 8)} · ${d.subject} · built ${d.builtAt}`
  $('meta').title = $('meta').textContent
  let tab = 'a'
  let status = ''
  let q = ''
  let flashId = ''
  const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c])
  function draw() {
    const rows = (tab === 'a' ? assets : mats).filter(r => {
      const planned = r[3] !== 'ready'
      if (status === 'ready' && planned) return false
      if (status === 'planned' && !planned) return false
      return !q || (r[0] + ' ' + r[1] + ' ' + r[2]).toLowerCase().includes(q)
    })
    const label = s => (s === 'ready' ? 'built' : s === 'building' ? 'being built' : 'planned · not built yet')
    let html = rows.slice(0, 600).map(r => {
      const planned = r[3] !== 'ready'
      return `<div class="row ${planned ? 'planned' : ''} ${r[0] === flashId ? 'flash' : ''}" data-id="${esc(r[0])}" title="${esc(r[1])} (${esc(r[0])})"><span class="n">${esc(r[1])}</span><span class="c">${esc(r[2])}</span><span class="tag ${r[3]}">${label(r[3])}</span></div>`
    }).join('')
    if (rows.length > 600) html += `<div class="row"><span class="n">… ${rows.length - 600} more: search to narrow</span></div>`
    $('l').innerHTML = html || '<div class="row"><span class="n">Nothing matches.</span></div>'
    const f = $('l').querySelector('.flash')
    if (f) f.scrollIntoView({ block: 'center' })
  }
  const setOn = (sel, el) => root.querySelectorAll(sel).forEach(x => x.classList.toggle('on', x === el))
  function showLib(open) {
    $('p').classList.toggle('open', open)
    if (open) $('np').classList.remove('open')
    if (open) draw()
  }
  $('b').onclick = () => showLib(!$('p').classList.contains('open'))
  root.querySelectorAll('[data-t]').forEach(b => {
    b.onclick = () => {
      tab = b.dataset.t
      setOn('[data-t]', b)
      flashId = ''
      draw()
    }
  })
  root.querySelectorAll('[data-s]').forEach(b => {
    b.onclick = () => {
      status = b.dataset.s
      setOn('[data-s]', b)
      draw()
    }
  })
  $('q').oninput = e => {
    q = e.target.value.trim().toLowerCase()
    flashId = ''
    draw()
  }
  for (const ev of ['keydown', 'keyup', 'keypress']) $('q').addEventListener(ev, e => e.stopPropagation())

  // ---- the bell -------------------------------------------------------------------------
  const SEEN = 'studio-preview-last-seen'
  const lastSeen = () => localStorage.getItem(SEEN) || ''
  let news = []
  let seenAtOpen = ''
  let latestSha = d.sha
  const unseen = e => (e.landedAt || e.addedAt || '') > (seenAtOpen || lastSeen())
  function drawBell() {
    const n = news.filter(unseen).length
    $('bell').classList.toggle('lit', n > 0)
    $('cnt').textContent = n > 99 ? '99+' : String(n)
    $('bell').title = n ? `${n} new since you last looked` : "What's new (nothing new since you last looked)"
    $('hint').classList.toggle('on', latestSha !== d.sha)
    $('nl').innerHTML = news.length
      ? news.map((e, i) => {
        const when = new Date(e.addedAt).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })
        return `<button class="new ${unseen(e) ? 'unseen' : ''}" data-i="${i}" title="${esc(e.name)} — ${esc(e.title)} (${when})">
          <div class="l1"><span class="n">${esc(e.name)}</span><span class="tag kind">${esc(e.kind)}</span></div>
          <div class="l2">${when} · ${esc(e.title)}</div></button>`
      }).join('')
      : '<div class="empty">Nothing new has landed yet. New materials, assets, styles and features appear here as they are merged.</div>'
  }
  async function pollNews() {
    try {
      const j = await fetchJson('whats-new.json')
      news = j.entries || []
      latestSha = j.sha || latestSha
    } catch (e) {
      console.warn('studio preview: whats-new.json', e)
    }
    drawBell()
  }
  $('bell').onclick = () => {
    const open = !$('np').classList.contains('open')
    $('np').classList.toggle('open', open)
    if (open) {
      $('p').classList.remove('open')
      seenAtOpen = lastSeen() // keep this session's unseen highlighted while the list is open
      drawBell()
      localStorage.setItem(SEEN, new Date().toISOString())
    } else {
      seenAtOpen = ''
      drawBell()
    }
  }
  $('hint').onclick = () => location.reload()
  $('nl').onclick = ev => {
    const el = ev.target.closest('[data-i]')
    if (el) goTo(news[Number(el.dataset.i)])
  }
  function goTo(e) {
    window.dispatchEvent(new CustomEvent('studio-preview-select', { detail: { kind: e.kind, id: e.id } }))
    if (e.kind === 'material' || e.kind === 'asset') {
      tab = e.kind === 'asset' ? 'a' : 'm'
      status = ''
      q = ''
      flashId = e.id
      $('q').value = ''
      root.querySelectorAll('[data-t]').forEach(b => b.classList.toggle('on', b.dataset.t === tab))
      setOn('[data-s]', root.querySelector('[data-s=""]'))
      showLib(true)
    }
    if (e.kind === 'material') openInViewer(e)
  }
  // The viewer's own library list (legacy viewer): search for the material and open its row.
  function openInViewer(e) {
    const search = document.getElementById('mv-cat-q')
    if (search) {
      search.value = e.name
      search.dispatchEvent(new Event('input', { bubbles: true }))
    }
    let tries = 0
    const t = setInterval(() => {
      const btn = document.querySelector(`[data-cat-open="${CSS.escape(e.id)}"]`)
      if (btn) {
        btn.scrollIntoView({ block: 'center' })
        btn.click()
        clearInterval(t)
      } else if (++tries > 12) clearInterval(t)
    }, 150)
  }
  await pollNews()
  setInterval(pollNews, 60000)
  document.addEventListener('visibilitychange', () => document.visibilityState === 'visible' && pollNews())
  window.__studioPreview = { materials: { built: mReady, planned: mPlan }, assets: { ready: aReady, planned: aPlan }, sha: d.sha, refresh: pollNews }
})()
