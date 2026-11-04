// Turns the daemon log file into colorable lines.
// Format: `DD-MM-YYYY HH:MM:SS :: <emoji prefix><message>` (continuation lines have no timestamp).

const LINE_RE = /^(\d{2}-\d{2}-\d{4} \d{2}:\d{2}:\d{2}) :: (.*)$/;

export function toneOf(body) {
  const text = body.replace(/^\s*└─\s*/, '');
  if (text.startsWith('❌')) return 'error';
  if (text.startsWith('⚠') || text.startsWith('🚫')) return 'warn';
  if (text.startsWith('✅')) return 'success';
  if (text.startsWith('🔍') || text.startsWith('🧪')) return 'trace';
  if (text.startsWith('🚀') || text.startsWith('🏁')) return 'accent';
  return 'info';
}

export function parseLogLines(content) {
  const lines = String(content ?? '').split('\n');
  if (lines[lines.length - 1] === '') lines.pop();

  let previous = 'info';
  return lines.map((line, id) => {
    const match = line.match(LINE_RE);
    if (match) {
      previous = toneOf(match[2]);
      return { id, time: match[1], body: match[2], tone: previous };
    }
    // Continuation (e.g. traceback): keep the tone of an error / warning above it
    const tone = previous === 'error' || previous === 'warn' ? previous : 'info';
    return { id, time: '', body: line, tone };
  });
}
