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
import datetime
import ast
import operator
from collections import defaultdict
from flask import Flask, request, jsonify, render_template_string

app = Flask(__name__)

# ==========================================
# CONFIGURATION
# ==========================================
PORT = int(os.environ.get("PORT", 8080))
ADMIN_PORT = PORT + 1
RUSTATIO_API = os.environ.get("RUSTATIO_API", f"http://127.0.0.1:{PORT}")
AUTH_TOKEN = os.environ.get("AUTH_TOKEN", "")
REFRESH_INTERVAL = int(os.environ.get("REFRESH_INTERVAL", 0))
ARCHIVE_FOLDER = os.environ.get("ARCHIVE_FOLDER", "/data/archived")
RULES_FILE = os.environ.get("RULES_FILE", "/data/rules.txt")
DEFAULTS_FILE = os.environ.get("DEFAULTS_FILE", "/data/state.json")
DRY_RUN = os.environ.get("DRY_RUN", "false").lower() == "true"
LOGFILE = os.environ.get("LOGFILE", "/data/rustatio_daemon.log")
CHECK_LOGS_FILE = os.environ.get("CHECK_LOGS_FILE", os.path.join(os.path.dirname(RULES_FILE), "check_logs.json"))

LOGS_WATCHER = int(os.environ.get("LOGS_WATCHER", 1))
WATCHER_MAX_STRIKE = int(os.environ.get("WATCHER_MAX_STRIKE", 3))
WATCHER_STRIKE_TIME = int(os.environ.get("WATCHER_STRIKE_TIME", 3600))
WATCHER_PAUSE_TIME = int(os.environ.get("WATCHER_PAUSE_TIME", 3600))
TOR_KEEP_LAST = int(os.environ.get("TOR_KEEP_LAST", 1))

# ==========================================
# HTML TEMPLATE (ADMIN PANEL)
# ==========================================
HTML_TEMPLATE = """
<!DOCTYPE html>
<html lang="fr">
<head>
    <meta charset="UTF-8">
    <title>Rustatio - Panneau de Contrôle</title>
    <style>
        :root {
            --bg-color: #121212;
            --panel-bg: #1e1e1e;
            --section-bg: #262626;
            --text-main: #e0e0e0;
            --rust-orange: #ce412b;
            --rust-orange-hover: #e84d35;
            --border-color: #3d3d3d;
            --success: #4caf50;
            --danger: #f44336;
        }
        body {
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            background-color: var(--bg-color);
            color: var(--text-main);
            margin: 0;
            padding: 20px;
            display: flex;
            flex-direction: column;
            align-items: center;
        }
        .container {
            width: 100%;
            max-width: 1450px;
            display: flex;
            flex-direction: column;
            gap: 20px;
        }
        .header {
            width: 100%;
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 15px 20px;
            background: var(--panel-bg);
            border: 1px solid var(--border-color);
            border-radius: 8px;
            box-sizing: border-box;
        }
        h1, h2 { margin: 0; color: var(--rust-orange); }
        .panel {
            background: var(--panel-bg);
            border: 1px solid var(--border-color);
            border-radius: 8px;
            padding: 20px;
            display: flex;
            flex-direction: column;
        }
        .controls { display: flex; gap: 10px; align-items: center; }
        button {
            background-color: var(--rust-orange);
            color: white;
            border: none;
            padding: 10px 15px;
            border-radius: 4px;
            cursor: pointer;
            font-weight: bold;
            transition: background 0.2s, transform 0.1s;
        }
        button:hover { background-color: var(--rust-orange-hover); }
        button.stop { background-color: var(--danger); }
        button.start { background-color: var(--success); }
        button.small { padding: 4px 8px; font-size: 0.8em; }
        button:disabled {
            opacity: 0.4;
            cursor: not-allowed;
            filter: grayscale(100%);
            pointer-events: none;
        }
        .rule-row {
            display: flex;
            gap: 12px;
            align-items: stretch;
            background: #181818;
            padding: 12px;
            border-radius: 8px;
            margin-bottom: 15px;
            border: 1px solid var(--border-color);
            position: relative;
        }
        .rule-block {
            background: var(--section-bg);
            border: 1px solid var(--border-color);
            border-radius: 6px;
            padding: 10px;
            display: flex;
            flex-direction: column;
            gap: 8px;
        }
        .rule-block-title {
            font-size: 0.75em;
            font-weight: bold;
            letter-spacing: 1px;
            color: #888;
            text-transform: uppercase;
            border-bottom: 1px solid #333;
            padding-bottom: 4px;
            margin-bottom: 2px;
        }
        .block-conditions { flex: 3; }
        .block-action { flex: 1; min-width: 160px; }
        .block-assign { flex: 2; min-width: 220px; }
        .rule-arrow {
            display: flex;
            align-items: center;
            justify-content: center;
            color: var(--rust-orange);
            font-size: 1.4em;
            font-weight: bold;
            user-select: none;
        }
        .conditions-wrapper {
            display: flex;
            flex-direction: column;
            gap: 8px;
        }
        .condition-block {
            display: flex;
            gap: 6px;
            align-items: center;
            background: #1e1e1e;
            padding: 6px;
            border-radius: 4px;
            border: 1px solid #333;
        }
        select, input[type="text"], input[type="number"] {
            background: #0d0d0d;
            color: #fff;
            border: 1px solid var(--border-color);
            padding: 7px;
            border-radius: 4px;
            font-size: 0.9em;
        }
        select:focus, input:focus { outline: 1px solid var(--rust-orange); }
        .cond-logop { font-weight: bold; color: var(--rust-orange); border-color: var(--rust-orange); }
        input[type="text"], input[type="number"] { flex-grow: 1; min-width: 100px; }
        .action-type {
            font-weight: bold;
            text-transform: uppercase;
            padding: 8px;
            border-radius: 4px;
            cursor: pointer;
        }
        .action-type[data-action="start"] { background-color: #1b5e20; color: #a5d6a7; border-color: #2e7d32; }
        .action-type[data-action="stop"] { background-color: #b71c1c; color: #ffcdd2; border-color: #c62828; }
        .action-type[data-action="pause"] { background-color: #e65100; color: #ffe0b2; border-color: #f57c00; }
        .action-type[data-action="resume"] { background-color: #0d47a1; color: #bbdefb; border-color: #1565c0; }
        .action-type[data-action="delete"] { background-color: #4a148c; color: #e1bee7; border-color: #6a1b9a; }
        .action-type[data-action="update"] { background-color: #004d40; color: #b2dfdb; border-color: #00695c; }
        .action-type[data-action="addtags"] { background-color: #006064; color: #b2ebf2; border-color: #00838f; }
        .action-type[data-action="removetags"] { background-color: #4e342e; color: #d7ccc8; border-color: #6d4c41; }
        .btn-delete-rule {
            align-self: center;
            background: #333;
            color: #ff6b6b;
            border: 1px solid #555;
            border-radius: 50%;
            width: 28px;
            height: 28px;
            padding: 0;
            display: flex;
            align-items: center;
            justify-content: center;
            cursor: pointer;
        }
        .btn-delete-rule:hover { background: var(--danger); color: white; }
        .raw-editor-container { margin-top: 15px; display: none; }
        textarea {
            width: 100%; height: 150px; background: #000; color: #fff;
            border: 1px solid var(--border-color); padding: 10px;
            font-family: monospace; resize: vertical; box-sizing: border-box;
        }
        #logs {
            width: 100%; height: 400px; background: #000; color: #a5d6a7;
            border: 1px solid var(--border-color); padding: 10px;
            font-family: monospace; overflow-y: scroll; box-sizing: border-box;
            white-space: pre-wrap;
        }
        .status-badge {
            padding: 5px 10px; border-radius: 12px; font-size: 0.9em; font-weight: bold;
        }
        .status-running { background: rgba(76, 175, 80, 0.2); color: var(--success); }
        .status-stopped { background: rgba(244, 67, 54, 0.2); color: var(--danger); }
        .refresh-spinner {
            display: inline-block;
            width: 12px;
            height: 12px;
            border: 2px solid rgba(206, 65, 43, 0.3);
            border-radius: 50%;
            border-top-color: var(--rust-orange);
            animation: spin 0.8s linear infinite;
            opacity: 0;
            transition: opacity 0.2s ease;
            margin-left: 8px;
            vertical-align: middle;
        }
        .refresh-spinner.active {
            opacity: 1;
        }
        .refresh-slot {
            position: relative;
            display: inline-flex;
            align-items: center;
            justify-content: center;
            width: 1cm;
            height: 16px;
            margin: 0 8px;
            flex-shrink: 0;
        }
        .progress-container {
            width: 100%;
            height: 4px;
            background-color: #2a2a2a;
            border-radius: 2px;
            overflow: hidden;
            transition: opacity 0.2s ease;
        }
        .progress-bar {
            height: 100%;
            width: 0%;
            background-color: var(--rust-orange);
        }
        .refresh-slot .refresh-spinner {
            position: absolute;
            opacity: 0;
            transition: opacity 0.2s ease;
            pointer-events: none;
        }
        .refresh-slot .refresh-spinner.active {
            opacity: 1;
        }
        .refresh-slot:has(.refresh-spinner.active) .progress-container {
            opacity: 0;
        }
        @keyframes spin {
            to { transform: rotate(360deg); }
        }
        .collapsible-header {
            cursor: pointer;
            user-select: none;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }
        .collapse-icon {
            display: inline-block;
            transition: transform 0.2s ease;
            margin-right: 8px;
            font-size: 0.8em;
            color: var(--rust-orange);
        }
        .panel.collapsed .collapse-content {
            display: none !important;
        }
    </style>
</head>
<body>
    <div class="container">
        <div class="panel collapsed" id="panel-control">
            <div class="collapsible-header" onclick="togglePanel('panel-control')">
                <h2 style="display: flex; align-items: center;">
                    <span class="collapse-icon">▶</span> ⚙️ Rustatio Control
                </h2>
                <div class="controls" onclick="event.stopPropagation()">
                    <span id="daemon-status" class="status-badge status-stopped">Vérification...</span>
                    <button id="btn-daemon-start" class="start" onclick="daemonAction('start')">▶ Démarrer Daemon</button>
                    <button id="btn-daemon-stop" class="stop" onclick="daemonAction('stop')">⏹ Arrêter Daemon</button>
                    <button id="btn-daemon-restart" onclick="daemonAction('restart')">🔄 Redémarrer Daemon</button>
                    <button style="background-color: #555;" onclick="restartAdmin()">♻️ Redémarrer Script</button>
                </div>
            </div>

            <div class="collapse-content" style="margin-top: 15px;">
                <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 10px;">
                    <span style="font-size: 0.8em; font-weight: bold; color: #888; text-transform: uppercase; letter-spacing: 1px;">
                        🔧 Configuration de l'environnement (Modifications temporaires)
                    </span>
                    <div style="display: flex; gap: 8px;">
                        <button onclick="resetEnvConfig()" class="small" style="background: #444;">🔄 Réinitialiser</button>
                        <button onclick="saveEnvConfig()" class="small" style="background: var(--rust-orange);">⚡ Appliquer à la session</button>
                    </div>
                </div>
                <div id="env-config-grid" style="display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 10px;">
                    <div style="color: #666; font-size: 0.85em;">Chargement des variables...</div>
                </div>
            </div>
        </div>

        <div class="panel collapsed" id="panel-watcher">
            <div class="collapsible-header" onclick="togglePanel('panel-watcher')">
                <h2 style="display: flex; align-items: center;">
                    <span class="collapse-icon">▶</span> 👁️ Log Watcher
                    <div class="refresh-slot">
                        <div class="progress-container">
                            <div id="watcher-progress-bar" class="progress-bar"></div>
                        </div>
                        <span id="watcher-spinner" class="refresh-spinner" title="Mise à jour..."></span>
                    </div>
                </h2>
                <div class="controls" onclick="event.stopPropagation()">
                    <span id="watcher-status" class="status-badge status-stopped">Vérification...</span>
                    <button id="btn-watcher-start" class="start" onclick="watcherAction('start')">▶ Démarrer</button>
                    <button id="btn-watcher-stop" class="stop" onclick="watcherAction('stop')">⏹ Arrêter</button>
                    <button id="btn-watcher-restart" onclick="watcherAction('restart')">🔄 Redémarrer</button>
                </div>
            </div>

            <div class="collapse-content" style="margin-top: 15px;">
                <div style="background: #181818; border: 1px solid var(--border-color); border-radius: 8px; padding: 10px; overflow-x: auto;">
                    <table style="width: 100%; border-collapse: collapse; font-size: 0.95em; text-align: left;">
                        <thead>
                            <tr style="border-bottom: 1px solid var(--border-color); color: #888;">
                                <th style="padding: 10px;">Torrent (Tag)</th>
                                <th style="padding: 10px;">Statut</th>
                                <th style="padding: 10px;">Détails (Erreurs / Strikes)</th>
                                <th style="padding: 10px;">Temps restant</th>
                            </tr>
                        </thead>
                        <tbody id="watcher-state-body">
                            <tr><td colspan="4" style="padding: 15px; text-align: center; color: #666;">Aucune erreur en cours ...</td></tr>
                        </tbody>
                    </table>
                </div>
            </div>
        </div>

        <div class="panel collapsed" id="panel-rules">
            <div class="collapsible-header" onclick="togglePanel('panel-rules')">
                <h2 style="display: flex; align-items: center;">
                    <span class="collapse-icon">▶</span> 📝 Éditeur de Règles ({{ rules_filename }})
                </h2>
            </div>
            <div class="collapse-content" style="margin-top: 10px;">
                <p style="font-size: 0.85em; color: #888;">Gestion visuelle structurée : Condition(s) ➔ Action ➔ Assignation/Paramètres.</p>
                <div id="visual-rules-container"></div>
                <div style="display: flex; gap: 10px; margin-top: 10px;">
                    <button onclick="addRuleRow()">➕ Ajouter une règle</button>
                    <button onclick="toggleRawEditor()" class="small" style="background: #555;">🔄 Vue Texte Brut</button>
                    <button onclick="saveRules()" style="margin-left: auto;">💾 Sauvegarder les règles</button>
                </div>
                <div class="raw-editor-container" id="raw-editor-container">
                    <p style="font-size: 0.85em; color: #e84d35;">Éditeur manuel (Format : condition | action | assignation)</p>
                    <textarea id="rules-editor" onchange="parseTextToVisual()"></textarea>
                </div>
            </div>
        </div>

        <div class="panel">
            <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 10px;">
                <h2 style="display: flex; align-items: center;">
                    🔍 Logs ({{ logfile_filename }})
                    <div class="refresh-slot">
                        <div class="progress-container">
                            <div id="log-progress-bar" class="progress-bar"></div>
                        </div>
                        <span id="log-spinner" class="refresh-spinner" title="Mise à jour..."></span>
                    </div>
                </h2>
                <div style="display: flex; gap: 8px; align-items: center;">
                    <button onclick="clearLogs()" class="small stop">Vider</button>
                    <button onclick="archiveLogs()" class="small" style="background-color: #2e7d32;">📁 Archiver</button>
                    <button onclick="fetchLogs()" class="small">Rafraîchir</button>
                    <label for="log-lines" style="font-size: 0.85em; color: #888;">Lignes :</label>
                    <select id="log-lines" onchange="fetchLogs()" style="padding: 3px 6px; font-size: 0.85em;">
                        <option value="50">50</option>
                        <option value="100" selected>100</option>
                        <option value="200">200</option>
                        <option value="500">500</option>
                        <option value="1000">1000</option>
                    </select>
                </div>
            </div>
            <div id="logs"></div>
        </div>

    <datalist id="default-config-list">
        <option value="default_config.upload_rate"></option>
        <option value="default_config.download_rate"></option>
        <option value="default_config.port"></option>
        <option value="default_config.vpn_port_sync"></option>
        <option value="default_config.client_type"></option>
        <option value="default_config.client_version"></option>
        <option value="default_config.initial_uploaded"></option>
        <option value="default_config.initial_downloaded"></option>
        <option value="default_config.completion_percent"></option>
        <option value="default_config.num_want"></option>
        <option value="default_config.randomize_rates"></option>
        <option value="default_config.random_range_percent"></option>
        <option value="default_config.randomize_ratio"></option>
        <option value="default_config.random_ratio_range_percent"></option>
        <option value="default_config.stop_at_ratio"></option>
        <option value="default_config.effective_stop_at_ratio"></option>
        <option value="default_config.stop_at_uploaded"></option>
        <option value="default_config.stop_at_downloaded"></option>
        <option value="default_config.stop_at_seed_time"></option>
        <option value="default_config.idle_when_no_leechers"></option>
        <option value="default_config.idle_when_no_seeders"></option>
        <option value="default_config.scrape_interval"></option>
        <option value="default_config.progressive_rates"></option>
        <option value="default_config.target_upload_rate"></option>
        <option value="default_config.target_download_rate"></option>
        <option value="default_config.progressive_duration"></option>
        <option value="default_config.post_stop_action"></option>
    </datalist>

    <script>
        const FIELDS = [
            "id", "torrent.info_hash", "torrent.announce", "torrent.name", "torrent.total_size",
            "torrent.piece_length", "torrent.num_pieces", "torrent.comment", "torrent.created_by",
            "torrent.is_single_file", "torrent.file_count", 
            "config.upload_rate", "config.download_rate", "config.port", "config.vpn_port_sync",
            "config.client_type", "config.client_version", "config.initial_uploaded", "config.initial_downloaded",
            "config.completion_percent", "config.num_want", "config.randomize_rates", "config.random_range_percent",
            "config.randomize_ratio", "config.random_ratio_range_percent", "config.stop_at_ratio",
            "config.effective_stop_at_ratio", "config.stop_at_uploaded", "config.stop_at_downloaded",
            "config.stop_at_seed_time", "config.idle_when_no_leechers", "config.idle_when_no_seeders",
            "config.scrape_interval", "config.progressive_rates", "config.target_upload_rate",
            "config.target_download_rate", "config.progressive_duration", "config.post_stop_action",
            "stats.uploaded", "stats.downloaded", "stats.ratio", "stats.left", "stats.torrent_completion",
            "stats.seeders", "stats.leechers", "stats.state", "stats.is_idling", "stats.idling_reason",
            "stats.session_uploaded", "stats.session_downloaded", "stats.session_ratio",
            "stats.current_upload_rate", "stats.current_download_rate", "stats.average_upload_rate",
            "stats.average_download_rate", "stats.upload_progress", "stats.download_progress",
            "stats.ratio_progress", "stats.seed_time_progress", "stats.effective_stop_at_ratio",
            "stats.eta_ratio", "stats.eta_uploaded", "stats.eta_download_completion",
            "stats.stop_condition_met", "stats.post_stop_action", "stats.stats.announce_count",
            "tags", "default_config.upload_rate", "default_config.download_rate"
        ];
        
        const CONFIG_TYPES = {
            "config.upload_rate": "number", "config.download_rate": "number", "config.port": "number",
            "config.vpn_port_sync": "boolean", "config.client_type": "string", "config.client_version": "string",
            "config.initial_uploaded": "number", "config.initial_downloaded": "number",
            "config.completion_percent": "number", "config.num_want": "number", "config.randomize_rates": "boolean",
            "config.random_range_percent": "number", "config.randomize_ratio": "boolean",
            "config.random_ratio_range_percent": "number", "config.stop_at_ratio": "number",
            "config.effective_stop_at_ratio": "number", "config.stop_at_uploaded": "number",
            "config.stop_at_downloaded": "number", "config.stop_at_seed_time": "number",
            "config.idle_when_no_leechers": "boolean", "config.idle_when_no_seeders": "boolean",
            "config.scrape_interval": "number", "config.progressive_rates": "boolean",
            "config.target_upload_rate": "number", "config.target_download_rate": "number",
            "config.progressive_duration": "number", "config.post_stop_action": "string"
        };
        
        const OPERATORS = [":", "=", "<", ">", "!=", "<=", ">=", "~"];
        const ACTIONS = ["start", "stop", "pause", "resume", "delete", "update", "addtags", "removetags"];
        const LOGICAL_OPS = ["AND", "OR"];

        let currentReadonlyKeys = [];

        function createOptions(arr, selected = '') {
            return arr.map(item => `<option value="${item}" ${item === selected ? 'selected' : ''}>${item}</option>`).join('');
        }
        
        function createConfigOptions(selected = '') {
            return Object.keys(CONFIG_TYPES).map(k => `<option value="${k}" ${k === selected ? 'selected' : ''}>${k}</option>`).join('');
        }

        function getActionValueInput(configKey, val) {
            return `<input type="text" class="action-assign-val" list="default-config-list" placeholder="Valeur ou default_config..." value="${val.replace(/"/g, '&quot;')}">`;
        }

        function applyActionStyle(selectElem) {
            const action = selectElem.value;
            selectElem.setAttribute('data-action', action);
        }

        function updateActionUI(selectElem) {
            applyActionStyle(selectElem);
            const action = selectElem.value;
            const row = selectElem.closest('.rule-row');
            const container = row.querySelector('.action-assign-container');
            const assignBlock = row.querySelector('.block-assign');
            const assignArrow = row.querySelector('.arrow-assign');

            if (['start', 'stop', 'pause', 'resume'].includes(action)) {
                container.innerHTML = '<span style="color: #666; font-size: 0.85em; italic;">Aucun paramètre</span>';
                assignBlock.style.opacity = '0.5';
                if (assignArrow) assignArrow.style.opacity = '0.3';
            } else {
                assignBlock.style.opacity = '1';
                if (assignArrow) assignArrow.style.opacity = '1';

                if (action === 'update') {
                    const confKey = 'config.upload_rate';
                    container.innerHTML = `
                        <select class="action-assign-conf" style="flex-grow:1;">${createConfigOptions(confKey)}</select>
                        <span style="color:#888; font-weight:bold; align-self:center;">=</span>
                        ${getActionValueInput(confKey, '')}
                    `;
                } else {
                    container.innerHTML = `<input type="text" class="action-assign-text" placeholder="Paramètres (ex: étiquette1, étiquette2)" value="">`;
                }
            }
        }

        function renderActionAssignContainer(action, assignVal) {
            if (['start', 'stop', 'pause', 'resume'].includes(action)) {
                return '<span style="color: #666; font-size: 0.85em; italic;">Aucun paramètre</span>';
            } else if (action === 'update') {
                let confKey = 'config.upload_rate';
                let val = '';
                if (assignVal) {
                    const parts = assignVal.split('=');
                    if (parts.length >= 2) {
                        confKey = parts[0].trim();
                        val = parts.slice(1).join('=').trim();
                    }
                }
                if (!CONFIG_TYPES[confKey]) confKey = 'config.upload_rate';
                
                return `
                    <select class="action-assign-conf" style="flex-grow:1;">${createConfigOptions(confKey)}</select>
                    <span style="color:#888; font-weight:bold; align-self:center;">=</span>
                    ${getActionValueInput(confKey, val)}
                `;
            } else {
                return `<input type="text" class="action-assign-text" placeholder="Paramètres (ex: étiquette1, étiquette2)" value="${(assignVal||'').replace(/"/g, '&quot;')}">`;
            }
        }

        function addCondition(container, logOp='', fieldVal='', opVal=':', condVal='') {
            const block = document.createElement('div');
            block.className = 'condition-block';
            let logOpHtml = '';
            if (container.children.length > 0) {
                logOpHtml = `<select class="cond-logop">${createOptions(LOGICAL_OPS, logOp || 'AND')}</select>`;
            }
            const placeholder = opVal === ':' ? "Ex: 1.8 - 2.4" : "Valeur";

            block.innerHTML = `
                ${logOpHtml}
                <select class="cond-field">${createOptions(FIELDS, fieldVal)}</select>
                <select class="cond-op">${createOptions(OPERATORS, opVal)}</select>
                <input type="text" class="cond-val" placeholder="${placeholder}" value="${condVal.replace(/"/g, '&quot;')}">
                ${container.children.length > 0 ? `<button class="stop small" onclick="this.parentElement.remove()" title="Supprimer">✕</button>` : ''}
            `;
            container.appendChild(block);
        }

        function addRuleRow(conditions = [], actionVal='start', assignVal='') {
            const container = document.getElementById('visual-rules-container');
            const row = document.createElement('div');
            row.className = 'rule-row';
            const isSimpleAction = ['start', 'stop', 'pause', 'resume'].includes(actionVal);

            row.innerHTML = `
                <div class="rule-block block-conditions">
                    <div class="rule-block-title">1. Condition(s)</div>
                    <div class="conditions-wrapper"></div>
                    <button class="small" onclick="addCondition(this.previousElementSibling)" style="align-self: flex-start; margin-top: 4px;">➕ Condition</button>
                </div>
                <div class="rule-arrow">➔</div>
                <div class="rule-block block-action">
                    <div class="rule-block-title">2. Action</div>
                    <select class="action-type" data-action="${actionVal}" onchange="updateActionUI(this)">${createOptions(ACTIONS, actionVal)}</select>
                </div>
                <div class="rule-arrow arrow-assign" style="${isSimpleAction ? 'opacity: 0.3;' : ''}">➔</div>
                <div class="rule-block block-assign" style="${isSimpleAction ? 'opacity: 0.5;' : ''}">
                    <div class="rule-block-title">3. Paramètres / Assignation</div>
                    <div class="action-assign-container" style="display: flex; gap: 6px; align-items: center; flex-grow: 1;">
                        ${renderActionAssignContainer(actionVal, assignVal)}
                    </div>
                </div>
                <button class="btn-delete-rule" onclick="this.closest('.rule-row').remove()" title="Supprimer la règle">✕</button>
            `;
            
            const condWrapper = row.querySelector('.conditions-wrapper');
            if (conditions.length === 0) {
                addCondition(condWrapper);
            } else {
                conditions.forEach(c => addCondition(condWrapper, c.logOp, c.field, c.op, c.val));
            }
            container.appendChild(row);
        }

        function parseVisualToText() {
            const rows = document.querySelectorAll('.rule-row');
            let text = [];
            rows.forEach(row => {
                const condBlocks = row.querySelectorAll('.condition-block');
                let condString = '';
                condBlocks.forEach((block, index) => {
                    const field = block.querySelector('.cond-field').value;
                    const op = block.querySelector('.cond-op').value;
                    const val = block.querySelector('.cond-val').value;
                    if (index > 0) condString += ` ${block.querySelector('.cond-logop').value} `;
                    condString += op === ':' ? `${field}: ${val}` : `${field} ${op} ${val}`;
                });
                
                const action = row.querySelector('.action-type').value;
                let assign = '';
                const assignContainer = row.querySelector('.action-assign-container');
                if (action === 'update') {
                    const conf = assignContainer.querySelector('.action-assign-conf')?.value;
                    const val = assignContainer.querySelector('.action-assign-val')?.value;
                    if (conf && val !== undefined) assign = `${conf} = ${val}`;
                } else if (!['start', 'stop', 'pause', 'resume'].includes(action)) {
                    const txt = assignContainer.querySelector('.action-assign-text');
                    if (txt) assign = txt.value;
                }
                
                if (condString && action) {
                    let line = `${condString} | ${action} |`;
                    if (assign) line += ` ${assign}`;
                    text.push(line);
                }
            });
            document.getElementById('rules-editor').value = text.join('\\n');
        }

        function parseTextToVisual() {
            const container = document.getElementById('visual-rules-container');
            container.innerHTML = '';
            const text = document.getElementById('rules-editor').value;
            const lines = text.split('\\n');
            const condRegex = /^([a-zA-Z0-9_.]+)\\s*(<=|>=|!=|:|=|<|>|~)\\s*(.*)$/;
            
            lines.forEach(line => {
                const trimmed = line.trim();
                if (!trimmed || trimmed.startsWith('#')) return; 
                
                const parts = trimmed.split('|').map(p => p.trim());
                if (parts.length >= 2) {
                    const condString = parts[0];
                    const action = parts[1] || 'start';
                    const assign = parts.length >= 3 ? parts[2] : '';
                    const tokens = condString.split(/\\s+(AND|OR|and|or)\\s+/);
                    const conditions = [];
                    let currentLogOp = '';
                    
                    tokens.forEach(token => {
                        const t = token.trim();
                        if (/^(AND|OR)$/i.test(t)) {
                            currentLogOp = t.toUpperCase();
                        } else if (t) {
                            const match = t.match(condRegex);
                            if (match) {
                                conditions.push({ logOp: currentLogOp, field: match[1], op: match[2], val: match[3] });
                            } else {
                                conditions.push({ logOp: currentLogOp, field: t, op: ':', val: '' });
                            }
                            currentLogOp = '';
                        }
                    });
                    addRuleRow(conditions, action, assign);
                }
            });
            if (container.children.length === 0) addRuleRow();
        }

        function resetAndStartProgress(barId, durationMs) {
            const bar = document.getElementById(barId);
            if (!bar) return;
            bar.style.transition = 'none';
            bar.style.width = '0%';
            void bar.offsetWidth;
            bar.style.transition = `width ${durationMs}ms linear`;
            bar.style.width = '100%';
        }

        function toggleRawEditor() {
            const raw = document.getElementById('raw-editor-container');
            if (raw.style.display === 'block') {
                raw.style.display = 'none';
                parseTextToVisual();
            } else {
                parseVisualToText();
                raw.style.display = 'block';
            }
        }
        
        function togglePanel(panelId) {
            const panel = document.getElementById(panelId);
            if (!panel) return;
            const icon = panel.querySelector('.collapse-icon');
            panel.classList.toggle('collapsed');
            if (icon) {
                icon.textContent = panel.classList.contains('collapsed') ? '▶' : '▼';
            }
        }

        async function fetchStatus() {
            const badge = document.getElementById('daemon-status');
            const btnStart = document.getElementById('btn-daemon-start');
            const btnStop = document.getElementById('btn-daemon-stop');
            const btnRestart = document.getElementById('btn-daemon-restart');
            try {
                const res = await fetch('/api/status');
                if (!res.ok) throw new Error('Erreur HTTP ' + res.status);
                const data = await res.json();
                const isRunning = !!data.running;
                if (isRunning) {
                    badge.className = 'status-badge status-running';
                    badge.innerText = "En cours d'exécution (PID: " + data.pid + ")";
                } else {
                    badge.className = 'status-badge status-stopped';
                    badge.innerText = 'Arrêté';
                }
                if (btnStart) btnStart.disabled = isRunning;
                if (btnStop) btnStop.disabled = !isRunning;
                if (btnRestart) btnRestart.disabled = !isRunning;
            } catch (err) {
                badge.className = 'status-badge status-stopped';
                badge.innerText = 'Erreur de connexion';
                if (btnStart) btnStart.disabled = false;
                if (btnStop) btnStop.disabled = true;
                if (btnRestart) btnRestart.disabled = true;
            }
        }

        async function daemonAction(action) {
            try { await fetch(`/api/daemon/${action}`, { method: 'POST' }); } 
            catch (e) { console.error(e); }
            setTimeout(fetchStatus, 1000);
            setTimeout(fetchLogs, 1000);
        }

        async function fetchWatcherStatus() {
            const badge = document.getElementById('watcher-status');
            const spinner = document.getElementById('watcher-spinner');
            const btnStart = document.getElementById('btn-watcher-start');
            const btnStop = document.getElementById('btn-watcher-stop');
            const btnRestart = document.getElementById('btn-watcher-restart');
            if (spinner) spinner.classList.add('active');
            try {
                const res = await fetch('/api/watcher/status');
                if (!res.ok) throw new Error('Erreur HTTP ' + res.status);
                const data = await res.json();
                const isRunning = !!data.running;
                if (isRunning) {
                    badge.className = 'status-badge status-running';
                    badge.innerText = `En cours d'exécution`;
                } else if (data.status === 'crashed') {
                    badge.className = 'status-badge status-stopped';
                    badge.innerText = `Crashé : ${data.error}`;
                } else {
                    badge.className = 'status-badge status-stopped';
                    badge.innerText = 'Arrêté (Thread inactif)';
                }
                if (btnStart) btnStart.disabled = isRunning;
                if (btnStop) btnStop.disabled = !isRunning;
                if (btnRestart) btnRestart.disabled = !isRunning;
            } catch (err) {
                badge.className = 'status-badge status-stopped';
                badge.innerText = 'Erreur de connexion';
                if (btnStart) btnStart.disabled = false;
                if (btnStop) btnStop.disabled = true;
                if (btnRestart) btnRestart.disabled = true;
            } finally {
                setTimeout(() => { if (spinner) spinner.classList.remove('active'); }, 500);
            }
        }

        async function watcherAction(action) {
            try { await fetch(`/api/watcher/${action}`, { method: 'POST' }); } 
            catch (e) { console.error(e); }
            setTimeout(fetchWatcherStatus, 1000);
            setTimeout(fetchLogs, 1000);
        }

        function formatTimeLeft(seconds) {
            if (seconds <= 0) return "Expiration...";
            const h = Math.floor(seconds / 3600);
            const m = Math.floor((seconds % 3600) / 60);
            const s = seconds % 60;
            if (h > 0) return `${h}h ${m}m ${s}s`;
            return `${m}m ${s}s`;
        }

        function toggleTokenVisibility() {
            const input = document.getElementById('auth-token-input');
            const btn = document.getElementById('auth-token-btn');
            if (input) {
                if (input.type === 'password') {
                    input.type = 'text';
                    if (btn) btn.textContent = '🙈';
                } else {
                    input.type = 'password';
                    if (btn) btn.textContent = '👁️';
                }
            }
        }

        async function fetchWatcherState() {
            const spinner = document.getElementById('watcher-spinner');
            if (spinner) spinner.classList.add('active');
            try {
                const res = await fetch('/api/watcher/state');
                if (!res.ok) return;
                const data = await res.json();
                const tbody = document.getElementById('watcher-state-body');
                
                if (Object.keys(data.state).length === 0) {
                    tbody.innerHTML = '<tr><td colspan="4" style="padding: 15px; text-align: center; color: #666;">Aucune erreur en cours ...</td></tr>';
                    return;
                }
                
                let html = '';
                for (const [tag, info] of Object.entries(data.state)) {
                    let errDetails = '';
                    for (const [err, count] of Object.entries(info.errors)) {
                        errDetails += `<div style="font-size: 0.85em; color: #aaa; margin-top: 4px;">- ${err}: <strong style="color:#fff;">${count}</strong></div>`;
                    }
                    
                    let statusBadge = info.status === "En pause" 
                        ? `<span class="status-badge status-stopped" style="background: rgba(230, 81, 0, 0.2); color: #ffb74d;">En pause</span>`
                        : `<span class="status-badge status-running" style="color: #a5d6a7;">Observation</span>`;
                        
                    html += `<tr style="border-bottom: 1px solid #333;">
                        <td style="padding: 10px; word-break: break-all; max-width: 250px;"><strong>${tag}</strong></td>
                        <td style="padding: 10px;">${statusBadge}</td>
                        <td style="padding: 10px;">
                            <span style="color: var(--rust-orange); font-weight: bold;">Total Strikes : ${info.total_strikes}</span>
                            ${errDetails}
                        </td>
                        <td style="padding: 10px; color: #64b5f6; font-weight: bold;">${formatTimeLeft(info.time_left)}</td>
                    </tr>`;
                }
                tbody.innerHTML = html;
            } catch (err) {
                console.error("Erreur de récupération de l'état du watcher:", err);
            } finally {
                setTimeout(() => { if (spinner) spinner.classList.remove('active'); }, 500);
            }
        }

        async function loadRules() {
            try {
                const res = await fetch('/api/rules');
                const data = await res.json();
                document.getElementById('rules-editor').value = data.content || '';
                parseTextToVisual();
            } catch (e) { console.error(e); }
        }

        async function saveRules() {
            if (document.getElementById('raw-editor-container').style.display !== 'block') {
                parseVisualToText();
            }
            const content = document.getElementById('rules-editor').value;
            const res = await fetch('/api/rules', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ content })
            });
            if (res.ok) alert('Règles sauvegardées avec succès !');
        }

        async function refreshWatcher() {
            await fetchWatcherStatus();
            await fetchWatcherState();
            resetAndStartProgress('watcher-progress-bar', 5000);
        }

        async function fetchLogs() {
            const spinner = document.getElementById('log-spinner');
            const linesCount = document.getElementById('log-lines')?.value || 100;
            if (spinner) spinner.classList.add('active');
            try {
                const res = await fetch(`/api/logs?lines=${linesCount}`);
                const data = await res.json();
                const logDiv = document.getElementById('logs');
                const isAtBottom = logDiv.scrollHeight - logDiv.clientHeight - logDiv.scrollTop <= 30;
                const scrollOffsetFromBottom = logDiv.scrollHeight - logDiv.scrollTop;
                logDiv.textContent = data.content;
                if (isAtBottom) {
                    logDiv.scrollTop = logDiv.scrollHeight;
                } else {
                    logDiv.scrollTop = logDiv.scrollHeight - scrollOffsetFromBottom;
                }
            } catch (e) { 
                console.error(e); 
            } finally {
                setTimeout(() => {
                    if (spinner) spinner.classList.remove('active');
                    resetAndStartProgress('log-progress-bar', 5000);
                }, 500);
            }
        }

        async function clearLogs() {
            if (!confirm("Voulez-vous vraiment vider le fichier de logs ?")) return;
            try {
                const res = await fetch('/api/logs/clear', { method: 'POST' });
                if (res.ok) fetchLogs();
                else alert("Erreur lors de la suppression des logs.");
            } catch (e) { console.error(e); }
        }

        async function archiveLogs() {
            try {
                const res = await fetch('/api/logs/archive', { method: 'POST' });
                const data = await res.json();
                if (res.ok && data.success) {
                    alert("Logs archivés avec succès : " + data.filename);
                    fetchLogs();
                } else {
                    alert("Erreur lors de l'archivage : " + (data.error || "Inconnue"));
                }
            } catch (e) { console.error(e); }
        }

        async function restartAdmin() {
            if (!confirm("Voulez-vous vraiment redémarrer le panneau d'administration ?")) return;
            try {
                await fetch('/api/admin/restart', { method: 'POST' });
                alert("Le panneau admin redémarre... La page va se recharger dans 3 secondes.");
                setTimeout(() => window.location.reload(), 3000);
            } catch (e) {
                console.error(e);
                alert("Erreur lors de la demande de redémarrage.");
            }
        }

        async function fetchEnvConfig() {
            try {
                const res = await fetch('/api/env');
                if (!res.ok) return;
                const data = await res.json();
                const container = document.getElementById('env-config-grid');
                if (!container) return;

                const config = data.config || {};
                currentReadonlyKeys = data.readonly || [];

                let html = '';
                for (const [key, val] of Object.entries(config)) {
                    let inputHtml = '';
                    const isReadOnly = currentReadonlyKeys.includes(key);

                    const cardStyle = isReadOnly 
                        ? 'background: #121212; border: 1px dashed #333; opacity: 0.75;' 
                        : 'background: #181818; border: 1px solid var(--border-color);';
                    
                    const labelHtml = isReadOnly 
                        ? `<span style="color: #777; font-family: monospace;" title="Lecture seule (redémarrage requis)">🔒 ${key}</span>` 
                        : `<span style="color: #aaa; font-family: monospace;">${key}</span>`;

                    if (key === 'AUTH_TOKEN') {
                        inputHtml = `
                            <div style="display: inline-flex; align-items: center; gap: 4px;">
                                <input id="env-input-${key}" type="password" value="${val}" ${isReadOnly ? 'readonly' : ''} 
                                       style="background: #181818; border: 1px solid #333; border-radius: 4px; color: #888; font-family: monospace; width: 100px; padding: 2px 5px; text-align: right; cursor: not-allowed;">
                                <button id="auth-token-btn" onclick="toggleTokenVisibility()" type="button"
                                        style="background: none; border: none; cursor: pointer; padding: 0; font-size: 1em;">👁️</button>
                            </div>`;
                    } else if (key === 'LOGS_WATCHER' || key === 'TOR_KEEP_LAST') {
                        const intVal = parseInt(val, 10) || 0;
                        inputHtml = `
                            <select id="env-input-${key}" style="background: #222; border: 1px solid #444; border-radius: 4px; color: #fff; font-family: monospace; padding: 2px 5px;">
                                <option value="0" ${intVal === 0 ? 'selected' : ''}>0</option>
                                <option value="1" ${intVal === 1 ? 'selected' : ''}>1</option>
                            </select>`;
                    } else if (key === 'DRY_RUN' || typeof val === 'boolean') {
                        const boolVal = (val === true || val === 'true');
                        inputHtml = `
                            <select id="env-input-${key}" style="background: #222; border: 1px solid #444; border-radius: 4px; color: #fff; font-family: monospace; padding: 2px 5px;">
                                <option value="true" ${boolVal ? 'selected' : ''}>true</option>
                                <option value="false" ${!boolVal ? 'selected' : ''}>false</option>
                            </select>`;
                    } else {
                        const inputBg = isReadOnly ? '#181818' : '#222';
                        const inputBorder = isReadOnly ? '1px solid #333' : '1px solid #444';
                        const inputColor = isReadOnly ? '#888' : '#fff';
                        const inputCursor = isReadOnly ? 'not-allowed' : 'text';

                        inputHtml = `
                            <input id="env-input-${key}" type="${typeof val === 'number' ? 'number' : 'text'}" value="${val}" ${isReadOnly ? 'readonly' : ''} 
                                   style="background: ${inputBg}; border: ${inputBorder}; border-radius: 4px; color: ${inputColor}; font-family: monospace; width: 110px; padding: 2px 5px; text-align: right; cursor: ${inputCursor};">`;
                    }

                    html += `
                        <div style="${cardStyle} padding: 8px 12px; border-radius: 6px; display: flex; justify-content: space-between; align-items: center; font-size: 0.85em;">
                            ${labelHtml}
                            <div style="margin-left: 10px;">${inputHtml}</div>
                        </div>`;
                }
                container.innerHTML = html;
            } catch (e) {
                console.error("Erreur de chargement des variables :", e);
            }
        }

        async function saveEnvConfig() {
            const payload = {};

            document.querySelectorAll('[id^="env-input-"]').forEach(el => {
                const key = el.id.replace('env-input-', '');
                if (!currentReadonlyKeys.includes(key)) {
                    if (key === 'LOGS_WATCHER' || key === 'TOR_KEEP_LAST') {
                        payload[key] = parseInt(el.value, 10);
                    } else if (el.value === 'true' || el.value === 'false') {
                        payload[key] = el.value === 'true';
                    } else if (el.type === 'number') {
                        payload[key] = parseInt(el.value, 10);
                    } else {
                        payload[key] = el.value;
                    }
                }
            });

            try {
                const res = await fetch('/api/env', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify(payload)
                });
                
                const data = await res.json();

                if (res.ok) {
                    alert("Configuration appliquée à la session en cours.");
                    fetchEnvConfig();
                } else {
                    alert(`Erreur (${res.status}) : ${data.message || 'Action non autorisée'}`);
                }
            } catch (e) {
                console.error("Erreur d'envoi de la configuration :", e);
            }
        }

        async function resetEnvConfig() {
            if (!confirm("Rétablir les variables de session à leurs valeurs initiales ?")) return;

            try {
                const res = await fetch('/api/env/reset', { method: 'POST' });
                if (res.ok) {
                    await fetchEnvConfig();
                    alert("Configuration réinitialisée aux valeurs de départ.");
                } else {
                    alert("Erreur lors de la réinitialisation.");
                }
            } catch (e) {
                console.error("Erreur de réinitialisation :", e);
            }
        }

        loadRules();
        fetchStatus();
        refreshWatcher();
        fetchLogs();
        fetchEnvConfig();
        setInterval(fetchStatus, 5000);
        setInterval(refreshWatcher, 5000);
        setInterval(fetchLogs, 5000);
    </script>
</body>
</html>
"""

# ==========================================
# LOGGING SETUP
# ==========================================
RUST_LOG_LEVELS = {
    "TRACE": logging.DEBUG,
    "DEBUG": logging.DEBUG,
    "INFO": logging.INFO,
    "WARN": logging.WARNING,
    "ERROR": logging.ERROR,
}

def get_global_rust_log_level(env_val):
    if not env_val:
        return logging.INFO
        
    directives = [d.strip() for d in env_val.split(",") if d.strip()]
    for directive in directives:
        if "=" not in directive:
            level_str = directive.upper()
            if level_str in RUST_LOG_LEVELS:
                return RUST_LOG_LEVELS[level_str]
    
    return logging.INFO

daemon_log_env = os.getenv("RUST_DAEMON_LOG")

if daemon_log_env:
    log_level = RUST_LOG_LEVELS.get(daemon_log_env.strip().upper(), logging.INFO)
else:
    rust_log_env = os.getenv("RUST_LOG", "info")
    log_level = get_global_rust_log_level(rust_log_env)

logger = logging.getLogger("Rustatio")
logger.setLevel(log_level)

formatter = logging.Formatter('%(asctime)s :: %(message)s', "%d-%m-%Y %H:%M:%S")
console_handler = logging.StreamHandler()
console_handler.setFormatter(formatter)
logger.addHandler(console_handler)

class FlushingRotatingFileHandler(RotatingFileHandler):
    def emit(self, record):
        super().emit(record)
        self.flush()

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
        
        file_handler = FlushingRotatingFileHandler(LOGFILE, maxBytes=1024*1024, backupCount=10)
        file_handler.setFormatter(formatter)
        logger.addHandler(file_handler)

setup_file_handler()

STYLE_LEVELS = {
    "trace": logging.DEBUG, "finish": logging.INFO, "task": logging.INFO,
    "recycle": logging.INFO, "lock": logging.INFO, "data": logging.INFO,
    "saving": logging.INFO, "succes": logging.INFO, "start": logging.INFO,
    "warning": logging.WARNING, "error": logging.ERROR, "denied": logging.ERROR,
}

def log(msg, style="default"):
    prefix_spaces = ""
    base_style = style

    if style.startswith("ff_"):
        prefix_spaces = "          └─ "
        base_style = style[3:]
    elif style.startswith("f_"):
        prefix_spaces = "   └─ "
        base_style = style[2:]

    prefixes = {
        "start": "🚀 ", "error": "❌ ", "succes": "✅️ ", "warning": "⚠️ ",
        "denied": "🚫 ", "saving": "💾 ", "data": "🧪 ", "lock": "🔒 ",
        "recycle": "♻️ ", "task": "⚡ ", "finish": "🏁 ", "trace": "🔍 "
    }
    prefix = prefixes.get(base_style, "")
    level = STYLE_LEVELS.get(base_style, logging.INFO)
    logger.log(level, f"{prefix_spaces}{prefix}{msg}")


# ==========================================
# API CLIENT
# ==========================================
class APIClient:
    def __init__(self, base_url):
        self.base_url = base_url.rstrip("/")
        self.session = requests.Session()
        self.session.headers.update({"Accept": "application/json"})

        if AUTH_TOKEN:
            self.session.headers.update({"Authorization": f"Bearer {AUTH_TOKEN}"})

        log(f"APIClient initialized with base_url: {self.base_url}", "trace")

    def request(self, method, endpoint, payload=None, timeout=5):
        url = f"{self.base_url}/api/{endpoint}"
        max_retries = 3
        log(f"API Request -> {method} {url} with payload: {payload}", "trace")

        for attempt in range(1, max_retries + 1):
            try:
                if payload and method in ["POST", "PATCH"]:
                    resp = self.session.request(method, url, json=payload, timeout=timeout)
                else:
                    resp = self.session.request(method, url, timeout=timeout)
                resp.raise_for_status()
                data = resp.json()
                log(f"API Response <- {method} {endpoint} Status: {resp.status_code}", "trace")

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
# AST EVALUATOR (REPLACES EVAL SAFELY)
# ==========================================
class ASTEvaluator:
    BINARY_OPS = {
        ast.Add: operator.add,
        ast.Sub: operator.sub,
        ast.Mult: operator.mul,
        ast.Div: operator.truediv,
        ast.FloorDiv: operator.floordiv,
        ast.Mod: operator.mod,
        ast.Pow: operator.pow,
    }

    UNARY_OPS = {
        ast.UAdd: operator.pos,
        ast.USub: operator.neg,
        ast.Not: operator.not_,
    }

    COMP_OPS = {
        ast.Eq: operator.eq,
        ast.NotEq: operator.ne,
        ast.Lt: operator.lt,
        ast.LtE: operator.le,
        ast.Gt: operator.gt,
        ast.GtE: operator.ge,
        ast.In: lambda a, b: a in b,
        ast.NotIn: lambda a, b: a not in b,
    }

    ALLOWED_FUNCS = {
        "str": str,
        "float": float,
        "int": int,
        "bool": bool,
    }

    ALLOWED_METHODS = {"lower", "upper", "strip", "hex", "startswith", "endswith"}

    def __init__(self, context):
        self.context = context

    def eval(self, node):
        if isinstance(node, ast.Expression):
            return self.eval(node.body)

        elif isinstance(node, ast.Constant):
            return node.value

        elif hasattr(ast, 'Num') and isinstance(node, ast.Num):
            return node.n
        elif hasattr(ast, 'Str') and isinstance(node, ast.Str):
            return node.s
        elif hasattr(ast, 'NameConstant') and isinstance(node, ast.NameConstant):
            return node.value
        elif hasattr(ast, 'Bytes') and isinstance(node, ast.Bytes):
            return node.s

        elif isinstance(node, ast.Name):
            if node.id in ("True", "true"):
                return True
            if node.id in ("False", "false"):
                return False
            if node.id in ("None", "null"):
                return None
            if node.id in self.context:
                return self.context[node.id]
            if node.id in self.ALLOWED_FUNCS:
                return self.ALLOWED_FUNCS[node.id]
            raise NameError(f"Name '{node.id}' is not defined in evaluation context")

        elif isinstance(node, ast.BoolOp):
            if isinstance(node.op, ast.And):
                for val in node.values:
                    res = self.eval(val)
                    if not res:
                        return res
                return res
            elif isinstance(node.op, ast.Or):
                for val in node.values:
                    res = self.eval(val)
                    if res:
                        return res
                return res

        elif isinstance(node, ast.UnaryOp):
            op_type = type(node.op)
            if op_type in self.UNARY_OPS:
                return self.UNARY_OPS[op_type](self.eval(node.operand))
            raise TypeError(f"Unsupported unary operator: {op_type}")

        elif isinstance(node, ast.BinOp):
            op_type = type(node.op)
            if op_type in self.BINARY_OPS:
                left = self.eval(node.left)
                right = self.eval(node.right)
                return self.BINARY_OPS[op_type](left, right)
            raise TypeError(f"Unsupported binary operator: {op_type}")

        elif isinstance(node, ast.Compare):
            left = self.eval(node.left)
            for op, comparator in zip(node.ops, node.comparators):
                op_type = type(op)
                if op_type not in self.COMP_OPS:
                    raise TypeError(f"Unsupported comparison operator: {op_type}")
                right = self.eval(comparator)
                if not self.COMP_OPS[op_type](left, right):
                    return False
                left = right
            return True

        elif isinstance(node, ast.Call):
            if isinstance(node.func, ast.Name):
                func_name = node.func.id
                args = [self.eval(arg) for arg in node.args]
                kwargs = {kw.arg: self.eval(kw.value) for kw in node.keywords}
                if func_name in self.context and callable(self.context[func_name]):
                    return self.context[func_name](*args, **kwargs)
                if func_name in self.ALLOWED_FUNCS:
                    return self.ALLOWED_FUNCS[func_name](*args, **kwargs)
                raise NameError(f"Function '{func_name}' is not allowed or defined")

            elif isinstance(node.func, ast.Attribute):
                obj = self.eval(node.func.value)
                method_name = node.func.attr
                if method_name in self.ALLOWED_METHODS and hasattr(obj, method_name):
                    args = [self.eval(arg) for arg in node.args]
                    kwargs = {kw.arg: self.eval(kw.value) for kw in node.keywords}
                    return getattr(obj, method_name)(*args, **kwargs)
                raise AttributeError(f"Method '{method_name}' is not allowed")

        elif isinstance(node, ast.Attribute):
            obj = self.eval(node.value)
            attr_name = node.attr
            if hasattr(obj, attr_name):
                return getattr(obj, attr_name)
            return None

        raise TypeError(f"Unsupported AST node type: {type(node).__name__}")


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
        self.current_instances = []
        
        # Thread Controls
        self.stop_event = threading.Event()
        self.logs_thread = None
        self.stop_event = threading.Event()
        self.watcher_stop_event = threading.Event()  # <--- AJOUT
        self.logs_thread = None

        log("Initializing regex patterns for RustatioManager", "trace")
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
                log(f"get_val path '{path}' missed at part '{part}', returning default: {default}", "trace")
                return default
        return current

    def get_num(self, data, path, default=0):
        val = self.get_val(data, path, default)
        try:
            return float(val) if val is not None else float(default)
        except (ValueError, TypeError):
            log(f"get_num conversion failed for path '{path}' with value '{val}', falling back to default: {default}", "trace")
            return float(default)

    def _extract_bracket(self, msg):
        start = msg.find("[")
        if start == -1: return None, msg
        level = 0
        for i in range(start, len(msg)):
            if msg[i] == "[": level += 1
            elif msg[i] == "]":
                level -= 1
                if level == 0: return msg[start+1:i].strip(), msg[i+1:].strip()
        return msg[start:], ""

    def load_configs(self):
        log("Loading configurations...", "trace")
        try:
            if os.path.exists(DEFAULTS_FILE):
                with open(DEFAULTS_FILE, 'r') as f:
                    data = json.load(f)
                    self.default_config = data.get("default_config", data)
                    log(f"Defaults loaded from {DEFAULTS_FILE}", "start")
                    log(f"Loaded default_config content: {self.default_config}", "trace")
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
                    log(f"Processing rule line: {line}", "trace")
                    parts = line.split('|', 2)
                    if len(parts) == 3:
                        cond, action, assign = [x.strip() for x in parts]
                        default_keys = set(self.re_default_config.findall(cond))
                        all_keys = set(self.re_inst_keys.findall(cond))
                        instance_keys = { k for k in all_keys if not k.startswith("default_config.") and not k.startswith("torrent.info_hash") }

                        py_cond = self.translate_condition(cond)
                        try:
                            ast_tree = ast.parse(py_cond, mode='eval')
                            self.rules_lines.append({
                                "raw": line, "cond_str": cond, "ast_tree": ast_tree,
                                "action": action, "assign": assign,
                                "default_keys": default_keys, "instance_keys": instance_keys
                            })
                            log(f"Rule successfully parsed to AST -> Condition: {py_cond}", "trace")
                        except SyntaxError as e:
                            log(f"Generated rule syntax error: {py_cond} ({e})", "error")
                    else:
                        log(f"Invalid rule skipped: {line}", "denied")

                log(f"Rules loaded and compiled to AST from {RULES_FILE}", "start")
            else:
                log(f"Rules file {RULES_FILE} not found", "error")
        except Exception as e:
            log(f"Error: {str(e)}", "error")

    def load_logs_state(self):
        log(f"Loading logs state from {CHECK_LOGS_FILE}", "trace")
        try:
            if os.path.exists(CHECK_LOGS_FILE):
                with open(CHECK_LOGS_FILE, 'r') as f:
                    return json.load(f)
        except Exception as e:
            log(f"Error: {str(e)}", "error")
        return {}

    def save_logs_state(self):
        log(f"Saving logs state to {CHECK_LOGS_FILE}", "trace")
        try:
            with open(CHECK_LOGS_FILE, 'w') as f:
                json.dump(self.logs_state, f)
        except Exception as e:
            log(f"Error: {str(e)}", "error")

    def format_time_bash_style(self, t):
        h = t // 3600
        m = (t % 3600) // 60
        if h > 0: return f"{h}h{m:02d}" if m > 0 else f"{h}h"
        return f"{m}min"

    def get_elapsed_since_midnight_tag(self, ts):
        t_struct = time.localtime(ts)
        midnight = time.mktime((t_struct.tm_year, t_struct.tm_mon, t_struct.tm_mday, 0, 0, 0, t_struct.tm_wday, t_struct.tm_yday, t_struct.tm_isdst))
        return self.format_time_bash_style(int(ts - midnight))

    def get_cached_rand(self, path, low, high, raw_match, tracker=None):
        key = f"{path}:{low}:{high}:{raw_match}"
        if key not in self.rand_cache:
            l, h = float(low), float(high)
            if l > h: l, h = h, l
            self.rand_cache[key] = random.uniform(l, h)
            log(f"Generated new cached random for {key}: {self.rand_cache[key]}", "trace")
        if tracker is not None:
            tracker.append(key)
        return self.rand_cache[key]

    def clear_rand_keys(self, keys):
        if keys:
            for k in keys:
                if k in self.rand_cache:
                    self.rand_cache.pop(k, None)
                    log(f"Purged cached random key after API success: {k}", "trace")

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
        c = re.sub(r"\b(AND|OR)\b", lambda m: m.group(0).lower(), cond_str)
        c = self.re_default_config.sub(r'__default__("\1")', c)

        def repl_bool(m):
            val_map = {"true": "True", "false": "False", "null": "None"}
            return f"(__get__('{m.group(1)}') == {val_map[m.group(2).lower()]})"
        
        c = self.re_bool_null_eq.sub(repl_bool, c)
        c = self.re_bool_null_colon.sub(repl_bool, c)

        def repl_range(m):
            path, low_str, high_str = m.group(1), m.group(2), m.group(3)
            return f"(__get_num__('{path}') > __get_rand__('{path}', {low_str}, {high_str}, '{m.group(0)}'))"
        c = self.re_range.sub(repl_range, c)

        for pattern, repl in self.regex_subs:
            c = pattern.sub(repl, c)

        c = self.re_num_cmp.sub(r'(__get_num__("\1") \2 \3)', c)
        c = self.re_num_eq.sub(lambda m: f"(__get_num__('{m.group(1)}') {'==' if m.group(2) == '=' else '!='} {m.group(3)})", c)
        c = self.re_num_neq.sub(r'(__get_num__("\1") != \2)', c)
        return c

    def evaluate_rule(self, inst, ast_tree):
        eval_rand_keys = []
        eval_context = {
            '__get__': lambda path, d=None: self.get_val(inst, path, d),
            '__get_num__': lambda path: self.get_num(inst, path),
            '__tags__': inst.get("tags") or [],
            '__info_hash__': lambda: bytes(inst.get("torrent", {}).get("info_hash", [])).hex(),
            '__default__': lambda k: self.get_val(self.default_config, k),
            '__get_rand__': lambda path, low, high, raw_match: self.get_cached_rand(path, low, high, raw_match, eval_rand_keys),
            'str': str,
            'float': float,
            'int': int,
            'bool': bool,
            'True': True,
            'False': False,
            'None': None,
        }
        try:
            evaluator = ASTEvaluator(eval_context)
            result = evaluator.eval(ast_tree)
            log(f"Evaluated rule for instance ID {inst.get('id')}: result = {result}", "trace")
            return bool(result), eval_rand_keys
        except Exception as e:
            log(f"Evaluation error for instance ID {inst.get('id')}: {e}", "trace")
            return False, []

    def is_action_valid(self, action, state):
        if action == "start": return (state == "Stopped")
        if action in ["stop", "pause"]: return (state == "Running")
        if action == "resume": return (state == "Paused")
        return action in ["update", "addtags", "removetags", "delete"]

    def _resolve_assignment_val(self, val_raw):
        val_raw = val_raw.strip()
        if val_raw.startswith("default_config."): return self.get_val(self.default_config, val_raw.replace("default_config.", ""))
        lower_val = val_raw.lower()
        if lower_val in ["true", "false", "null"]: return True if lower_val == "true" else False if lower_val == "false" else None
        if (val_raw.startswith('"') and val_raw.endswith('"')) or (val_raw.startswith("'") and val_raw.endswith("'")): return val_raw[1:-1]
        try: return float(val_raw) if "." in val_raw else int(val_raw)
        except ValueError: return val_raw

    def process_rules(self):
        log("Starting process_rules cycle", "trace")
        instances_resp = self.api.request("GET", "instances")
        if not instances_resp:
            log("process_rules aborted: no response from instances endpoint", "trace")
            return

        with self.strike_lock:
            self.current_instances = instances_resp.get("data", [])

        if not self.current_instances:
            log("process_rules: current_instances list is empty", "trace")
            return
        sample_inst = self.current_instances[0]

        for rule in self.rules_lines:
            if not self.validate_rule_keys(rule, sample_inst):
                log(f"Rule keys validation failed for rule: {rule['raw']}", "trace")
                continue
            for inst in self.current_instances:
                if inst.get("_deleted"): continue
                state = self.get_val(inst, "stats.state")
                if not self.is_action_valid(rule["action"], state): continue
                if TOR_KEEP_LAST and rule["action"] in ["stop", "delete"]:
                    announce = str(self.get_val(inst, "torrent.announce", "")).lower()
                    count = sum(1 for i in self.current_instances if not i.get("_deleted") and str(self.get_val(i, "torrent.announce", "")).lower() == announce)
                    if count <= 1:
                        log(f"TOR_KEEP_LAST triggered: skipping action '{rule['action']}' for instance {inst.get('id')}.", "trace")
                        continue

                matched, rand_keys = self.evaluate_rule(inst, rule["ast_tree"])
                if matched:
                    log(f"Rule matched! Applying action '{rule['action']}' on instance ID {inst.get('id')}", "trace")   
                    self.apply_action(inst, rule["action"], rule["assign"], rule["raw"], rand_keys)

    def apply_action(self, inst, action, assign, rule_line, used_keys=None):
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
            if DRY_RUN: log(f"Would add tags to ID='{id_}' TAGS='{payload}'", "f_recycle")
            elif self.api.request("POST", "grid/tag", payload):
                inst["tags"] = list(set(existing_tags + new_tags))
                log(f"Tags added ({assign})", "f_succes")
                self.clear_rand_keys(used_keys)
            else:
                log(f"Failed to add tags ({assign}) for instance {id_}", "f_error")

        elif action == "removetags":
            tags_to_remove = [t.strip() for t in assign.split(',') if t.strip()]
            existing_tags = inst.get("tags") or []
            del_tags = [t for t in tags_to_remove if t in existing_tags]

            if not del_tags: return
            payload = {"ids": [id_], "add_tags": [], "remove_tags": del_tags}
            if DRY_RUN: log(f"Would remove tags from ID='{id_}' TAGS='{payload}'", "f_recycle")
            elif self.api.request("POST", "grid/tag", payload):
                inst["tags"] = [t for t in existing_tags if t not in del_tags]
                log(f"Tags removed ({assign})", "f_succes")
                self.clear_rand_keys(used_keys)
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

                if DRY_RUN: log(f"Would patch config ID='{id_}' PAYLOAD='{payload}'", "f_recycle")
                elif self.api.request("PATCH", f"instances/{id_}/config", payload):
                    inst.setdefault("config", {})[key] = val
                    log(f"Patch succeeded ({assign})", "f_succes")
                    self.clear_rand_keys(used_keys)
                else: log("Patch failed", "f_error")
            else: log(f"update: invalid assign '{assign}'", "warning")

        elif action == "start":
            payload = {"torrent": copy.deepcopy(inst.get("torrent", {})), "config": copy.deepcopy(inst.get("config", {}))}
            if DRY_RUN: log(f"Would start ID='{id_}'", "f_recycle")
            elif self.api.request("POST", f"faker/{id_}/start", payload):
                inst.setdefault("stats", {})["state"] = "Running"
                log("Start succeeded", "f_succes")
                self.clear_rand_keys(used_keys)

        elif action in ["stop", "pause", "resume"]:
            endpoints = {"stop": f"faker/{id_}/stop", "pause": "grid/pause", "resume": "grid/resume"}
            payload = {"ids": [id_]} if action in ["pause", "resume"] else None
            if DRY_RUN: log(f"Would {action} ID='{id_}'", "f_recycle")
            elif self.api.request("POST", endpoints[action], payload):
                inst.setdefault("stats", {})["state"] = "Stopped" if action == "stop" else ("Paused" if action == "pause" else "Running")
                log(f"{action.capitalize()} succeeded", "f_succes")
                self.clear_rand_keys(used_keys)

        elif action == "delete":
            if "instance" in assign:
                if DRY_RUN: log(f"Would delete ID='{id_}'", "f_recycle")
                elif self.api.request("DELETE", f"instances/{id_}?force=true"):
                    inst["_deleted"] = True
                    log("Delete succeeded", "f_succes")
                    self.clear_rand_keys(used_keys)

            if "watchfile" in assign or "archive" in assign:
                hex_hash = bytes(inst.get("torrent", {}).get("info_hash", [])).hex()
                if hex_hash:
                    files_resp = self.api.request("GET", "watch/files")
                    files = files_resp.get("data", []) if files_resp else []														
                    for f in files:
                        if f.get("info_hash") == hex_hash:
                            filename, filepath = f.get("filename", ""), f.get("path", "")
                            
                            if "archive" in assign and ARCHIVE_FOLDER:
                                dest = os.path.join(ARCHIVE_FOLDER, os.path.basename(filename))
                                if not os.path.exists(dest):
                                    if DRY_RUN: log(f"Would archive file '{filename}' at '{filepath}'", "f_recycle")
                                    else:
                                        os.makedirs(ARCHIVE_FOLDER, exist_ok=True)
                                        try:
                                            shutil.copy2(filepath, dest)
                                            log(f"Torrent archived {dest}", "f_saving")
                                        except Exception: log(f"Failed to archive {filepath}", "f_error")

                            if "watchfile" in assign:
                                if DRY_RUN: log(f"Would delete file '{filename}' at '{filepath}'", "f_recycle")
                                else:
                                    if self.api.request("DELETE", f"watch/files?path={urllib.parse.quote(filename)}"):
                                        log("Delete succeeded", "f_succes")
                                    else:
                                        log("Failed to delete file", "f_error")

    def logs_watcher_thread(self):
        url = f"{self.api.base_url}/api/logs"
        log(f"Starting logs_watcher_thread targeting SSE URL: {url}", "trace")
        headers = {
            "Accept": "text/event-stream",
            "Cache-Control": "no-cache",
            "Connection": "keep-alive",
            "X-Accel-Buffering": "no"
        }
        if AUTH_TOKEN:
            headers["Authorization"] = f"Bearer {AUTH_TOKEN}"

        while not self.stop_event.is_set() and not self.watcher_stop_event.is_set():
            try:
                with requests.get(url, stream=True, headers=headers, timeout=60) as r:
                    for line in r.iter_lines(decode_unicode=True):
                        if self.stop_event.is_set() or self.watcher_stop_event.is_set(): return
                        self._check_expirations()
                        if line and line.startswith("data:"):
                            self._handle_log_event(line[5:].strip())
            except requests.exceptions.Timeout:
                self._check_expirations()
            except Exception as e:
                if self.stop_event.is_set() or self.watcher_stop_event.is_set():
                    log(f"logs_watcher_thread stopping", "trace")
                    return
                log(f"logs_watcher_thread exception encountered: {e}", "trace")
                self.stop_event.wait(5)

    def _handle_log_event(self, json_str):
        log(f"Handling log event payload: {json_str}", "trace")
        try:
            event = json.loads(json_str)
            level = event.get("level", "").lower()
            msg = event.get("message", "")

            if "error" in level and "[" in msg:
                tag, rest = self._extract_bracket(msg)
                if tag:
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
                if self.get_val(inst, "torrent.name") == name: return inst
        return None

    def _trigger_watcher_pause(self, tag, rest, action_ts):
        inst = self._find_instance_by_name(tag)
        if not inst: return
        state = self.get_val(inst, "stats.state")
        if self.is_action_valid("pause", state):
            log(f"Repeated error detected (x{WATCHER_MAX_STRIKE}). Try to pause for {self.format_time_bash_style(WATCHER_PAUSE_TIME)} and add tag", "warning")
            log(f"Torrent name : {tag}", "f_data")
            log(f"{rest}", "f_data")
            id_ = inst.get("id")
            err_tag = f"Err {self.get_elapsed_since_midnight_tag(action_ts)}"

            if DRY_RUN: log(f"Would pause '{tag}' and addtags '{err_tag}'", "f_recycle")
            else:
                if self.api.request("POST", "grid/pause", {"ids": [id_]}):
                    existing_tags = inst.get("tags") or []
                    if err_tag not in existing_tags:
                        self.api.request("POST", "grid/tag", {"ids": [id_], "add_tags": [err_tag], "remove_tags": []})
                        log(f"Tags applied ({err_tag})", "f_succes")

    def _check_expirations(self):
        log("Checking log expirations and purges", "trace")
        
        with self.strike_lock:
            if not self.current_instances:
                log("Skip _check_expirations: current_instances is empty", "trace")
                return
            instances_snapshot = list(self.current_instances)

        now = time.time()
        dirty = False
        expired_tags = []
        active_names = {self.get_val(inst, "torrent.name") for inst in instances_snapshot}

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

            if DRY_RUN: log(f"Would resume '{tag}' and removetags '{err_tag}'", "f_recycle")
            else:
                if self.api.request("POST", "grid/resume", {"ids": [id_]}):
                    log("Resume succeeded", "f_succes")
                    existing_tags = inst.get("tags") or []
                    if err_tag in existing_tags:
                        self.api.request("POST", "grid/tag", {"ids": [id_], "add_tags": [], "remove_tags": [err_tag]})
                        log(f"Tags removed ({err_tag})", "f_succes")

    def run(self):
        initial_interval = 5
        log(f"Starting RustatioManager daemon", "start")
        self.load_configs()
        self.stop_event.clear()

        interval = REFRESH_INTERVAL
        if interval == 0 and self.default_config:
            interval = int(self.get_val(self.default_config, "scrape_interval", 60))

        interval = max(0, interval - initial_interval)
        log(f"REFRESH INTERVAL : {interval + initial_interval}s", "data")

        if LOGS_WATCHER:
            self.start_watcher()

        headers = {}
        if AUTH_TOKEN:
            headers["Authorization"] = f"Bearer {AUTH_TOKEN}"

        while not self.stop_event.is_set():
            try:
                resp = requests.get(f"{RUSTATIO_API}/health", headers=headers, timeout=3)
                if resp.text == "OK": break
            except Exception: pass
            if self.stop_event.is_set(): return
            log(f"Rustatio not ready. Retry in {initial_interval}s", "warning")
            self.stop_event.wait(initial_interval)

        while not self.stop_event.is_set():
            if self.stop_event.wait(initial_interval): break
            if LOGFILE and LOGFILE != "/dev/null" and not os.path.exists(LOGFILE):
                setup_file_handler()
                log("Log recreated automatically", "start")
                self.load_configs()

            try: 
                self.process_rules()
                if LOGS_WATCHER:
                    self._check_expirations()
            except Exception as e: 
                log(f"Error in process_rules: {str(e)}", "error")
            
            if self.stop_event.wait(interval): break
            
        log("RustatioManager stopped", "finish")

    def is_watcher_running(self):
        return self.logs_thread is not None and self.logs_thread.is_alive()

    def start_watcher(self):
        if not self.is_watcher_running():
            self.watcher_stop_event.clear()
            self.logs_thread = threading.Thread(target=self.logs_watcher_thread, daemon=True)
            self.logs_thread.start()
            log("Log Watcher started", "start")

    def stop_watcher(self):
        if self.is_watcher_running():
            self.watcher_stop_event.set()
            if self.logs_thread:
                self.logs_thread.join(timeout=3)
            log("Log Watcher stopped", "finish")

    def stop(self):
        self.stop_event.set()
        for h in logger.handlers:
            h.flush()


# ==========================================
# FLASK WEB PANEL (ADMIN)
# ==========================================
manager = None
manager_thread = None

@app.before_request
def require_auth():
    if not AUTH_TOKEN:
        return None

    auth_header = request.headers.get("Authorization", "")
    token_param = request.args.get("token", "")
    auth = request.authorization

    if auth_header.startswith("Bearer ") and auth_header[7:].strip() == AUTH_TOKEN:
        return None

    if auth and (auth.password == AUTH_TOKEN or auth.username == AUTH_TOKEN):
        return None

    if token_param == AUTH_TOKEN:
        return None

    return jsonify({"error": "Accès non autorisé"}), 401, {
        'WWW-Authenticate': 'Basic realm="Rustatio Admin"'
    }

@app.route('/')
def index():
    rules_filename = os.path.basename(RULES_FILE) if RULES_FILE else "rules.txt"
    logfile_filename = os.path.basename(LOGFILE) if LOGFILE else "rustatio_daemon.log"
    return render_template_string(
        HTML_TEMPLATE, 
        rules_filename=rules_filename, 
        logfile_filename=logfile_filename
    )

@app.route('/api/status', methods=['GET'])
def get_status():
    global manager_thread
    if manager_thread and manager_thread.is_alive():
        return jsonify({"running": True, "pid": os.getpid()})
    return jsonify({"running": False})

daemon_lock = threading.Lock()

@app.route('/api/daemon/<action>', methods=['POST'])
def manage_daemon(action):
    global manager, manager_thread
    
    with daemon_lock:
        if action == 'stop':
            if manager and manager_thread and manager_thread.is_alive():
                manager.stop()
                manager_thread.join(timeout=5)
            return jsonify({"success": True})

        elif action == 'start':
            if not manager_thread or not manager_thread.is_alive():
                manager = RustatioManager()
                manager_thread = threading.Thread(target=manager.run, daemon=True)
                manager_thread.start()
            return jsonify({"success": True})

        elif action == 'restart':
            if manager and manager_thread and manager_thread.is_alive():
                manager.stop()
                manager_thread.join(timeout=5)
            manager = RustatioManager()
            manager_thread = threading.Thread(target=manager.run, daemon=True)
            manager_thread.start()
            return jsonify({"success": True})

        return jsonify({"success": False, "error": "Action invalide ou état incorrect"})

@app.route('/api/rules', methods=['GET', 'POST'])
def manage_rules():
    if request.method == 'POST':
        content = request.json.get('content', '')
        os.makedirs(os.path.dirname(RULES_FILE), exist_ok=True)
        with open(RULES_FILE, 'w', encoding='utf-8') as f:
            f.write(content)
        if manager:
            manager.load_configs()
        return jsonify({"success": True})
    else:
        content = ""
        if os.path.exists(RULES_FILE):
            with open(RULES_FILE, 'r', encoding='utf-8') as f:
                content = f.read()
        return jsonify({"content": content})

@app.route('/api/logs', methods=['GET'])
def get_logs():
    content = "Fichier log introuvable."
    try:
        lines_limit = int(request.args.get('lines', 100))
    except ValueError:
        lines_limit = 100

    if os.path.exists(LOGFILE):
        with open(LOGFILE, 'r', encoding='utf-8') as f:
            lines = f.readlines()
            content = "".join(lines[-lines_limit:])
    return jsonify({"content": content})

@app.route('/api/logs/clear', methods=['POST'])
def clear_logs():
    if os.path.exists(LOGFILE):
        with open(LOGFILE, 'w', encoding='utf-8') as f:
            f.write("")
    return jsonify({"success": True})

@app.route('/api/logs/archive', methods=['POST'])
def archive_logs():
    if not os.path.exists(LOGFILE):
        return jsonify({"success": False, "error": "Fichier de log introuvable"}), 404
    try:
        timestamp = datetime.datetime.now().strftime("%Y-%m-%d_%H-%M-%S")
        dir_name = os.path.dirname(LOGFILE)
        base_name = os.path.basename(LOGFILE)
        name, ext = os.path.splitext(base_name)
        archive_name = f"{name}_{timestamp}{ext}"
        archive_path = os.path.join(dir_name, archive_name)
        
        shutil.copy2(LOGFILE, archive_path)
        with open(LOGFILE, 'w', encoding='utf-8') as f:
            f.write(f"--- Logs archivés le {timestamp} ---\n")
            
        return jsonify({"success": True, "filename": archive_name})
    except Exception as e:
        return jsonify({"success": False, "error": str(e)}), 500

@app.route('/api/admin/restart', methods=['POST'])
def restart_admin():
    def delayed_restart():
        time.sleep(1)
        os._exit(42)

    threading.Thread(target=delayed_restart).start()
    return jsonify({"success": True, "message": "Redémarrage du processus en cours..."})

@app.route('/api/watcher/status', methods=['GET'])
def get_watcher_status():
    if manager:
        if manager.is_watcher_running():
            return jsonify({
                "running": True, 
                "status": "ok", 
                "pid": os.getpid()
            })
        elif getattr(manager, 'watcher_error', None):
            return jsonify({
                "running": False, 
                "status": "crashed", 
                "error": manager.watcher_error
            })
    return jsonify({"running": False, "status": "stopped"})

@app.route('/api/watcher/state', methods=['GET'])
def get_watcher_state():
    if not manager:
        return jsonify({"state": {}})
    
    now = time.time()
    res = {}
    
    with manager.strike_lock:
        for tag, state in manager.logs_state.items():
            counts = sum(state.get("counts", {}).values())
            action_ts = state.get("action", 0)
            last_ts = state.get("last_count_time", 0)
            
            if action_ts > 0:
                status = "En pause"
                time_left = max(0, int((action_ts + WATCHER_PAUSE_TIME) - now))
            else:
                status = "En observation"
                time_left = max(0, int((last_ts + WATCHER_STRIKE_TIME) - now))
                
            res[tag] = {
                "errors": state.get("counts", {}),
                "total_strikes": counts,
                "status": status,
                "time_left": time_left
            }
            
    return jsonify({"state": res})

@app.route('/api/watcher/<action>', methods=['POST'])
def manage_watcher(action):
    if not manager or not manager_thread or not manager_thread.is_alive():
        return jsonify({"success": False, "error": "Le daemon principal est arrêté"})

    if action == 'start':
        manager.start_watcher()
        return jsonify({"success": True})
    elif action == 'stop':
        manager.stop_watcher()
        return jsonify({"success": True})
    elif action == 'restart':
        manager.stop_watcher()
        time.sleep(3)
        manager.start_watcher()
        return jsonify({"success": True})

    return jsonify({"success": False, "error": "Action invalide"})

INITIAL_ENV = {
    'REFRESH_INTERVAL': REFRESH_INTERVAL,
    'DRY_RUN': DRY_RUN,
    'LOGS_WATCHER': LOGS_WATCHER,
    'WATCHER_MAX_STRIKE': WATCHER_MAX_STRIKE,
    'WATCHER_STRIKE_TIME': WATCHER_STRIKE_TIME,
    'WATCHER_PAUSE_TIME': WATCHER_PAUSE_TIME,
    'TOR_KEEP_LAST': TOR_KEEP_LAST
}
READONLY_KEYS = {
    'PORT', 'ADMIN_PORT', 'RUSTATIO_API', 'AUTH_TOKEN', 
    'ARCHIVE_FOLDER', 'RULES_FILE', 'DEFAULTS_FILE', 
    'LOGFILE', 'CHECK_LOGS_FILE'
}

@app.route('/api/env', methods=['GET', 'POST'])
def handle_env_config():
    if request.method == 'POST':
        data = request.json or {}

        forbidden_attempts = [key for key in data if key in READONLY_KEYS]
        if forbidden_attempts:
            return jsonify({
                "status": "forbidden",
                "message": f"Modification interdite pour : {', '.join(forbidden_attempts)}"
            }), 403

        for key, val in data.items():
            if key in globals() and key not in READONLY_KEYS:
                if key in ('LOGS_WATCHER', 'TOR_KEEP_LAST'):
                    globals()[key] = 1 if str(val) in ('1', 'true', 'True') else 0
                elif isinstance(globals()[key], bool) or key == 'DRY_RUN':
                    if isinstance(val, str):
                        globals()[key] = val.lower() == 'true'
                    else:
                        globals()[key] = bool(val)
                elif isinstance(globals()[key], int):
                    globals()[key] = int(val)
                elif isinstance(globals()[key], float):
                    globals()[key] = float(val)
                else:
                    globals()[key] = str(val)

        return jsonify({"status": "success", "message": "Variables mises à jour temporairement"})

    config_keys = [
        'PORT', 'ADMIN_PORT', 'RUSTATIO_API', 'AUTH_TOKEN',
        'REFRESH_INTERVAL', 'ARCHIVE_FOLDER', 'RULES_FILE',
        'DEFAULTS_FILE', 'DRY_RUN', 'LOGFILE', 'CHECK_LOGS_FILE',
        'LOGS_WATCHER', 'WATCHER_MAX_STRIKE', 'WATCHER_STRIKE_TIME',
        'WATCHER_PAUSE_TIME', 'TOR_KEEP_LAST'
    ]
    
    return jsonify({
        "config": {
            k: (int(globals().get(k)) if k in ('LOGS_WATCHER', 'TOR_KEEP_LAST') else globals().get(k))
            for k in config_keys
        },
        "readonly": list(READONLY_KEYS)
    })

@app.route('/api/env/reset', methods=['POST'])
def reset_env_config():
    # Restauration des variables d'origine
    for key, val in INITIAL_ENV.items():
        if key in globals() and key not in READONLY_KEYS:
            globals()[key] = val
            
    return jsonify({"status": "success", "message": "Configuration réinitialisée aux valeurs d'origine"})

def run_flask_app():
    log_werkzeug = logging.getLogger('werkzeug')
    log_werkzeug.setLevel(logging.ERROR)
    
    log(f"Admin web panel started on port {ADMIN_PORT}", "start")
    app.run(host='0.0.0.0', port=ADMIN_PORT, debug=False, use_reloader=False)

if __name__ == "__main__":
    manager = RustatioManager()
    manager_thread = threading.Thread(target=manager.run, daemon=True)
    manager_thread.start()

    run_flask_app()
