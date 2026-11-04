// Shared Tailwind class strings for the daemon view.
const base =
  'inline-flex h-7 items-center gap-1 border px-2.5 text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-40 cursor-pointer';

export const btn = `${base} border-border text-foreground hover:bg-muted`;
export const btnPrimary = `${base} border-primary bg-primary font-medium text-primary-foreground hover:opacity-90`;
/** Tinted button; `tone` is one of: start, stop, restart (colors live in daemon.css). */
const btnTone = tone => `${base} dm-tone dm-${tone}`;
export const btnStart = btnTone('start');
export const btnDanger = btnTone('stop');
export const btnRestart = btnTone('restart');

export const field =
  'h-7 border border-input bg-background px-2 text-xs text-foreground placeholder:text-muted-foreground focus:border-ring focus:outline-none disabled:opacity-60';

export const badgeTones = {
  running: 'border-stat-upload/30 bg-stat-upload/10 text-stat-upload',
  stopped: 'border-stat-danger/30 bg-stat-danger/10 text-stat-danger',
  paused: 'border-stat-ratio/30 bg-stat-ratio/10 text-stat-ratio',
  announce: 'border-stat-download/30 bg-stat-download/10 text-stat-download',
  neutral: 'border-border bg-muted/30 text-muted-foreground',
};
export const badgeBase =
  'inline-flex items-center gap-1 rounded-md border px-2 py-0.5 text-xs font-medium';
