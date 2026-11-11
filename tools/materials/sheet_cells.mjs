// Contact-sheet cells for the material viewer captures (used by sheet.mjs).
// A row the viewer cannot draw (status `planned`, a held row) has no picture to shoot, and the viewer cannot
// select it: shooting it would capture whatever row was open before it. So a held row gets a "held: <reason>"
// tile instead, with the reason the viewer shows (data/planned.json), and no picture at all.

/** True when the viewer can open and draw the row (same rule as the viewer's `row_ready`). */
export const isDrawn = (row) => row.status === 'ready' || row.status === 'reviewed' || row.status === 'alias';

/** The reason the viewer gives for a row it does not draw: its own line, then its group's, then the generic ones. */
export function heldReason(row, planned) {
  const own = planned.rows?.[row.id];
  if (own) return own;
  const group = (planned.groups ?? []).find((g) => g.ids.includes(row.id));
  if (group) return group.reason;
  const backend = planned.backend?.[row.recipe];
  if (backend) return backend;
  if (row.batch != null && planned.batch) return planned.batch.replace('{batch}', String(row.batch));
  return planned.default ?? 'Not drawn yet.';
}

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

/**
 * The cells of one tier's sheet, in row order. A drawn row gives its picture (and, with `close`, its close-up);
 * a held row gives ONE tile that says why it is held, never a picture.
 * Each cell: { id, src?, caption, held?: reason, wide?: true }.
 */
export function sheetCells(rows, tier, planned, close) {
  return rows.flatMap((r) => {
    if (!isDrawn(r)) return [{ id: r.id, caption: r.id, held: heldReason(r, planned), wide: close }];
    const cells = [{ id: r.id, src: `/_s/${tier}/${r.id}.png`, caption: r.id }];
    if (close) cells.push({ id: r.id, src: `/_s/${tier}/${r.id}.close.png`, caption: r.id + ' close' });
    return cells;
  });
}

/** One cell as html. */
export function cellHtml(c) {
  if (c.held !== undefined) {
    return `<figure class="held" style="margin:0;background:#3a2a2a;padding:4px;${c.wide ? 'grid-column:span 2;' : ''}display:flex;flex-direction:column;justify-content:center;min-height:120px"><div style="font-size:13px;font-weight:bold;color:#ffb0a0">held: ${esc(c.held)}</div><figcaption style="padding:2px 0;font-size:11px">${esc(c.caption)}</figcaption></figure>`;
  }
  return `<figure style="margin:0;background:#2a313a;padding:4px"><img src="${c.src}" style="width:100%;display:block;background:#d2cdc6"><figcaption style="padding:2px 0;font-size:11px">${esc(c.caption)}</figcaption></figure>`;
}
