// Pure helpers for the daemon rules editor (no imports: testable with `node --test`).
// Rule line format: `condition [AND|OR condition ...] | action | assignment`

export const FIELDS = [
  'id',
  'torrent.info_hash',
  'torrent.announce',
  'torrent.name',
  'torrent.total_size',
  'torrent.piece_length',
  'torrent.num_pieces',
  'torrent.comment',
  'torrent.created_by',
  'torrent.is_single_file',
  'torrent.file_count',
  'config.upload_rate',
  'config.download_rate',
  'config.port',
  'config.vpn_port_sync',
  'config.client_type',
  'config.client_version',
  'config.initial_uploaded',
  'config.initial_downloaded',
  'config.completion_percent',
  'config.num_want',
  'config.randomize_rates',
  'config.random_range_percent',
  'config.randomize_ratio',
  'config.random_ratio_range_percent',
  'config.stop_at_ratio',
  'config.effective_stop_at_ratio',
  'config.stop_at_uploaded',
  'config.stop_at_downloaded',
  'config.stop_at_seed_time',
  'config.idle_when_no_leechers',
  'config.idle_when_no_seeders',
  'config.scrape_interval',
  'config.progressive_rates',
  'config.target_upload_rate',
  'config.target_download_rate',
  'config.progressive_duration',
  'config.post_stop_action',
  'stats.uploaded',
  'stats.downloaded',
  'stats.ratio',
  'stats.left',
  'stats.torrent_completion',
  'stats.seeders',
  'stats.leechers',
  'stats.state',
  'stats.is_idling',
  'stats.idling_reason',
  'stats.session_uploaded',
  'stats.session_downloaded',
  'stats.session_ratio',
  'stats.current_upload_rate',
  'stats.current_download_rate',
  'stats.average_upload_rate',
  'stats.average_download_rate',
  'stats.upload_progress',
  'stats.download_progress',
  'stats.ratio_progress',
  'stats.seed_time_progress',
  'stats.effective_stop_at_ratio',
  'stats.eta_ratio',
  'stats.eta_uploaded',
  'stats.eta_download_completion',
  'stats.stop_condition_met',
  'stats.post_stop_action',
  'stats.stats.announce_count',
  'tags',
  'default_config.upload_rate',
  'default_config.download_rate',
];

// Settable fields for the `update` action (value = type of the setting).
export const CONFIG_TYPES = {
  'config.upload_rate': 'number',
  'config.download_rate': 'number',
  'config.port': 'number',
  'config.vpn_port_sync': 'boolean',
  'config.client_type': 'string',
  'config.client_version': 'string',
  'config.initial_uploaded': 'number',
  'config.initial_downloaded': 'number',
  'config.completion_percent': 'number',
  'config.num_want': 'number',
  'config.randomize_rates': 'boolean',
  'config.random_range_percent': 'number',
  'config.randomize_ratio': 'boolean',
  'config.random_ratio_range_percent': 'number',
  'config.stop_at_ratio': 'number',
  'config.effective_stop_at_ratio': 'number',
  'config.stop_at_uploaded': 'number',
  'config.stop_at_downloaded': 'number',
  'config.stop_at_seed_time': 'number',
  'config.idle_when_no_leechers': 'boolean',
  'config.idle_when_no_seeders': 'boolean',
  'config.scrape_interval': 'number',
  'config.progressive_rates': 'boolean',
  'config.target_upload_rate': 'number',
  'config.target_download_rate': 'number',
  'config.progressive_duration': 'number',
  'config.post_stop_action': 'string',
};

export const CONFIG_KEYS = Object.keys(CONFIG_TYPES);
// Suggestions for the value input: a literal, or the same setting taken from default_config.
export const DEFAULT_CONFIG_SUGGESTIONS = CONFIG_KEYS.map(key =>
  key.replace(/^config\./, 'default_config.')
);

export const OPERATORS = [':', '=', '<', '>', '!=', '<=', '>=', '~'];
export const ACTIONS = [
  'start',
  'stop',
  'pause',
  'resume',
  'delete',
  'update',
  'addtags',
  'removetags',
];
export const LOGICAL_OPS = ['AND', 'OR'];
export const SIMPLE_ACTIONS = ['start', 'stop', 'pause', 'resume'];

const DEFAULT_CONF_KEY = 'config.upload_rate';
const CONDITION_REGEX = /^([a-zA-Z0-9_.]+)\s*(<=|>=|!=|:|=|<|>|~)\s*(.*)$/;

let idCounter = 0;
const nextId = () => ++idCounter;

export function isSimpleAction(action) {
  return SIMPLE_ACTIONS.includes(action);
}

export function createCondition(logOp = 'AND', field = 'id', op = ':', val = '') {
  return { id: nextId(), logOp, field, op, val };
}

export function createRule() {
  return {
    id: nextId(),
    conditions: [createCondition()],
    action: 'start',
    conf: DEFAULT_CONF_KEY, // `update` : setting name
    value: '', // `update` : new value (literal or default_config.*)
    text: '', // other actions : free parameters (tags...)
  };
}

/** Returns `list`, plus `current` when it is missing (so an unknown value is never silently replaced). */
export function withCurrent(list, current) {
  return current && !list.includes(current) ? [...list, current] : list;
}

export function parseRules(text) {
  const rules = [];

  for (const line of String(text ?? '').split('\n')) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith('#')) continue;

    const parts = trimmed.split('|').map(part => part.trim());
    if (parts.length < 2) continue;

    const rule = createRule();
    rule.action = parts[1] || 'start';
    const assign = parts.length >= 3 ? parts[2] : '';

    const conditions = [];
    let logOp = 'AND';
    for (const token of parts[0].split(/\s+(AND|OR|and|or)\s+/)) {
      const piece = token.trim();
      if (/^(AND|OR)$/i.test(piece)) {
        logOp = piece.toUpperCase();
      } else if (piece) {
        const match = piece.match(CONDITION_REGEX);
        conditions.push(
          match
            ? createCondition(logOp, match[1], match[2], match[3])
            : createCondition(logOp, piece, ':', '')
        );
        logOp = 'AND';
      }
    }
    if (conditions.length > 0) rule.conditions = conditions;

    if (rule.action === 'update') {
      const eq = assign.indexOf('=');
      if (eq >= 0) {
        rule.conf = assign.slice(0, eq).trim() || DEFAULT_CONF_KEY;
        rule.value = assign.slice(eq + 1).trim();
      }
    } else if (!isSimpleAction(rule.action)) {
      rule.text = assign;
    }

    rules.push(rule);
  }

  return rules.length > 0 ? rules : [createRule()];
}

export function serializeRules(rules) {
  const lines = [];

  for (const rule of rules) {
    const condition = rule.conditions
      .map((c, index) => {
        const expr = c.op === ':' ? `${c.field}: ${c.val}` : `${c.field} ${c.op} ${c.val}`;
        return index > 0 ? ` ${c.logOp || 'AND'} ${expr}` : expr;
      })
      .join('');

    let assign = '';
    if (rule.action === 'update') {
      if (rule.conf) assign = `${rule.conf} = ${rule.value}`;
    } else if (!isSimpleAction(rule.action)) {
      assign = rule.text;
    }

    if (condition && rule.action) {
      lines.push(`${condition} | ${rule.action} |${assign ? ` ${assign}` : ''}`);
    }
  }

  return lines.join('\n');
}
