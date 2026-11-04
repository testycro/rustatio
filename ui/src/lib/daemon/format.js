/** "1h 2m 3s" / "2m 3s"; `expiringLabel` is shown once the delay has elapsed. */
export function formatTimeLeft(seconds, expiringLabel = '…') {
  if (!(seconds > 0)) return expiringLabel;
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  return h > 0 ? `${h}h ${m}m ${s}s` : `${m}m ${s}s`;
}
