#!/usr/bin/env node
// Static server for the Asset & Style Studio live preview (see PREVIEW.md).
// Serves the folder named in <root>/current.txt on ONE fixed localhost port.
// The refresh script only rewrites current.txt after a good build, so the page
// never changes under a visitor halfway through a build. Plain node, no packages.
//
//   node tools/studio/preview-server.mjs [--root <preview folder>] [--port 8123]

import http from 'node:http'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

const arg = (name, dflt) => {
  const i = process.argv.indexOf(name)
  return i > 0 && process.argv[i + 1] ? process.argv[i + 1] : dflt
}
const ROOT = path.resolve(arg('--root', process.env.STUDIO_PREVIEW_ROOT || path.join(os.tmpdir(), 'evani-studio-preview')))
const PORT = Number(arg('--port', process.env.STUDIO_PREVIEW_PORT || '8123'))

const LOG = arg('--log', '')
if (LOG) {
  // Started without a console (WMI): write the log ourselves.
  for (const k of ['log', 'error']) console[k] = (...a) => { try { fs.appendFileSync(LOG, a.join(' ') + '\n') } catch {} }
}

const MIME = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8', '.json': 'application/json; charset=utf-8', '.wasm': 'application/wasm',
  '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.webp': 'image/webp', '.svg': 'image/svg+xml',
  '.glb': 'model/gltf-binary', '.gltf': 'model/gltf+json', '.bin': 'application/octet-stream', '.ktx2': 'image/ktx2',
  '.txt': 'text/plain; charset=utf-8', '.md': 'text/plain; charset=utf-8', '.woff2': 'font/woff2', '.ttf': 'font/ttf',
  '.ico': 'image/x-icon', '.mp3': 'audio/mpeg', '.ogg': 'audio/ogg', '.wav': 'audio/wav',
}

function siteDir() {
  try {
    const name = fs.readFileSync(path.join(ROOT, 'current.txt'), 'utf8').trim()
    if (!name || /[\\/]|\.\./.test(name)) return null
    const dir = path.join(ROOT, 'sites', name)
    return fs.existsSync(dir) ? dir : null
  } catch {
    return null
  }
}

const server = http.createServer((req, res) => {
  const send = (code, body, type = 'text/plain; charset=utf-8') => {
    res.writeHead(code, { 'Content-Type': type, 'Cache-Control': 'no-store' })
    res.end(body)
  }
  let urlPath
  try {
    urlPath = decodeURIComponent(new URL(req.url, 'http://x').pathname)
  } catch {
    return send(400, 'bad url')
  }
  if (urlPath === '/__health') {
    const dir = siteDir()
    return send(200, JSON.stringify({ ok: true, pid: process.pid, port: PORT, site: dir ? path.basename(dir) : null }), 'application/json')
  }
  if (urlPath === '/favicon.ico') {
    res.writeHead(204, { 'Cache-Control': 'no-store' })
    return res.end()
  }
  const dir = siteDir()
  if (!dir) return send(503, 'The studio preview has no good build yet. Run: node tools/studio/preview-refresh.mjs')
  if (req.method !== 'GET' && req.method !== 'HEAD') return send(405, 'GET only')
  if (urlPath.endsWith('/')) urlPath += 'index.html'
  const file = path.join(dir, urlPath)
  if (file !== dir && !file.startsWith(dir + path.sep)) return send(403, 'no')
  fs.stat(file, (err, st) => {
    if (err || !st.isFile()) return send(404, 'not found: ' + urlPath)
    res.writeHead(200, {
      'Content-Type': MIME[path.extname(file).toLowerCase()] || 'application/octet-stream',
      'Content-Length': st.size,
      'Cache-Control': 'no-store',
    })
    if (req.method === 'HEAD') return res.end()
    fs.createReadStream(file).on('error', () => res.destroy()).pipe(res)
  })
})
server.on('error', e => {
  console.error('preview server:', e.message)
  process.exit(1)
})
server.listen(PORT, '127.0.0.1', () => console.log(`studio preview server: http://localhost:${PORT}/ (root ${ROOT}, pid ${process.pid})`))
