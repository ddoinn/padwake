// Cloud PC mode (Settings → Power, `cloudPc` in settings): for AntCloud, whose PCs don't turn off from Windows. The
// session ends from the device streaming it (antcloud.co/downloads/windows, antcloud.co/faq, checked 2026-11-02):
// the AntCloud app's Ctrl+F9 closes the stream, the browser's F9 opens its menu with Shut Down PC, and the PC turns off
// by itself about a minute after the AntCloud tab closes. Keys pressed inside the PC never reach that device, so
// Padwake can only say what to press: Shut Down becomes End Session (these steps), and Sleep goes.

export const isCloudPc = settings => settings?.cloudPc === true;

export const END_LABEL = 'End Session';
export const END_TITLE = 'End your AntCloud session';
export const END_STEPS = [
  'AntCloud app: press Ctrl + F9 on your device.',
  'Browser: press F9 on your device.',
  'Then choose Shut Down PC.',
  // one short line each: Control Center's panel cuts longer ones
  'Or close the AntCloud tab.',
  'The PC turns off about a minute later.',
];
