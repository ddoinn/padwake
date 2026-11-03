// Padwake's own trophies: the same six for every game, worked out from what the app itself counted (time played and
// sessions), so they need no account, no network and no game support, and a restored backup brings them back. Nothing
// the player can set earns one: a journal status can't, and neither can a rating. They are not the game's achievements
// (game.achievements, library/progress.rs) and never count towards them. No dates: a trophy's date isn't recorded.

export const TROPHIES = [
  { id: 'first-steps', name: 'First Steps', tier: 'bronze', goal: 'First play', earned: g => g.playtimeSec >= 60 },
  { id: 'regular', name: 'Regular', tier: 'bronze', goal: '10 sessions', earned: g => g.sessions >= 10 },
  { id: 'invested', name: 'Invested', tier: 'silver', goal: '10 hours', earned: g => g.playtimeSec >= 10 * 3600 },
  { id: 'old-friend', name: 'Old Friend', tier: 'silver', goal: '50 sessions', earned: g => g.sessions >= 50 },
  { id: 'devoted', name: 'Devoted', tier: 'gold', goal: '50 hours', earned: g => g.playtimeSec >= 50 * 3600 },
  { id: 'centurion', name: 'Centurion', tier: 'gold', goal: '100 hours', earned: g => g.playtimeSec >= 100 * 3600 },
];

const numbers = g => ({ playtimeSec: g?.playtimeSec ?? 0, sessions: g?.sessions ?? 0 });

export const earned = g => TROPHIES.filter(t => t.earned(numbers(g)));

// what a session added: those earned after it and not before it
export function newlyEarned(before, after) {
  const had = new Set(earned(before).map(t => t.id));
  return earned(after).filter(t => !had.has(t.id));
}

export const tierName = t => t.tier[0].toUpperCase() + t.tier.slice(1);
export const TIER_COLOR = { bronze: '#cd8f5e', silver: '#c9ccd6', gold: '#f2c75c' }; // as journal.css

// our own cup, in currentColor
export const CUP = '<svg viewBox="0 0 32 32"><path fill="currentColor" d="M9 4h14v2h5v4a6 6 0 0 1-5.6 6A7 7 0 0 1 17 20.9V24h4a2 2 0 0 1 2 2v2H9v-2a2 2 0 0 1 2-2h4v-3.100A7 7 0 0 1 9.600 16 6 6 0 0 1 4 10V6h5Zm0 4H6v2a4 4 0 0 0 3 3.900Zm14 0v5.900A4 4 0 0 0 26 10V8Z"/></svg>';
