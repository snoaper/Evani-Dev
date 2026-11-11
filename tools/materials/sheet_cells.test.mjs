// node --test tools/materials/   (no browser, no build)
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { cellHtml, heldReason, isDrawn, sheetCells } from './sheet_cells.mjs';

const root = new URL('../../', import.meta.url);
const catalogue = JSON.parse(readFileSync(new URL('catalog/materials/materials.json', root), 'utf8'));
const planned = JSON.parse(readFileSync(new URL('kits/evani-kit-material-view/data/planned.json', root), 'utf8'));

test('a held row gets a held tile with its reason and no picture, in its own place', () => {
  const rows = ['paper.parchment', 'paper.map', 'cloth.chiffon'].map((id) => catalogue.rows.find((r) => r.id === id));
  for (const close of [false, true]) {
    const cells = sheetCells(rows, 'low', planned, close);
    const held = cells.filter((c) => c.held !== undefined);
    assert.deepEqual(held.map((c) => c.id), ['paper.map', 'cloth.chiffon']);
    for (const c of held) {
      assert.equal(c.src, undefined, `${c.id}: a held row must not show a picture (the previous row's was shown before)`);
      assert.match(cellHtml(c), new RegExp(`held: .+`));
      assert.ok(!cellHtml(c).includes('<img'), c.id);
    }
    assert.equal(held[0].held, planned.rows['paper.map']);
    // The drawn row keeps its own picture(s); the sheet has one tile per held row and two per drawn row with close-ups.
    assert.equal(cells.length, close ? 2 + 1 + 1 : 3);
    assert.deepEqual(cells.filter((c) => c.src).map((c) => c.src.split('/').pop()), close ? ['paper.parchment.png', 'paper.parchment.close.png'] : ['paper.parchment.png']);
  }
});

test('every row of the catalogue is either drawn with its own picture or held with a reason', () => {
  const cells = sheetCells(catalogue.rows, 'ultra', planned, true);
  for (const c of cells) {
    if (c.held === undefined) assert.ok(c.src.includes(`/${c.id}.`), `${c.id}: picture of another row`);
    else assert.ok(c.held.length > 3, c.id);
  }
  assert.equal(cells.filter((c) => c.held !== undefined).length, catalogue.rows.filter((r) => !isDrawn(r)).length);
});

test('a held row with numbers kept gives its own reason, not the generic line', () => {
  const generic = new Set([planned.default, ...Array.from({ length: 11 }, (_, i) => planned.batch.replace('{batch}', String(i + 1))), ...Object.values(planned.backend)]);
  for (const r of catalogue.rows.filter((x) => !isDrawn(x) && x.params?.numbers)) {
    assert.ok(!generic.has(heldReason(r, planned)), `${r.id} shows a generic reason`);
  }
});
