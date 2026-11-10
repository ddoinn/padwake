// The colour of a game's own art, for the screens that have no background art to show (row B5): Home and the journal
// page wash their background in the tile's average colour, kept dark, so the screen still follows the focused game.
// Padwake's own UI has no hue of its own (base.css); colour only ever comes from game art.
//
//   artWash(url) → Promise<string | null>   a CSS radial gradient in the art's colour, or null when it can't be read
//   NEUTRAL_WASH                            the same shape in grey, for an entry with no art at all

const cache = new Map(); // url → Promise<string | null>
const SHAPE = 'radial-gradient(ellipse at 78% 40%,';
export const NEUTRAL_WASH = `${SHAPE} #262626, #080808 70%, #000)`;

export function artWash(url) {
  if (!url) return Promise.resolve(null);
  if (!cache.has(url)) {
    const img = new Image();
    img.crossOrigin = 'anonymous'; // readable on a canvas: the asset protocol allows our origin
    img.src = url;
    cache.set(url, img.decode().then(() => wash(img), () => null));
    if (cache.size > 64) cache.delete(cache.keys().next().value);
  }
  return cache.get(url);
}

// the art's average on an 8 × 8 copy, brought down so its brightest channel is at most 25 % (key art is dark where
// text sits); the edge goes to near-black in the same hue
function wash(img) {
  const c = Object.assign(document.createElement('canvas'), { width: 8, height: 8 });
  const g = c.getContext('2d', { willReadFrequently: true });
  g.drawImage(img, 0, 0, 8, 8);
  let d;
  try { d = g.getImageData(0, 0, 8, 8).data; } catch { return null; }
  const sum = [0, 0, 0];
  for (let i = 0; i < d.length; i += 4) for (let k = 0; k < 3; k++) sum[k] += d[i + k];
  const avg = sum.map(v => v / 64);
  const k = Math.min(1, 64 / Math.max(1, ...avg));
  const at = f => `rgb(${avg.map(v => Math.round(v * k * f)).join(', ')})`;
  return `${SHAPE} ${at(1)}, ${at(0.22)} 70%, #000)`;
}
