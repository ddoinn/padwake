// Display (row A10): resolution and refresh rate from the display's real modes, HDR where the display has it, and
// the PS5-style "Keep these settings?" check. A new mode is a trial: the host applies it without saving and puts the
// old one back after 15 s on its own timer (src-tauri/src/system/display.rs), so a picture the TV can't show fixes
// itself even if this page is stuck. A here keeps it; B, Revert or the timeout put the old one back.

const res = m => `${m.width} × ${m.height}`;
const describe = m => `${res(m)} at ${m.hz} Hz`;
const wait = ms => new Promise(r => setTimeout(r, ms));
const TRIAL_S = 15;

export function display(ctx) {
  const s = { d: null };
  const load = async () => { s.d = await ctx.host.system.display(); };
  const cur = () => s.d?.current ?? { width: 0, height: 0, hz: 0 };
  const sizes = () => [...new Map((s.d?.modes ?? []).map(m => [`${m.width}x${m.height}`, m])).values()];
  const rates = () => [...new Set((s.d?.modes ?? []).filter(m => m.width === cur().width && m.height === cur().height).map(m => m.hz))];

  async function trial(mode) {
    const before = { ...cur() };
    // the host's `display` event says when its timer put the old mode back
    let off = () => {};
    const reverted = new Promise(r => { off = ctx.host.on('display', e => e.state === 'reverted' && r('reverted')); });
    try {
      await ctx.host.system.tryDisplayMode(mode);
      s.d.current = { ...mode };
      ctx.update();
      const answer = await ctx.dialog({
        title: 'Keep these display settings?',
        text: n => `Going back to ${describe(before)} in ${n} second${n === 1 ? '' : 's'}.`,
        countdown: TRIAL_S,
        buttons: [{ label: 'Keep', value: 'keep' }, { label: 'Revert', value: 'revert' }],
        focus: 0,
        until: reverted,
      });
      if (answer === 'keep') await ctx.host.system.keepDisplayMode();
      else if (answer === 'timeout') {
        // the host reverts at the same moment; ask it directly only if its event doesn't come
        if ((await Promise.race([reverted, wait(3000)])) !== 'reverted') await ctx.host.system.revertDisplayMode().catch(() => {});
      } else if (answer !== 'reverted') await ctx.host.system.revertDisplayMode().catch(() => {});
    } finally {
      off();
      await load().catch(() => {});
      ctx.update();
    }
  }

  return {
    title: 'Display',
    layout: 'two',
    load,
    cats: [{
      label: 'Video Output',
      rows: () => [
        { kind: 'link', label: 'Video Output Information', open: () => info(s.d) },
        {
          kind: 'select', label: 'Resolution', key: 'resolution',
          options: () => sizes().map(m => ({ value: `${m.width}x${m.height}`, label: res(m) })),
          get: () => `${cur().width}x${cur().height}`,
          set: v => {
            const [width, height] = v.split('x').map(Number);
            const at = (s.d.modes ?? []).filter(m => m.width === width && m.height === height);
            const mode = at.find(m => m.hz === cur().hz) ?? at[0];
            return trial(mode);
          },
        },
        {
          kind: 'select', label: 'Refresh Rate', key: 'rate',
          desc: 'How many times a second the picture changes. Higher is smoother if the display supports it.',
          options: () => rates().map(hz => ({ value: hz, label: `${hz} Hz` })),
          get: () => cur().hz,
          set: hz => trial({ ...cur(), hz }),
        },
        s.d?.hdr.supported
          ? {
            kind: 'toggle', label: 'HDR', key: 'hdr',
            desc: 'Brighter highlights and richer colour in games that support HDR. Turn it off if the picture looks washed out.',
            get: () => s.d.hdr.enabled,
            set: async v => { await ctx.host.system.setHdr(v); await load(); },
          }
          : { kind: 'info', label: 'HDR', key: 'hdr', value: 'Not supported', disabled: true, desc: "This display doesn't support HDR." },
      ],
    }],
  };
}

// Video Output Information (3-settings-a.md §2.3): a read-only list of what the display reports
function info(d) {
  const at = d.modes.filter(m => m.width === d.current.width && m.height === d.current.height).map(m => `${m.hz} Hz`);
  const sizes = new Set(d.modes.map(res));
  return {
    title: 'Video Output Information',
    layout: 'info',
    rows: [
      { kind: 'info', label: 'Resolution', value: `${res(d.current)} - ${d.current.hz} Hz` },
      { kind: 'info', label: 'HDR', value: !d.hdr.supported ? 'Not supported' : d.hdr.enabled ? 'On' : 'Off' },
      { kind: 'info', label: 'Display', value: d.name || d.device },
      { kind: 'header', label: 'Information for the connected display' },
      { kind: 'info', label: `Refresh Rates (${res(d.current)})`, value: at.join(', ') },
      { kind: 'info', label: 'Resolutions', value: `${sizes.size}, up to ${[...sizes][0] ?? ''}` },
      { kind: 'info', label: 'Device', value: d.device },
    ],
  };
}
