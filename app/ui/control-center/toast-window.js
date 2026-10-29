// One toast in its own window while the app sleeps (toast.html; the host opens it, src-tauri/src/system/toast.rs).
import { mountToasts, toast } from './toast.js';
const box = document.getElementById('box');
const fit = () => { box.style.transform = `scale(${innerWidth / 652})`; };
addEventListener('resize', fit);
fit();
mountToasts(box, { x: 0, y: 0 });
const q = new URLSearchParams(location.search);
const icon = q.get('icon') || '';
await document.fonts.load('400 30px "Open Sans"');
toast({
  title: q.get('title') ?? '',
  message: q.get('message') ?? '',
  icon: !icon ? null : icon.startsWith('glyph:') ? icon : window.__TAURI__?.core.convertFileSrc(icon) ?? icon,
});
