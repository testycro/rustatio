#!/usr/bin/env python3
import os
import sys
import time
import json
import logging
from logging.handlers import RotatingFileHandler
import threading
import random
import re
import shutil
import urllib.parse
import requests
import copy
from collections import defaultdict

# ==========================================
# CONFIGURATION
# ==========================================
PORT = os.environ.get("PORT", "8080")
RUSTATIO_API = os.environ.get("RUSTATIO_API", f"http://127.0.0.1:{PORT}")
REFRESH_INTERVAL = int(os.environ.get("REFRESH_INTERVAL", 0))
ARCHIVE_FOLDER = os.environ.get("ARCHIVE_FOLDER", "/data/archived")
RULES_FILE = os.environ.get("RULES_FILE", "/data/rules.txt")
DEFAULTS_FILE = os.environ.get("DEFAULTS_FILE", "/data/state.json")
DRY_RUN = os.environ.get("DRY_RUN", "false").lower() == "true"
LOGFILE = os.environ.get("LOGFILE", "/data/rustatio_daemon.log")
TRACE = os.environ.get("TRACE", "false").lower() == "true"
CHECK_LOGS_FILE = os.environ.get("CHECK_LOGS_FILE", os.path.join(os.path.dirname(RULES_FILE), "check_logs.json"))

LOGS_WATCHER = int(os.environ.get("LOGS_WATCHER", 1))
WATCHER_MAX_STRIKE = int(os.environ.get("WATCHER_MAX_STRIKE", 3))
WATCHER_STRIKE_TIME = int(os.environ.get("WATCHER_STRIKE_TIME", 3600))
WATCHER_PAUSE_TIME = int(os.environ.get("WATCHER_PAUSE_TIME", 3600))
TOR_KEEP_LAST = int(os.environ.get("TOR_KEEP_LAST", 1))

# ==========================================
# LOGGING SETUP
# ==========================================
logger = logging.getLogger("Rustatio")
logger.setLevel(logging.INFO)
formatter = logging.Formatter('%(asctime)s :: %(message)s', "%d-%m-%Y %H:%M:%S")

console_handler = logging.StreamHandler()
console_handler.setFormatter(formatter)
logger.addHandler(console_handler)

def setup_file_handler():
    if LOGFILE and LOGFILE != "/dev/null":
        for h in logger.handlers[:]:
            if isinstance(h, RotatingFileHandler):
                h.close()
                logger.removeHandler(h)
        log_dir = os.path.dirname(LOGFILE)
        if log_dir:
            os.makedirs(log_dir, exist_ok=True)
        if not os.path.exists(LOGFILE):
            open(LOGFILE, 'a').close()
        file_handler = RotatingFileHandler(LOGFILE, maxBytes=1024*1024, backupCount=10)
        file_handler.setFormatter(formatter)
        logger.addHandler(file_handler)

setup_file_handler()

def log(msg, style="default"):
    prefix_spaces = ""
    if style.startswith("ff_"):
        prefix_spaces = "          └─ "
        style = style[3:]
    elif style.startswith("f_"):
        prefix_spaces = "   └─ "
        style = style[2:]

    prefixes = {
        "start": "🚀 ", "error": "❌ ", "succes": "✅️ ", "warning": "⚠️ ",
        "denied": "🚫 ", "saving": "💾 ", "data": "🧪 ", "lock": "🔒 ",
        "recycle": "♻️ ", "task": "⚡ ", "finish": "🏁 ", "trace": "🔍 "
    }
    prefix = prefixes.get(style, "")
    logger.info(f"{prefix_spaces}{prefix}{msg}")

def trace(msg):
    if TRACE:
        log(msg, "ff_trace")

# ==========================================
# API CLIENT
# ==========================================
class APIClient:
    def __init__(self, base_url):
        self.base_url = base_url.rstrip("/")
        self.session = requests.Session()
        self.session.headers.update({"Accept": "application/json"})
        trace(f"APIClient initialized with base_url: {self.base_url}")

    def request(self, method, endpoint, payload=None, timeout=5):
        url = f"{self.base_url}/api/{endpoint}"
        max_retries = 3
        trace(f"API Request -> {method} {url} with payload: {payload}")

        for attempt in range(1, max_retries + 1):
            try:
                if payload and method in ["POST", "PATCH"]:
                    resp = self.session.request(method, url, json=payload, timeout=timeout)
                else:
                    resp = self.session.request(method, url, timeout=timeout)
                resp.raise_for_status()
                data = resp.json()

                trace(f"API Response <- {method} {endpoint} Status: {resp.status_code}")

                if data.get("success") is True:
                    if "data" in data or "stats" in data or "config" in data:
                        return data
                    return None
                elif "data" in data or isinstance(data, list):
                    return data
            except Exception as e:
                if attempt == max_retries:
                    log(f"Failed to {method} {endpoint} after {max_retries} attempts. Error: {str(e)}", "f_error")
                else:
                    log(f"Attempt {attempt} failed for {method} {endpoint}. Retrying in 2 seconds...", "f_error")
                    time.sleep(2)
        return None

# ==========================================
# CORE MANAGER
# ==========================================
class RustatioManager:
    def __init__(self):
        self.api = APIClient(RUSTATIO_API)
        self.default_config = {}
        self.rules_lines = []
        self.strike_lock = threading.Lock()
        self.logs_state = self.load_logs_state()
        self.rand_cache = {}
        self.used_rand_keys = set()
        self.current_instances = []

        trace("Initializing regex patterns for RustatioManager")

        self.re_range = re.compile(r'([A-Za-z0-9_.]+): *([0-9.]+) *- *([0-9.]+)')
        self.re_default_config = re.compile(r'default_config\.([A-Za-z0-9_.]+)')
        self.re_num_cmp = re.compile(r'([A-Za-z0-9_.]+) *(<=|>=|<|>) *([0-9.]+)')
        self.re_num_eq = re.compile(r'([A-Za-z0-9_.]+) *(!=|=) *([0-9.]+)')
        self.re_num_neq = re.compile(r'([A-Za-z0-9_.]+) *!= *([0-9.]+)')
        self.re_bool_null_eq = re.compile(r'([A-Za-z0-9_.]+) *= *(true|false|null)', re.IGNORECASE)
        self.re_bool_null_colon = re.compile(r'([A-Za-z0-9_.]+) *: *(true|false|null)', re.IGNORECASE)
        self.re_inst_keys = re.compile(r'([A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)+)')

        self.regex_subs = [
            (re.compile(r'torrent\.(announce|name|comment|created_by) *~ *"([^"]+)"'), r'("\2".lower() in str(__get__("torrent.\1", "")).lower())'),
            (re.compile(r'torrent\.(announce|name|comment|created_by) *~ *([A-Za-z0-9_@./:-]+)'), r'("\2".lower() in str(__get__("torrent.\1", "")).lower())'),
            (re.compile(r'tags *!= *"([^"]+)"'), r'("\1" not in __tags__)'),
            (re.compile(r'tags *!= *([A-Za-z0-9_@./:-]+)'), r'("\1" not in __tags__)'),
            (re.compile(r'tags *: *"([^"]+)"'), r'("\1" in __tags__)'),
            (re.compile(r'tags *: *([A-Za-z0-9_@./:-]+)'), r'("\1" in __tags__)'),
            (re.compile(r'tags *= *"([^"]+)"'), r'("\1" in __tags__)'),
            (re.compile(r'tags *= *([A-Za-z0-9_@./:-]+)'), r'("\1" in __tags__)'),
            (re.compile(r'torrent\.info_hash *: *"([A-Fa-f0-9]+)"'), r'(__info_hash__() == "\1")'),
            (re.compile(r'torrent\.info_hash *([A-Fa-f0-9]+)'), r'(__info_hash__() == "\1")'),
            (re.compile(r'torrent\.info_hash *= *"([A-Fa-f0-9]+)"'), r'(__info_hash__() == "\1")'),
            (re.compile(r'torrent\.info_hash *= *([A-Fa-f0-9]+)'), r'(__info_hash__() == "\1")'),
            (re.compile(r'([A-Za-z0-9_.]+) *!= *"([^"]+)"'), r'(str(__get__("\1", "")) != "\2")'),
            (re.compile(r'([A-Za-z0-9_.]+) *!= *([A-Za-z0-9_@./:-]+)'), r'(str(__get__("\1", "")) != "\2")'),
            (re.compile(r'([A-Za-z0-9_.]+) *= *"([^"]+)"'), r'(str(__get__("\1", "")) == "\2")'),
            (re.compile(r'([A-Za-z0-9_.]+) *= *([A-Za-z0-9_@./:-]+)'), r'(str(__get__("\1", "")) == "\2")'),
            (re.compile(r'([A-Za-z0-9_.]+) *: *"([^"]+)"'), r'(str(__get__("\1", "")) == "\2")'),
            (re.compile(r'([A-Za-z0-9_.]+) *: *([A-Za-z0-9_@./:-]+)'), r'(str(__get__("\1", "")) == "\2")')
        ]

    def get_val(self, data, path, default=None):
        current = data
        for part in path.split('.'):
            if isinstance(current, dict) and part in current:
                current = current.get(part)
            else:
                trace(f"get_val path '{path}' missed at part '{part}', returning default: {default}")
                return default
        return current

    def get_num(self, data, path, default=0):
        val = self.get_val(data, path, default)
        try:
            return float(val) if val is not None else float(default)
        except (ValueError, TypeError):
            trace(f"get_num conversion failed for path '{path}' with value '{val}', falling back to default: {default}")
            return float(default)

    def _extract_bracket(self, msg):
        start = msg.find("[")
        if start == -1:
            return None, msg
        level = 0
        for i in range(start, len(msg)):
            if msg[i] == "[":
                level += 1
            elif msg[i] == "]":
                level -= 1
                if level == 0:
                    return msg[start+1:i].strip(), msg[i+1:].strip()
        return msg[start:], ""

    def load_configs(self):
        trace("Loading configurations...")
        try:
            if os.path.exists(DEFAULTS_FILE):
                with open(DEFAULTS_FILE, 'r') as f:
                    data = json.load(f)
                    self.default_config = data.get("default_config", data)
                    log(f"Defaults loaded from {DEFAULTS_FILE}", "start")
                    trace(f"Loaded default_config content: {self.default_config}")
            else:
                log(f"Defaults file {DEFAULTS_FILE} not found", "error")
        except Exception as e:
            self.default_config = {}
            log(f"Defaults file {DEFAULTS_FILE} is not valid JSON (Error: {e})", "error")

        try:
            if os.path.exists(RULES_FILE):
                with open(RULES_FILE, "r") as f:
                    raw_lines = [l.strip() for l in f if l.strip() and not l.startswith("#")]

                self.rules_lines = []
                for line in raw_lines:
                    trace(f"Processing rule line: {line}")
                    parts = line.split('|', 2)
                    if len(parts) == 3:
                        cond, action, assign = [x.strip() for x in parts]

                        default_keys = set(self.re_default_config.findall(cond))
                        all_keys = set(self.re_inst_keys.findall(cond))
                        instance_keys = {
                            k for k in all_keys 
                            if not k.startswith("default_config.") and not k.startswith("torrent.info_hash")
                        }

                        py_cond = self.translate_condition(cond)
                        try:
                            compiled_cond = compile(py_cond, '<string>', 'eval')
                            self.rules_lines.append({
                                "raw": line,
                                "cond_str": cond,
                                "compiled": compiled_cond,
                                "action": action,
                                "assign": assign,
                                "default_keys": default_keys,
                                "instance_keys": instance_keys
                            })
                            trace(f"Rule successfully compiled -> Python condition: {py_cond}")
                        except SyntaxError as e:
                            log(f"Generated rule syntax error: {py_cond} ({e})", "error")
                    else:
                        log(f"Invalid rule skipped: {line}", "denied")

                log(f"Rules loaded and compiled from {RULES_FILE}", "start")
            else:
                log(f"Rules file {RULES_FILE} not found", "error")
        except Exception as e:
            log(f"Error: {str(e)}", "error")

    def load_logs_state(self):
        trace(f"Loading logs state from {CHECK_LOGS_FILE}")
        try:
            if os.path.exists(CHECK_LOGS_FILE):
                with open(CHECK_LOGS_FILE, 'r') as f:
                    return json.load(f)
        except Exception as e:
            log(f"Error: {str(e)}", "error")
        return {}

    def save_logs_state(self):
        trace(f"Saving logs state to {CHECK_LOGS_FILE}")
        try:
            with open(CHECK_LOGS_FILE, 'w') as f:
                json.dump(self.logs_state, f)
        except Exception as e:
            log(f"Error: {str(e)}", "error")

    def format_time_bash_style(self, t):
        h = t // 3600
        m = (t % 3600) // 60
        if h > 0:
            return f"{h}h{m:02d}" if m > 0 else f"{h}h"
        return f"{m}min"

    def get_elapsed_since_midnight_tag(self, ts):
        t_struct = time.localtime(ts)
        midnight = time.mktime((t_struct.tm_year, t_struct.tm_mon, t_struct.tm_mday, 0, 0, 0, t_struct.tm_wday, t_struct.tm_yday, t_struct.tm_isdst))
        return self.format_time_bash_style(int(ts - midnight))

    def get_cached_rand(self, path, low, high, raw_match):
        key = f"{path}:{low}:{high}:{raw_match}"
        if key not in self.rand_cache:
            l, h = float(low), float(high)
            if l > h: l, h = h, l
            self.rand_cache[key] = random.uniform(l, h)
            trace(f"Generated new cached random for {key}: {self.rand_cache[key]}")
        self.used_rand_keys.add(key)
        return self.rand_cache[key]

    def validate_rule_keys(self, rule, sample_inst):
        for key in rule["default_keys"]:
            if self.get_val(self.default_config, key, "__MISSING__") == "__MISSING__":
                log(f"default_config key 'default_config.{key}' not found", "f_error")
                return False

        for key in rule["instance_keys"]:
            if self.get_val(sample_inst, key, "__MISSING__") == "__MISSING__":
                log(f"Instance key '{key}' not found", "f_error")
                return False
        return True

    def translate_condition(self, cond_str):
        c = cond_str.replace("AND", "and").replace("OR", "or")

        c = self.re_default_config.sub(r'__default__("\1")', c)

        def repl_bool(m):
            val_map = {"true": "True", "false": "False", "null": "None"}
            return f"(__get__('{m.group(1)}') == {val_map[m.group(2).lower()]})"
        c = self.re_bool_null_eq.sub(repl_bool, c)
        c = self.re_bool_null_colon.sub(repl_bool, c)

        for pattern, repl in self.regex_subs:
            c = pattern.sub(repl, c)

        def repl_range(m):
            path, low_str, high_str = m.group(1), m.group(2), m.group(3)
            return f"(__get_num__('{path}') > __get_rand__('{path}', {low_str}, {high_str}, '{m.group(0)}'))"
        c = self.re_range.sub(repl_range, c)

        c = self.re_num_cmp.sub(r'(__get_num__("\1") \2 \3)', c)
        c = self.re_num_eq.sub(lambda m: f"(__get_num__('{m.group(1)}') {'==' if m.group(2) == '=' else '!='} {m.group(3)})", c)
        c = self.re_num_neq.sub(r'(__get_num__("\1") != \2)', c)

        return c

    def evaluate_rule(self, inst, compiled_cond):
        eval_context = {
            '__get__': lambda path, d=None: self.get_val(inst, path, d),
            '__get_num__': lambda path: self.get_num(inst, path),
            '__tags__': inst.get("tags") or [],
            '__info_hash__': lambda: bytes(inst.get("torrent", {}).get("info_hash", [])).hex(),
            '__default__': lambda k: self.get_val(self.default_config, k),
            '__get_rand__': self.get_cached_rand
        }
        try:
            result = eval(compiled_cond, {"__builtins__": {"str": str, "float": float}}, eval_context)
            trace(f"Evaluated rule for instance ID {inst.get('id')}: result = {result}")
            return result
        except Exception as e:
            trace(f"Evaluation error for instance ID {inst.get('id')}: {e}")
            return False

    def is_action_valid(self, action, state):
        if action == "start": return (state == "Stopped")
        if action in ["stop", "pause"]: return (state == "Running")
        if action == "resume": return (state == "Paused")
        return action in ["update", "addtags", "removetags", "delete"]

    def _resolve_assignment_val(self, val_raw):
        val_raw = val_raw.strip()
        if val_raw.startswith("default_config."):
            return self.get_val(self.default_config, val_raw.replace("default_config.", ""))
        
        lower_val = val_raw.lower()
        if lower_val in ["true", "false", "null"]:
            return True if lower_val == "true" else False if lower_val == "false" else None

        if (val_raw.startswith('"') and val_raw.endswith('"')) or (val_raw.startswith("'") and val_raw.endswith("'")):
            return val_raw[1:-1]

        try:
            return float(val_raw) if "." in val_raw else int(val_raw)
        except ValueError:
            return val_raw

    def process_rules(self):
        trace("Starting process_rules cycle")
        instances_resp = self.api.request("GET", "instances")
        if not instances_resp: 
            trace("process_rules aborted: no response from instances endpoint")
            return

        with self.strike_lock:
            self.current_instances = instances_resp.get("data", [])

        if not self.current_instances:
            trace("process_rules: current_instances list is empty")
            return

        sample_inst = self.current_instances[0]

        for rule in self.rules_lines:
            trace(f"Evaluating rule line: {rule['raw']}")
            if not self.validate_rule_keys(rule, sample_inst):
                trace(f"Rule keys validation failed for rule: {rule['raw']}")
                continue

            for inst in self.current_instances:
                if inst.get("_deleted"):
                    continue

                state = self.get_val(inst, "stats.state")
                if not self.is_action_valid(rule["action"], state):
                    continue

                if TOR_KEEP_LAST and rule["action"] in ["stop", "delete"]:
                    announce = str(self.get_val(inst, "torrent.announce", "")).lower()
                    count = sum(1 for i in self.current_instances if not i.get("_deleted") and str(self.get_val(i, "torrent.announce", "")).lower() == announce)
                    if count <= 1:
                        trace(f"TOR_KEEP_LAST triggered: skipping action '{rule['action']}' for instance {inst.get('id')}.")
                        continue

                if self.evaluate_rule(inst, rule["compiled"]):
                    trace(f"Rule matched! Applying action '{rule['action']}' on instance ID {inst.get('id')}")                   
                    self.apply_action(inst, rule["action"], rule["assign"], rule["raw"])

    def apply_action(self, inst, action, assign, rule_line):
        id_ = inst.get("id")
        name = self.get_val(inst, "torrent.name")

        log(f"Rule applied: {rule_line}", "task")
        log(f"Torrent name : {name}", "f_data")

        if action == "addtags":
            tags_to_add = [t.strip() for t in assign.split(',') if t.strip()]
            existing_tags = inst.get("tags") or []
            new_tags = [t for t in tags_to_add if t not in existing_tags]

            if not new_tags: return
            payload = {"ids": [id_], "add_tags": new_tags, "remove_tags": []}
            if DRY_RUN:
                log(f"Would add tags to ID='{id_}' TAGS='{payload}'", "f_recycle")
            elif self.api.request("POST", "grid/tag", payload):
                inst["tags"] = list(set(existing_tags + new_tags))
                log(f"Tags added ({assign})", "f_succes")
            else:
                log(f"Failed to add tags ({assign}) for instance {id_}", "f_error")

        elif action == "removetags":
            tags_to_remove = [t.strip() for t in assign.split(',') if t.strip()]
            existing_tags = inst.get("tags") or []
            del_tags = [t for t in tags_to_remove if t in existing_tags]

            if not del_tags: return
            payload = {"ids": [id_], "add_tags": [], "remove_tags": del_tags}
            if DRY_RUN:
                log(f"Would remove tags from ID='{id_}' TAGS='{payload}'", "f_recycle")
            elif self.api.request("POST", "grid/tag", payload):
                inst["tags"] = [t for t in existing_tags if t not in del_tags]
                log(f"Tags removed ({assign})", "f_succes")
            else:
                log(f"Failed to remove tags ({assign}) for instance {id_}", "f_error")

        elif action == "update":
            parts = assign.split("=", 1)
            if len(parts) == 2 and parts[0].strip().startswith("config."):
                key = parts[0].replace("config.", "").strip()
                val = self._resolve_assignment_val(parts[1].strip().rstrip(";"))

                if val == "__MISSING__":
                    log(f"default_config key '{parts[1].strip()}' not found in defaults", "f_error")
                    return

                payload = copy.deepcopy(inst.get("config", {}))
                payload[key] = val

                if DRY_RUN:
                    log(f"Would patch config ID='{id_}' PAYLOAD='{payload}'", "f_recycle")
                elif self.api.request("PATCH", f"instances/{id_}/config", payload):
                    inst.setdefault("config", {})[key] = val
                    log(f"Patch succeeded ({assign})", "f_succes")
                else:
                    log("Patch failed", "f_error")
            else:
                log(f"update: invalid assign '{assign}'", "warning")

        elif action == "start":
            parts = assign.split("=", 1)
            if len(parts) < 2:
                log(f"update: invalid assign '{assign}'", "warning")
                return

            lhs = parts[0].strip()
            val = self._resolve_assignment_val(parts[1].strip().rstrip(";"))

            if val == "__MISSING__":
                log(f"default_config key '{parts[1].strip()}' not found in defaults", "f_error")
                return

            payload = {"torrent": copy.deepcopy(inst.get("torrent", {})), "config": copy.deepcopy(inst.get("config", {}))}
            if lhs.startswith("config."):
                payload["config"][lhs.replace("config.", "")] = val

            if DRY_RUN:
                log(f"Would start ID='{id_}'", "f_recycle")
            elif self.api.request("POST", f"faker/{id_}/start", payload):
                inst.setdefault("stats", {})["state"] = "Running"
                log("Start succeeded", "f_succes")

        elif action == "stop":
            if DRY_RUN:
                log(f"Would stop ID='{id_}'", "f_recycle")
            elif self.api.request("POST", f"faker/{id_}/stop"):
                inst.setdefault("stats", {})["state"] = "Stopped"
                log("Stop succeeded", "f_succes")

        elif action == "pause":
            if DRY_RUN:
                log(f"Would pause ID='{id_}'", "f_recycle")
            elif self.api.request("POST", "grid/pause", {"ids": [id_]}):
                inst.setdefault("stats", {})["state"] = "Paused"
                log("Pause succeeded", "f_succes")

        elif action == "resume":
            if DRY_RUN:
                log(f"Would resume ID='{id_}'", "f_recycle")
            elif self.api.request("POST", "grid/resume", {"ids": [id_]}):
                inst.setdefault("stats", {})["state"] = "Running"
                log("Resume succeeded", "f_succes")

        elif action == "delete":
            if "instance" in assign:
                if DRY_RUN:
                    log(f"Would delete ID='{id_}'", "f_recycle")
                elif self.api.request("DELETE", f"instances/{id_}?force=true"):
                    inst["_deleted"] = True
                    log("Delete succeeded", "f_succes")

            if "watchfile" in assign or "archive" in assign:
                hex_hash = bytes(inst.get("torrent", {}).get("info_hash", [])).hex()

                if hex_hash:
                    files_resp = self.api.request("GET", "watch/files")
                    files = files_resp.get("data", []) if files_resp else []														
                    for f in files:
                        if f.get("info_hash") == hex_hash:
                            filename = f.get("filename", "")
                            filepath = f.get("path", "")
                            
                            if "archive" in assign and ARCHIVE_FOLDER:
                                dest = os.path.join(ARCHIVE_FOLDER, os.path.basename(filename))
                                if not os.path.exists(dest):
                                    if DRY_RUN:
                                        log(f"Would archive file '{filename}' at '{filepath}'", "f_recycle")
                                    else:
                                        os.makedirs(ARCHIVE_FOLDER, exist_ok=True)
                                        try:
                                            shutil.copy2(filepath, dest)
                                            log(f"Torrent archived {dest}", "f_saving")
                                        except Exception:
                                            log(f"Failed to archive {filepath}", "f_error")

                            if "watchfile" in assign:
                                if DRY_RUN:
                                    log(f"Would delete file '{filename}' at '{filepath}'", "f_recycle")
                                else:
                                    encoded_path = urllib.parse.quote(filename)
                                    if self.api.request("DELETE", f"watch/files?path={encoded_path}"):
                                        log("Delete succeeded", "f_succes")
                                    else:
                                        log("Failed to delete file", "f_error")

        for key in self.used_rand_keys:
            self.rand_cache.pop(key, None)
        self.used_rand_keys.clear()

    def logs_watcher_thread(self):
        url = f"{self.api.base_url}/api/logs"
        trace(f"Starting logs_watcher_thread targeting SSE URL: {url}")
        headers = {
            "Accept": "text/event-stream",
            "Cache-Control": "no-cache",
            "Connection": "keep-alive",
            "X-Accel-Buffering": "no"
        }
        while True:
            try:
                with requests.get(url, stream=True, headers=headers, timeout=60) as r:
                    for line in r.iter_lines(decode_unicode=True):
                        self._check_expirations()
                        if line and line.startswith("data:"):
                            self._handle_log_event(line[5:].strip())
            except requests.exceptions.Timeout:
                trace("logs_watcher_thread timeout reached, checking expirations")
                self._check_expirations()
            except Exception as e:
                trace(f"logs_watcher_thread exception encountered: {e}")
                time.sleep(5)

    def _handle_log_event(self, json_str):
        trace(f"Handling log event payload: {json_str}")
        try:
            event = json.loads(json_str)
            level = event.get("level", "").lower()
            msg = event.get("message", "")

            if "error" in level and "[" in msg:
                tag, rest = self._extract_bracket(msg)
                if tag:
                    with self.strike_lock:
                        now = time.time()
                        state = self.logs_state.setdefault(tag, {"counts": {}, "action": 0, "last_count_time": 0})

                        counts = state.setdefault("counts", {})
                        counts[rest] = counts.get(rest, 0) + 1
                        current_count = counts[rest]
                        state["last_count_time"] = now

                        if state["action"] == 0 and current_count >= WATCHER_MAX_STRIKE:
                            self._trigger_watcher_pause(tag, rest, now)
                            state["action"] = now

                        self.save_logs_state()
        except Exception as e:
            log(f"Error: {str(e)}", "error")

    def _find_instance_by_name(self, name):
        with self.strike_lock:
            for inst in self.current_instances:
                if self.get_val(inst, "torrent.name") == name:
                    return inst
        return None

    def _trigger_watcher_pause(self, tag, rest, action_ts):
        inst = self._find_instance_by_name(tag)
        if not inst: return

        state = self.get_val(inst, "stats.state")
        if self.is_action_valid("pause", state):
            pause_time_str = self.format_time_bash_style(WATCHER_PAUSE_TIME)
            log(f"Repeated error detected (x{WATCHER_MAX_STRIKE}). Try to pause for {pause_time_str} and add tag", "warning")
            log(f"Torrent name : {tag}", "f_data")
            log(f"{rest}", "f_data")

            id_ = inst.get("id")
            err_tag = f"Err {self.get_elapsed_since_midnight_tag(action_ts)}"

            if DRY_RUN:
                log(f"Would pause '{tag}'", "f_recycle")
                log(f"Would addtags '{err_tag}'", "f_recycle")
            else:
                if self.api.request("POST", "grid/pause", {"ids": [id_]}):
                    log("Pause succeeded", "f_succes")

                    existing_tags = inst.get("tags") or []
                    if err_tag not in existing_tags:
                        self.api.request("POST", "grid/tag", {"ids": [id_], "add_tags": [err_tag], "remove_tags": []})
                        log(f"Tags applied ({err_tag})", "f_succes")

    def _check_expirations(self):
        trace("Checking log expirations and purges")
        with self.strike_lock:
            now = time.time()
            dirty = False
            expired_tags = []

            active_names = {self.get_val(inst, "torrent.name") for inst in self.current_instances}
            for tag, state in list(self.logs_state.items()):
                if tag not in active_names:
                    expired_tags.append(tag)
                    continue

                if state.get("last_count_time", 0) > 0 and (now - state["last_count_time"]) > WATCHER_STRIKE_TIME:
                    if state.get("action", 0) == 0:
                        expired_tags.append(tag)

                if state.get("action", 0) > 0 and (now - state["action"]) > WATCHER_PAUSE_TIME:
                    self._trigger_watcher_resume(tag, state["action"])
                    expired_tags.append(tag)

            for tag in expired_tags:
                if tag in self.logs_state:
                    del self.logs_state[tag]
                    dirty = True

            if dirty:
                self.save_logs_state()

    def _trigger_watcher_resume(self, tag, action_ts):
        inst = self._find_instance_by_name(tag)
        if not inst: return

        state = self.get_val(inst, "stats.state")
        if self.is_action_valid("resume", state):
            log("Pause ended. Try to resume and remove tag", "warning")
            log(f"Torrent name : {tag}", "f_data")

            id_ = inst.get("id")
            err_tag = f"Err {self.get_elapsed_since_midnight_tag(action_ts)}"

            if DRY_RUN:
                log(f"Would resume '{tag}'", "f_recycle")
                log(f"Would removetags '{err_tag}'", "f_recycle")
            else:
                if self.api.request("POST", "grid/resume", {"ids": [id_]}):
                    log("Resume succeeded", "f_succes")

                    existing_tags = inst.get("tags") or []
                    if err_tag in existing_tags:
                        self.api.request("POST", "grid/tag", {"ids": [id_], "add_tags": [], "remove_tags": [err_tag]})
                        log(f"Tags removed ({err_tag})", "f_succes")

    def run(self):
        initial_interval = 5
        log(f"Starting RustatioManager daemon (TRACE mode: {'ENABLED' if TRACE else 'DISABLED'})", "start")
        self.load_configs()

        interval = REFRESH_INTERVAL
        if interval == 0 and self.default_config:
            interval = int(self.get_val(self.default_config, "scrape_interval", 60))

        interval = max(0, interval - initial_interval)

        log(f"REFRESH INTERVAL : {interval + initial_interval}s", "data")

        if LOGS_WATCHER:
            threading.Thread(target=self.logs_watcher_thread, daemon=True).start()

        while True:
            try:
                resp = requests.get(f"{RUSTATIO_API}/health", timeout=3)
                if resp.text == "OK": break
            except Exception:
                pass
            log(f"Rustatio not ready. Retry in {initial_interval}s", "warning")
            time.sleep(initial_interval)

        while True:
            time.sleep(initial_interval)

            if LOGFILE and LOGFILE != "/dev/null" and not os.path.exists(LOGFILE):
                setup_file_handler()
                log("Log recreated automatically", "start")
                self.load_configs()

            try:
                self.process_rules()
            except Exception as e:
                log(f"Error in process_rules: {str(e)}", "error")
            time.sleep(interval)

if __name__ == "__main__":
    manager = RustatioManager()
    manager.run()
