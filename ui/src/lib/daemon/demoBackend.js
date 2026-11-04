// Simulated admin daemon for the browser-only (WASM) build.
//
// The real daemon (scripts/rustatio_daemon.py) is a Python program that talks to a Rustatio
// backend, which does not exist when the app runs purely in the browser. To still let people
// look around the Daemon view, daemonRequest() is answered by this in-memory stand-in: it
// speaks the same small API (same paths, same response shapes) but everything is fictional,
// lives in the page only and is lost on reload. Nothing here ever touches a real torrent.
//
// Pure module (no browser or app imports) so it can be unit tested with node:test.

export const DEMO_PID = 4242;

const LOGS_LIMIT = 500;
const CYCLE_MS = 20_000;
const ANNOUNCE_SECONDS = 130;

export const DEMO_RULES = `stats.seeders <= 2 AND tags != P | addtags | P
stats.seeders > 2 AND tags = P | removetags | P
stats.leechers = 0 AND tags != I AND stats.is_idling = false | addtags | I
stats.leechers = 0 AND tags = I AND stats.is_idling = true | removetags | I
stats.leechers > 0 AND tags = I | removetags | I
stats.ratio: 1.8 - 2.4 AND config.upload_rate > 2 AND tags != SR AND tags != P | addtags | SR
stats.leechers > 10 AND stats.seeders > 100 AND stats.ratio < 9.5 AND tags = SR AND tags != P AND tags != I AND tags != F | addtags | F
stats.leechers < 10 AND tags = F AND tags = SR AND tags != P AND tags != I | removetags | F
tags = F AND stats.ratio: 9.5 - 11.5 | removetags | F
tags = P AND config.upload_rate > 0 | update | config.upload_rate = 0
tags = I AND config.upload_rate > 0 | update | config.upload_rate = 0
tags != SR AND tags != P AND tags != I AND config.upload_rate <= 1.1 | update | config.upload_rate = default_config.upload_rate
tags = F AND config.upload_rate != default_config.upload_rate | update | config.upload_rate = default_config.upload_rate
tags = SR AND tags != P AND tags != I AND tags != F AND config.upload_rate != 1.1 | update | config.upload_rate = 1.1
stats.stop_condition_met = true AND stats.is_idling = true AND stats.idling_reason = "stop_condition_met" AND tags != end | addtags | end
tags = end AND stats.state != "Stopped" | stop |
stats.state: Stopped AND tags = end | delete | watchfile,archive
stats.state = "Stopped" AND stats.stop_condition_met = False AND tags != end | start |
`;

// Same keys and shapes as DEFAULT_CONFIG in the daemon (paths are the Docker defaults).
const DEFAULT_ENV = {
  PORT: 8080,
  RUSTATIO_API: 'http://127.0.0.1:8080',
  ADMIN_PORT: 8081,
  ADMIN_HOST: '127.0.0.1',
  AUTH_TOKEN: 'demo',
  REFRESH_INTERVAL: 0,
  ARCHIVE_FOLDER: '/data/archived',
  RULES_FILE: '/data/rules.txt',
  DEFAULTS_FILE: '/data/state.json',
  DRY_RUN: false,
  LOGFILE: '/data/rustatio_daemon.log',
  CHECK_LOGS_FILE: '/data/check_logs.json',
  LOGS_WATCHER: 1,
  RUST_DAEMON_LOG: '',
  WATCHER_MAX_STRIKE: 3,
  WATCHER_STRIKE_TIME: 3600,
  WATCHER_PAUSE_TIME: 3600,
  WATCHER_ANNOUNCE_TIMEOUT: 130,
  TOR_KEEP_LAST: 1,
};

const READONLY_KEYS = [
  'PORT',
  'ADMIN_PORT',
  'ADMIN_HOST',
  'RUSTATIO_API',
  'AUTH_TOKEN',
  'ARCHIVE_FOLDER',
  'RULES_FILE',
  'DEFAULTS_FILE',
  'LOGFILE',
  'CHECK_LOGS_FILE',
];

const LOG_LEVELS = ['', 'TRACE', 'DEBUG', 'INFO', 'WARN', 'ERROR'];

// Fictional torrents the demo watcher "observes" (offsets in seconds from the demo start).
const WATCHER_SAMPLES = [
  {
    tag: 'Demo.Linux.Distro.Netinstall.amd64.iso',
    status: 'paused',
    errors: { 'Tracker returned: unregistered torrent': 3 },
    ends: 2400,
  },
  {
    tag: 'Demo.Public.Domain.Audiobooks.Collection',
    status: 'observing',
    errors: { 'Announce failed: timeout while connecting to <url>': 1 },
    ends: 1500,
  },
  {
    tag: 'Demo.Open.Movie.Sample.2160p',
    status: 'announce',
    errors: {},
    ends: ANNOUNCE_SECONDS,
  },
];

// Lines the demo "daemon" writes in turn while it is running.
const CYCLE_LINES = [
  '🔍 process_rules: 3 instance(s) evaluated',
  '⚡ Rule applied: stats.seeders <= 2 AND tags != P | addtags | P',
  '   └─ ✅️ Tags added (P)',
  '🔍 process_rules: 3 instance(s) evaluated',
  '⚠️ Demo.Open.Movie.Sample.2160p: tracker announce is slow',
  '🔍 process_rules: 3 instance(s) evaluated',
  '⚡ Rule applied: tags = P AND config.upload_rate > 0 | update | config.upload_rate = 0',
  '   └─ ✅️ Patch succeeded (config.upload_rate = 0)',
];

const pad = n => String(n).padStart(2, '0');

// `DD-MM-YYYY HH:MM:SS`, local time, like the daemon's log formatter.
export function formatLogTime(date) {
  return (
    `${pad(date.getDate())}-${pad(date.getMonth() + 1)}-${date.getFullYear()} ` +
    `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
  );
}

function logLine(timestamp, message) {
  return `${formatLogTime(new Date(timestamp))} :: ${message}`;
}

// Converts a posted value to the type of the default one, like the daemon's parse_value().
function parseLike(value, sample) {
  if (typeof sample === 'boolean') {
    return ['true', '1', 'yes'].includes(String(value).toLowerCase());
  }
  if (typeof sample === 'number') {
    const text = String(value).toLowerCase();
    if (text === 'true' || text === 'yes') return 1;
    if (text === 'false' || text === 'no') return 0;
    const number = Number.parseInt(value, 10);
    return Number.isNaN(number) ? sample : number;
  }
  return String(value);
}

export function createDemoDaemon({ now = () => Date.now(), latencyMs = 0 } = {}) {
  const startedAt = now();

  let running = true;
  let watcherRunning = true;
  let rules = DEMO_RULES;
  let env = { ...DEFAULT_ENV };
  let logs = [];
  let nextCycleAt = startedAt + CYCLE_MS;
  let cycleIndex = 0;
  const cleared = new Set();

  function addLog(message, timestamp = now()) {
    logs.push(logLine(timestamp, message));
    if (logs.length > LOGS_LIMIT) logs = logs.slice(-LOGS_LIMIT);
  }

  // Past activity, so the log is not empty the first time it is opened.
  [
    ['🚀 Starting RustatioManager daemon', 300],
    ['🚀 Defaults loaded from /data/state.json', 299],
    ['🚀 Rules loaded and compiled to AST from /data/rules.txt', 299],
    ['🧪 REFRESH INTERVAL : 60s', 299],
    ['🚀 Log Watcher started', 298],
    ['🔍 process_rules: 3 instance(s) evaluated', 240],
    ['⚡ Rule applied: stats.seeders <= 2 AND tags != P | addtags | P', 180],
    ['   └─ ✅️ Tags added (P)', 180],
    ['👁 Demo.Linux.Distro.Netinstall.amd64.iso: strike 3/3, paused', 120],
    ['🔍 process_rules: 3 instance(s) evaluated', 60],
  ].forEach(([message, secondsAgo]) => addLog(message, startedAt - secondsAgo * 1000));
  addLog('ℹ️ Demo mode: this log is simulated, nothing is running', startedAt - 1000);

  // Add the lines the daemon would have written since the last call.
  function catchUp() {
    const current = now();
    while (running && nextCycleAt <= current) {
      addLog(CYCLE_LINES[cycleIndex % CYCLE_LINES.length], nextCycleAt);
      cycleIndex += 1;
      nextCycleAt += CYCLE_MS;
    }
    if (!running) nextCycleAt = Math.max(nextCycleAt, current + CYCLE_MS);
  }

  function watcherState() {
    const current = now();
    const state = {};

    for (const sample of WATCHER_SAMPLES) {
      if (cleared.has(sample.tag)) continue;
      const deadline = startedAt + sample.ends * 1000;
      const timeLeft = Math.max(0, Math.floor((deadline - current) / 1000));
      if (sample.status !== 'announce' && timeLeft === 0) continue;

      const isAnnounce = sample.status === 'announce';
      // The announce countdown keeps restarting, like a new announce being detected
      const elapsed = (current - startedAt) / 1000;
      const announceLeft = Math.floor(ANNOUNCE_SECONDS - (elapsed % ANNOUNCE_SECONDS));
      state[sample.tag] = {
        errors: sample.errors,
        total_strikes: Object.values(sample.errors).reduce((sum, count) => sum + count, 0),
        status: sample.status,
        time_left: isAnnounce ? announceLeft : timeLeft,
        announce_ts: isAnnounce ? Math.floor(startedAt / 1000) : 0,
      };
    }
    return state;
  }

  const reply = (data, status = 200) => ({ ok: status < 400, status, data });

  function manageDaemon(action) {
    if (action === 'stop') {
      if (running) addLog('🏁 RustatioManager stopped');
      running = false;
      watcherRunning = false;
      return reply({ success: true });
    }
    if (action === 'start' || action === 'restart') {
      if (action === 'restart' && running) addLog('🏁 RustatioManager stopped');
      if (action === 'restart' || !running) {
        addLog('🚀 Starting RustatioManager daemon');
        nextCycleAt = now() + CYCLE_MS;
      }
      running = true;
      watcherRunning = Boolean(env.LOGS_WATCHER);
      return reply({ success: true });
    }
    return reply({ success: false, error: 'Action inconnue ou état incorrect' });
  }

  function manageWatcher(action) {
    if (!running) return reply({ success: false, error: 'Le daemon principal est arrêté' });
    if (action === 'start' || action === 'restart') {
      watcherRunning = true;
      addLog('🚀 Log Watcher started');
      return reply({ success: true });
    }
    if (action === 'stop') {
      watcherRunning = false;
      addLog('🏁 Log Watcher stopped');
      return reply({ success: true });
    }
    return reply({ success: false, error: 'Action inconnue' });
  }

  function updateEnv(payload) {
    const forbidden = Object.keys(payload).filter(key => READONLY_KEYS.includes(key));
    if (forbidden.length > 0) {
      return reply(
        { status: 'forbidden', message: `Modification interdite pour : ${forbidden.join(', ')}` },
        403
      );
    }

    const allowedLevels = [...LOG_LEVELS].sort().join(', ');
    const next = { ...env };
    for (const [key, value] of Object.entries(payload)) {
      if (!(key in next)) continue;
      if (key === 'RUST_DAEMON_LOG') {
        const level = String(value).trim().toUpperCase();
        if (!LOG_LEVELS.includes(level)) {
          return reply(
            {
              status: 'forbidden',
              message: `Valeur refusée pour RUST_DAEMON_LOG. Autorisé : ${allowedLevels}`,
            },
            403
          );
        }
        next[key] = level;
      } else {
        next[key] = parseLike(value, DEFAULT_ENV[key]);
      }
    }
    env = next;
    return reply({ status: 'success', message: 'Variables mises à jour temporairement' });
  }

  async function handle(path, { method = 'GET', body } = {}) {
    catchUp();

    const url = new URL(path, 'http://demo.invalid');
    const route = url.pathname.replace(/\/+$/, '') || '/';
    const verb = String(method).toUpperCase();
    const payload = body && typeof body === 'object' ? body : {};

    if (route === '/status' && verb === 'GET') {
      return reply(running ? { running: true, pid: DEMO_PID } : { running: false });
    }

    const daemonAction = route.match(/^\/daemon\/([^/]+)$/);
    if (daemonAction && verb === 'POST') return manageDaemon(daemonAction[1]);

    if (route === '/rules') {
      if (verb === 'POST') {
        rules = String(payload.content ?? '');
        addLog('🚀 Rules loaded and compiled to AST from /data/rules.txt');
        return reply({ success: true });
      }
      if (verb === 'GET') return reply({ content: rules });
    }

    if (route === '/logs' && verb === 'GET') {
      const requested = Number.parseInt(url.searchParams.get('lines') ?? '100', 10);
      const count = Number.isNaN(requested) || requested < 1 ? 100 : requested;
      const lines = logs.slice(-count);
      return reply({ content: lines.length > 0 ? `${lines.join('\n')}\n` : '' });
    }
    if (route === '/logs/clear' && verb === 'POST') {
      logs = [];
      return reply({ success: true });
    }
    if (route === '/logs/archive' && verb === 'POST') {
      const date = new Date(now());
      const day = `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
      const time = `${pad(date.getHours())}-${pad(date.getMinutes())}-${pad(date.getSeconds())}`;
      const filename = `rustatio_daemon_${day}_${time}.log`;
      logs = [logLine(now(), `--- Logs archivés (démo) : ${filename} ---`)];
      return reply({ success: true, filename });
    }

    if (route === '/admin/restart' && verb === 'POST') {
      addLog('🚀 Script restarted (demo: nothing was actually restarted)');
      return reply({ success: true, message: 'Redémarrage du processus...' });
    }

    if (route === '/watcher/status' && verb === 'GET') {
      return reply(
        running && watcherRunning
          ? { running: true, status: 'ok', pid: DEMO_PID }
          : { running: false, status: 'stopped' }
      );
    }
    if (route === '/watcher/state' && verb === 'GET') {
      return reply({ state: running ? watcherState() : {} });
    }
    if (route === '/watcher/clear_resume' && verb === 'POST') {
      if (!payload.tag) return reply({ success: false, error: 'Tag manquant' }, 400);
      cleared.add(String(payload.tag));
      addLog(`♻️ Strikes cleared and torrent resumed: ${payload.tag}`);
      return reply({ success: true });
    }
    const watcherAction = route.match(/^\/watcher\/([^/]+)$/);
    if (watcherAction && verb === 'POST') return manageWatcher(watcherAction[1]);

    if (route === '/env' && verb === 'GET') {
      return reply({ config: { ...env }, readonly: [...READONLY_KEYS] });
    }
    if (route === '/env' && verb === 'POST') return updateEnv(payload);
    if (route === '/env/reset' && verb === 'POST') {
      env = { ...DEFAULT_ENV };
      return reply({
        status: 'success',
        message: "Configuration réinitialisée aux valeurs d'origine",
      });
    }

    return reply({ error: 'Not found' }, 404);
  }

  return {
    async request(path, options) {
      if (latencyMs > 0) await new Promise(resolve => setTimeout(resolve, latencyMs));
      return handle(path, options);
    },
  };
}

let shared = null;

// Entry point used by daemonRequest() in the browser-only build: one daemon per page load.
export function demoRequest(path, options) {
  shared ??= createDemoDaemon({ latencyMs: 80 });
  return shared.request(path, options);
}
