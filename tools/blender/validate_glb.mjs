import validator from 'gltf-validator';
import fs from 'fs';
const f = process.argv[2];
const r = await validator.validateBytes(new Uint8Array(fs.readFileSync(f)), {maxIssues: 50});
const i = r.issues;
console.log(JSON.stringify({file: f.split(/[\/]/).pop(), validator: validator.version(), errors: i.numErrors, warnings: i.numWarnings, infos: i.numInfos, hints: i.numHints, generator: r.info.generator, materials: r.info.materialCount, textures: r.info.textureCount, drawCalls: r.info.drawCallCount, tris: r.info.totalTriangleCount, msgs: i.messages.slice(0,6).map(m=>m.severity+':'+m.code)}));
