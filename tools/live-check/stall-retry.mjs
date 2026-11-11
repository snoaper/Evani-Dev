// A stalled capture step gets one retry, then a clear ERROR (2026-10-25: after
// the batch 3-5 join a stalled browser left the material viewer's review card
// and critic packet with 0 pictures; nothing said so loudly).
//
// withStallRetry runs `attempt` again while it fails with a stall (`isStall`)
// for up to `windowMs`. When that window runs out, it calls `reload` once (a
// fresh page) and opens one more window; when the second runs out too, it
// throws `ERROR capture: <name> stalled ...`. Any other error is thrown at once.
//
// Self-test: node tools/live-check/stall-retry.mjs --self-test (run by the
// xtask test `the_stall_retry_self_test_passes`).

/**
 * @param {{ name: string, attempt: () => Promise<any>, isStall: (e: any) => boolean, windowMs: number,
 *           reload?: () => Promise<void>, log?: (m: string) => void, now?: () => number }} o
 */
export async function withStallRetry(o) {
  const now = o.now ?? Date.now;
  const log = o.log ?? (() => {});
  let reloaded = false;
  let until = now() + o.windowMs;
  for (;;) {
    try {
      return await o.attempt();
    } catch (e) {
      if (!o.isStall(e)) throw e;
      if (now() <= until) {
        log(`${o.name}: the page is not answering yet (pipelines still building?); asking again`);
        continue;
      }
      if (o.reload && !reloaded) {
        reloaded = true;
        log(`${o.name}: stalled for ${Math.round(o.windowMs / 1000)} s; reloading the page once and trying again`);
        await o.reload();
        until = now() + o.windowMs;
        continue;
      }
      const why = String(e?.message ?? e);
      throw new Error(`ERROR capture: ${o.name} stalled for ${Math.round(o.windowMs / 1000)} s${reloaded ? ' twice (the page was reloaded once in between)' : ''}; the page never answered. Last error: ${why}`);
    }
  }
}

async function selfTest() {
  const stall = (n) => new Error(`${n}: timeout: no frame`);
  const isStall = (e) => String(e.message).includes(': timeout:');
  let fails = 0;
  const check = (ok, what) => {
    if (!ok) {
      fails++;
      console.error(`FAIL ${what}`);
    }
  };
  // A fake clock: each attempt costs 10 s.
  const clock = () => {
    let t = 0;
    return { now: () => t, tick: () => (t += 10000) };
  };

  // 1. Answers after a few stalls inside the window: no reload.
  {
    const c = clock();
    let n = 0, reloads = 0;
    const r = await withStallRetry({ name: 'capture.frame', windowMs: 60000, now: c.now, isStall, reload: async () => void reloads++, attempt: async () => { c.tick(); if (++n < 3) throw stall('capture.frame'); return 'ok'; } });
    check(r === 'ok' && reloads === 0, 'answers inside the first window without a reload');
  }
  // 2. Stalls through the first window, answers after the one reload.
  {
    const c = clock();
    let reloads = 0;
    const r = await withStallRetry({ name: 'capture.frame', windowMs: 30000, now: c.now, isStall, reload: async () => void reloads++, attempt: async () => { c.tick(); if (reloads === 0) throw stall('capture.frame'); return 'ok'; } });
    check(r === 'ok' && reloads === 1, 'one reload rescues a stalled page');
  }
  // 3. Stalls through both windows: one reload, then a clear ERROR.
  {
    const c = clock();
    let reloads = 0;
    let err = null;
    try {
      await withStallRetry({ name: 'session.status', windowMs: 30000, now: c.now, isStall, reload: async () => void reloads++, attempt: async () => { c.tick(); throw stall('session.status'); } });
    } catch (e) {
      err = e;
    }
    check(reloads === 1, 'reloads exactly once');
    check(err && /^ERROR capture: session\.status stalled for 30 s twice/.test(err.message), `clear error after the retry (${err?.message})`);
  }
  // 4. Any other error stops at once, no reload.
  {
    let reloads = 0, n = 0, err = null;
    try {
      await withStallRetry({ name: 'param.get', windowMs: 30000, isStall, reload: async () => void reloads++, attempt: async () => { n++; throw new Error('param.get: bad_param: no such name'); } });
    } catch (e) {
      err = e;
    }
    check(n === 1 && reloads === 0 && err?.message.startsWith('param.get: bad_param'), 'a real error is not retried');
  }
  // 5. Without a reload hook: one window, then the ERROR.
  {
    const c = clock();
    let err = null;
    try {
      await withStallRetry({ name: 'tier.set', windowMs: 20000, now: c.now, isStall, attempt: async () => { c.tick(); throw stall('tier.set'); } });
    } catch (e) {
      err = e;
    }
    check(err && /^ERROR capture: tier\.set stalled for 20 s; /.test(err.message), `no reload hook: error after one window (${err?.message})`);
  }
  console.log(fails === 0 ? 'stall-retry self-test: ok' : `stall-retry self-test: ${fails} failed`);
  process.exit(fails === 0 ? 0 : 1);
}

if (process.argv[1]?.replace(/\\/g, '/').endsWith('/stall-retry.mjs') && process.argv.includes('--self-test')) selfTest();
