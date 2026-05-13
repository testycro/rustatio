#!/usr/bin/env bash
set -euo pipefail
BASE=$(basename "${0}")





# Configuration (à adapter)
RUSTATIO_API="http://127.0.0.1:${PORT}"     # A adapter seulement si le script est lancé en dehors du container de Rustatio

REFRESH_INTERVAL=0                          # Temps en seconds entre chaques traitement des règles de RULES_FILE. Minimum 5s => 0 pour utiliser Scrape Interval de la config par défault

ARCHIVE_FOLDER="/data/archived"             # Chemin vers le dossier d'archivage des .torrent

RULES_FILE="/data/rules.txt"                # Chemin vers le fichier des règles. Se charge au démarrage ou lorsque LOGFILE est suprimé

DEFAULTS_FILE="/data/state.json"            # Chemin vers le fichier state.json contenant default_config, etc. Se charge au démarrage ou lorsque LOGFILE est suprimé

DRY_RUN=false                               # Aucuns appel API, test seulement true/false

LOGFILE="/data/${BASE%.*}.log"              # Chemins vers le fichier log ou /dev/null pour désactiver
#LOGFILE="/dev/null"

LOGS_WATCHER=1                              # Activer/désactiver (1/0) la détection d'erreur dans les logs

WATCHER_MAX_STRIKE=3                        # Nombre d'erreurs récurentes avant de mettre en pause un torrent

WATCHER_STRIKE_TIME=3600                    # Durée de validitée d'un strike si max pas atteint

WATCHER_PAUSE_TIME=3600                     # Durée de la pause du torrent avant reprise en secondes

TOR_KEEP_LAST=1                             # Ne pas suprimer ou arrêter le dernier torrent d'un tracker (Garder une activitée dans le compte)





log() {
    local log_time="$(date "+%d-%m-%Y %H:%M:%S")"
    local message="${1}"
    local style="${2:-default}"
    local prefix=""

    if [[ "${style}" == ff_* ]]; then
        prefix="          └─ "
        style="${style#ff_}"
    fi

    if [[ "${style}" == f_* ]]; then
        prefix="   └─ "
        style="${style#f_}"
    fi

    case "${style}" in
        start)      prefix="${prefix}🚀 " ;;
        error)      prefix="${prefix}❌ " ;;
        succes)  prefix="${prefix}✅️ " ;;
        warning)    prefix="${prefix}⚠️ " ;;
        denied)  prefix="${prefix}🚫 " ;;
        saving)  prefix="${prefix}💾 " ;;
        data)        prefix="${prefix}🧪 " ;;
        lock)        prefix="${prefix}🔒 " ;;
        recycle)    prefix="${prefix}♻️ " ;;
        task)        prefix="${prefix}⚡ " ;;
        finish)  prefix="${prefix}🏁 " ;;
        default|*)  prefix="${prefix}";;
    esac

    if [[ -n "${3}" ]]; then
        if [[ "${3}" == 1 ]]; then
            printf '%s\n' "${log_time} :: ${prefix}${message}" >> "${LOGFILE}"
        else
            printf '%s\n' "${message}" >> "${LOGFILE}"
        fi
    else
        echo "${log_time} :: ${prefix}${message}"
    fi
}

rotate_logs() {
    local MAX_SIZE=$((1024 * 1024))

    if [[ -f "${LOGFILE}" && $(stat -c%s "${LOGFILE}") -ge ${MAX_SIZE} ]]; then
        local BASE="${LOGFILE%.*}"
        local EXT="${LOGFILE##*.}"
        local OLD_WATCHER="${LOGS_WATCHER}"

        if (( LOGS_WATCHER != 0 )) && [[ -n "${CHECK_LOGS_PID}" ]] && kill -0 "${CHECK_LOGS_PID}" 2>/dev/null; then
            LOGS_WATCHER=0

            kill -TERM "${CHECK_LOGS_PID}" 2>/dev/null
            wait "${CHECK_LOGS_PID}" 2>/dev/null
            unset CHECK_LOGS_PID
        fi

        local MAX_INDEX=0
        local F I

        shopt -s nullglob
        local FILES=( "${BASE}".*.${EXT} )
        shopt -u nullglob

        for F in "${FILES[@]}"; do
            if [[ "${F}" =~ \.([0-9]+)\.${EXT}$ ]]; then
                (( BASH_REMATCH[1] > MAX_INDEX )) && MAX_INDEX=${BASH_REMATCH[1]}
            fi
        done

        if (( MAX_INDEX >= 0 )) && [[ -f "${BASE}.${MAX_INDEX}.${EXT}" ]]; then
            mv "${BASE}.${MAX_INDEX}.${EXT}" "${BASE}.$((MAX_INDEX+1)).${EXT}"
        fi

        for (( I=MAX_INDEX-1; I>=0; I-- )); do
            if [[ -f "${BASE}.${I}.${EXT}" ]]; then
                mv "${BASE}.${I}.${EXT}" "${BASE}.$((I+1)).${EXT}"
            fi
        done

        mv "${LOGFILE}" "${BASE}.0.${EXT}"

       LOGS_WATCHER="${OLD_WATCHER}"
    fi
}

format_time() {
    local t=$1
    local h=$(( t / 3600 ))
    local m=$(( (t % 3600) / 60 ))

    if (( h > 0 )); then
        (( m > 0 )) && printf "%dh%02d" "$h" "$m" || printf "%dh" "$h"
    else
        printf "%dmin" "$m"
    fi
}

extract_bracket() {
    local S="${1}"
    local M="${2}"
    local EB_I=""
    local EB_R=""

    local START=$(expr index "${S}" '[')
    if [ "${START}" -eq 0 ]; then
        EB_I=""
        EB_R="${S}"
        return 0
    fi

    local LEN=${#S}
    local LEVEL=0
    local END=-1
    local I C

    for ((I=START-1; I<LEN; I++)); do
        C=${S:I:1}
        if [ "${C}" = "[" ]; then
          ((LEVEL++))
        elif [ "${C}" = "]" ]; then
            ((LEVEL--))
            if [ "${LEVEL}" -eq 0 ]; then
                END=${I}
                break
            fi
        fi
    done

    if [ "${END}" -lt 0 ]; then
        EB_I="${S:START}"
        EB_R=""
        return 0
    fi

    if [[ "${M}" == "I" ]]; then
        EB_I=$(_trim "${S:START:((END-START))}")
        printf '%s' "${EB_I}"
    fi
    if [[ "${M}" == "R" ]]; then
        EB_R=${S:END+1}
        EB_R=$(_trim "${S:END+1}")
        printf '%s' "${EB_R}"
    fi
}

cleanup() {
    if [[ -n "${PIDFILE}" && "${PIDFILE}" != "/dev/null" && -f "${PIDFILE}" ]]; then
        rm -f "${PIDFILE}"
    fi
}

url_encode() {
    local S="${1}"
    printf '%s' "${S}" | jq -s -R -r @uri
}

is_valid_json() {
    local S="${1}"

    if [[ -z "${S//[[:space:]]/}" ]]; then
        return 1
    fi

    if printf '%s' "${S}" | jq -e . >/dev/null 2>&1; then
        return 0
    else
        return 1
    fi
}

bytes_to_hex() {
    local BYTES="${1}"
    local HEX=""
    for N in ${BYTES}; do
        if ! [[ "${N}" =~ ^[0-9]+$ ]] || [ "${N}" -lt 0 ] || [ "${N}" -gt 255 ]; then
            log "Invalid byte '${N}' ignored" warning
            continue
        fi
        printf -v H "%02x" "$((10#${N}))"
        HEX+="${H}"
    done
    printf '%s' "${HEX}"
}

rustatio_api_request() {
    set +e
    local WAY="${1}"
    local PAYLOAD="${2}"
    local METHOD="${3}"
    local ATTEMPT=1
    local MAX_RETRIES=3
    local RESPONSE=""

    local CURL_EXIT=0

    while (( ATTEMPT <= MAX_RETRIES )); do
        if [ -z "${PAYLOAD}" ]; then
            RESPONSE=$(curl --fail -S -s -X ${METHOD} "${RUSTATIO_API}/api/${WAY}" \
                    -H "Accept: application/json" 2>&1)
            CURL_EXIT=$?
        else
            RESPONSE=$(printf '%s' "${PAYLOAD}" | \
                curl --fail -S -s -X ${METHOD} "${RUSTATIO_API}/api/${WAY}" \
                    -H "Content-Type: application/json" \
                    -H "Accept: application/json" \
                    --data-binary @- 2>&1)
            CURL_EXIT=$?
        fi

        if [ ${CURL_EXIT} -eq 0 ]; then
            if is_valid_json "${RESPONSE}"; then
                if jq -e -c '.success == true' <<< "${RESPONSE}" >/dev/null 2>&1; then
                    if jq -e -c '(.data and .data != {} and .data != []) or (.stats and .stats != {}) or (.config and .config != {})' <<< "${RESPONSE}" >/dev/null 2>&1; then
                        printf '%s\n' "${RESPONSE}"
                    fi

                    set -e
                    return 0;
                fi
            fi

            log "${RESPONSE}" f_error
            return 1
        fi
        log "Attempt ${ATTEMPT} failed (curl exit ${CURL_EXIT}). Retrying in 2 seconds..." f_error
        sleep 2
        (( ATTEMPT++ ))
    done

    log "Failed to ${METHOD} ${WAY} after ${MAX_RETRIES} attempts. Last curl exit: ${CURL_EXIT}" f_error
    set -e
    return 1
}

check_logs() {
    if [[ -n "${CHECK_LOGS_PID}" ]] && kill -0 "${CHECK_LOGS_PID}" 2>/dev/null; then
        return
    fi

    sleep 0.5
    (
        CHECK_LOGS_FILE="${RULES_FILE%/*}/check_logs.json"
        CHECK_LOGS_LE_TS=$(date +%s)

        log "Start checking logs" task 1
        log "${CHECK_LOGS_FILE}" f_data 1

        if [[ -n "${CHECK_LOGS_FILE}" && -s "${CHECK_LOGS_FILE}" ]]; then
            log "Recovering logs data" f_recycle 1
            JSONLOGS_FINAL=$(<"${CHECK_LOGS_FILE}")
        fi

        if is_valid_json "${JSONLOGS_FINAL}"; then
            JSONLOGS="${JSONLOGS_FINAL}"
			log "Recover succeeded" ff_succes 1
        else
            JSONLOGS="{}"
        fi

        exec 3< <(curl -sN --max-time 3660 --retry 3 --retry-delay 1 "${RUSTATIO_API}/api/logs" | tr -d '\r')
        CHECK_LOGS_CURL_PID=$!

        while true; do
            if IFS= read -r -t 0.5 CHECK_LOGS_LINE <&3; then
                CHECK_LOGS_LE_TS=$(date +%s)
            else
                CHECK_LOGS_LINE=""
            fi

            if ! kill -0 "$$" 2>/dev/null; then
                kill -KILL "${CHECK_LOGS_CURL_PID}" 2>/dev/null || true
                exit 0
            fi

            if ! kill -0 "${CHECK_LOGS_CURL_PID}" 2>/dev/null; then
                exit 0
            fi

            CHECK_LOGS_DIRTY=0
            CHECK_LOGS_NOW=$(date +%s)

            if (( CHECK_LOGS_NOW - CHECK_LOGS_LE_TS > 900 )); then
                kill -TERM "${CHECK_LOGS_CURL_PID}" 2>/dev/null || true
                wait "${CHECK_LOGS_CURL_PID}" 2>/dev/null
                break
            fi

            CHECK_LOGS_INSTANCES_JSON="$(rustatio_get_instances 2>&1)"
            CHECK_LOGS_RET=$?

            if ! is_valid_json "${CHECK_LOGS_INSTANCES_JSON}" && (( CHECK_LOGS_RET != 0 )); then
                log "INSTANCES_JSON invalid or non-JSON" error 1
                log "${CHECK_LOGS_INSTANCES_JSON}" f_data 1
                break
            fi

            CHECK_LOGS_NAMES_JSON=$(jq -c '[.data[].torrent.name] // []' <<<"${CHECK_LOGS_INSTANCES_JSON}")
            CHECK_LOGS_NEW_JSONLOGS=$(jq --argjson names "${CHECK_LOGS_NAMES_JSON}" 'if (type=="object") then with_entries(select(.key as $k | $names | index($k))) else {} end' <<<"${JSONLOGS}")

            if [[ "${CHECK_LOGS_NEW_JSONLOGS}" != "${JSONLOGS}" ]]; then
                JSONLOGS="${CHECK_LOGS_NEW_JSONLOGS}"

                CHECK_LOGS_DIRTY=1
            fi

            mapfile -t CHECK_LOGS_EXPIRED_TAGS < <(jq -r --argjson wst "${WATCHER_STRIKE_TIME}" --argjson now "${CHECK_LOGS_NOW}" 'to_entries[] | select((.value.last_count_time|type) == "number" and .value.last_count_time > 0 and ($now - .value.last_count_time) > $wst and ((.value.action|type) != "number" or .value.action == 0)) | .key' <<<"${JSONLOGS}")

            if (( ${#CHECK_LOGS_EXPIRED_TAGS[@]} > 0 )); then
                for CHECK_LOGS_TAG in "${CHECK_LOGS_EXPIRED_TAGS[@]}"; do
                    JSONLOGS=$(jq --arg tag "${CHECK_LOGS_TAG}" 'del(.[$tag])' <<<"${JSONLOGS}")

                    CHECK_LOGS_DIRTY=1
                done
            fi

            mapfile -t CHECK_LOGS_EXPIRED_TAGS < <(jq -r --argjson wpt "${WATCHER_PAUSE_TIME}" --argjson now "${CHECK_LOGS_NOW}" 'to_entries[] | select((.value.action|type) == "number" and .value.action > 0 and ($now - .value.action) > $wpt) | .key' <<<"${JSONLOGS}")

            if (( ${#CHECK_LOGS_EXPIRED_TAGS[@]} > 0 )); then
                for CHECK_LOGS_TAG in "${CHECK_LOGS_EXPIRED_TAGS[@]}"; do
                    CHECK_LOGS_INST=$(jq -r --arg t "${CHECK_LOGS_TAG}" '.data[] | select(.torrent.name == $t)' <<<"${CHECK_LOGS_INSTANCES_JSON}")
                    CHECK_LOGS_STATE=$(jq -r '.stats.state // empty' <<<"${CHECK_LOGS_INST}")

                    if is_action_valid "resume" "${CHECK_LOGS_STATE}"; then
                        log "Pause ended. Try to resume and remove tag" warning 1

                        CHECK_LOGS_ACTION_TS=$(jq -r --arg tag "${CHECK_LOGS_TAG}" '.[$tag].action // 0' <<<"${JSONLOGS}")
                        CHECK_LOGS_ACTION_TS="$(format_time "$(( CHECK_LOGS_ACTION_TS - $(date -d "$(date -d "@${CHECK_LOGS_ACTION_TS}" +%F) 00:00" +%s) ))")"

                        CHECK_LOGS_OUT="$(run_action_for_instance "resume" "${CHECK_LOGS_INST}" "" 2>&1)"

                        if [[ -n "${CHECK_LOGS_OUT//[[:space:]]/}" ]]; then
                            log "Torrent name : ${CHECK_LOGS_TAG}" f_data 1

                            if is_valid_json "${CHECK_LOGS_OUT}"; then
                                log "Resume succeeded" f_succes 1
                            else
                                log "${CHECK_LOGS_OUT}" "" 2
                            fi
                        fi

                        CHECK_LOGS_OUT="$(run_action_for_instance "removetags" "${CHECK_LOGS_INST}" "Err ${CHECK_LOGS_ACTION_TS}" 2>&1)"

                        if [[ -n "${CHECK_LOGS_OUT//[[:space:]]/}" ]]; then
                            if is_valid_json "${CHECK_LOGS_OUT}"; then
                                log "Tags removed (Err ${CHECK_LOGS_ACTION_TS})" f_succes 1
                            else
                                log "${CHECK_LOGS_OUT}" "" 2
                            fi
                        fi

                        JSONLOGS=$(jq --arg tag "${CHECK_LOGS_TAG}" 'del(.[$tag])' <<<"${JSONLOGS}")

                        CHECK_LOGS_DIRTY=1
                    fi
                done
            fi

            case "${CHECK_LOGS_LINE}" in
                data:*)
                    CHECK_LOGS_JSON="${CHECK_LOGS_LINE#data: }"

                    CHECK_LOGS_LEVEL=$(jq -r '.level // empty' <<<"${CHECK_LOGS_JSON}" 2>/dev/null || true)
                    CHECK_LOGS_MESSAGE=$(jq -r '.message // empty' <<<"${CHECK_LOGS_JSON}" 2>/dev/null || true)

                    if [[ "${CHECK_LOGS_LEVEL}" == *"error"* ]] && [[ "${CHECK_LOGS_MESSAGE}" == *'['*']'* ]]; then
                        CHECK_LOGS_TAG="$(extract_bracket "${CHECK_LOGS_MESSAGE}" "I")"
                        CHECK_LOGS_REST="$(extract_bracket "${CHECK_LOGS_MESSAGE}" "R")"

                        CHECK_LOGS_CUR_COUNT=$(jq -r --arg tag "${CHECK_LOGS_TAG}" 'if .[$tag] then .[$tag].count else 0 end' <<<"${JSONLOGS}")
                        CHECK_LOGS_CUR_ACTION=$(jq -r --arg tag "${CHECK_LOGS_TAG}" 'if .[$tag] then .[$tag].action else 0 end' <<<"${JSONLOGS}")

                        if (( CHECK_LOGS_CUR_ACTION == 0 )); then
                            JSONLOGS=$(jq --arg tag "${CHECK_LOGS_TAG}" --arg rest "${CHECK_LOGS_REST}" --argjson ltime "${CHECK_LOGS_NOW}" '.[$tag].rest |= ((. // []) + [$rest]) | .[$tag] |= (. // {count:0, action:0, last_count_time:0}) | .[$tag].count = (.[$tag].rest | map(select(. == $rest)) | length) | .[$tag].last_count_time = $ltime' <<<"${JSONLOGS}")

                            CHECK_LOGS_DIRTY=1

                            CHECK_LOGS_NEW_COUNT=$(jq -r --arg tag "${CHECK_LOGS_TAG}" '.[$tag].count // 0' <<<"${JSONLOGS}")

                            if (( CHECK_LOGS_NEW_COUNT >= WATCHER_MAX_STRIKE )); then

                                mapfile -t CHECK_LOGS_MATCHES < <(jq -c --arg t "${CHECK_LOGS_TAG}" '.data[] | select(.torrent.name == $t)' <<<"${CHECK_LOGS_INSTANCES_JSON}")

                                if ! [[ "${#CHECK_LOGS_MATCHES[@]}" -eq 0 ]]; then
                                    for CHECK_LOGS_INST in "${CHECK_LOGS_MATCHES[@]}"; do
                                        CHECK_LOGS_STATE=$(jq -r '.stats.state // empty' <<<"${CHECK_LOGS_INST}")

                                        if is_action_valid "pause" "${CHECK_LOGS_STATE}"; then
                                            log "Repeated error detected (x${WATCHER_MAX_STRIKE}). Try to pause for $(format_time "${WATCHER_PAUSE_TIME}") and add tag" warning 1
                                            log "Torrent name : ${CHECK_LOGS_TAG}" f_data 1
                                            log "${CHECK_LOGS_REST}" f_data 1

                                            CHECK_LOGS_TODAY="$(format_time "$(( CHECK_LOGS_NOW - $(date -d "$(date -d "@${CHECK_LOGS_NOW}" +%F) 00:00" +%s) ))")"

                                            CHECK_LOGS_OUT="$(run_action_for_instance "pause" "${CHECK_LOGS_INST}" "" 2>&1)"

                                            if [[ -n "${CHECK_LOGS_OUT//[[:space:]]/}" ]]; then
                                                if is_valid_json "${CHECK_LOGS_OUT}"; then
                                                    log "Pause succeeded" f_succes 1
                                                else
                                                    log "${CHECK_LOGS_OUT}" "" 2
                                                fi
                                            fi

                                            CHECK_LOGS_OUT="$(run_action_for_instance "addtags" "${CHECK_LOGS_INST}" "Err ${CHECK_LOGS_TODAY}" 2>&1)"

                                            if [[ -n "${CHECK_LOGS_OUT//[[:space:]]/}" ]]; then
                                                if is_valid_json "${CHECK_LOGS_OUT}"; then
                                                    log "Tags applied (Err ${CHECK_LOGS_TODAY})" f_succes 1
                                                else
                                                    log "${CHECK_LOGS_OUT}" "" 2
                                                fi
                                            fi

                                            JSONLOGS=$(jq --arg tag "${CHECK_LOGS_TAG}" --argjson atime "${CHECK_LOGS_NOW}" '.[$tag].action = $atime' <<<"${JSONLOGS}")

                                            CHECK_LOGS_DIRTY=1
                                        fi
                                    done
                                fi
                            fi
                        fi
                    fi
                ;;
                :*) ;;
                "") ;;
                *) ;;
            esac

            if is_valid_json "${JSONLOGS}" && (( CHECK_LOGS_DIRTY )); then
                sleep 0.5

                CHECK_LOGS_TMP_FILE="$(mktemp "${CHECK_LOGS_FILE}.XXXXXX")"

                printf '%s\n' "${JSONLOGS}" > "${CHECK_LOGS_TMP_FILE}"
                sync -f "${CHECK_LOGS_TMP_FILE}" 2>/dev/null || true

                mv -f "${CHECK_LOGS_TMP_FILE}" "${CHECK_LOGS_FILE}"

                CHECK_LOGS_DIRTY=0

                sleep 0.5
            fi
        done <&3

        kill -TERM "${CHECK_LOGS_CURL_PID}" 2>/dev/null
        wait "${CHECK_LOGS_CURL_PID}" 2>/dev/null
    ) &

    sleep 0.5

    CHECK_LOGS_PID=$!
    kill -0 "${CHECK_LOGS_PID}" 2>/dev/null || unset CHECK_LOGS_PID
}

rustatio_get_instances() {
    rustatio_api_request "instances" "" "GET"
}

rustatio_delete_instance() {
    rustatio_api_request "instances/${1}?force=true" "" "DELETE"
}

rustatio_pause_instance() {
    rustatio_api_request "grid/pause" "{\"ids\":[\"${1}\"]}" "POST"
}

rustatio_resume_instance() {
    rustatio_api_request "grid/resume" "{\"ids\":[\"${1}\"]}" "POST"
}

rustatio_stop_instance() {
    rustatio_api_request "faker/${1}/stop" "" "POST"
}

rustatio_start_instance() {
    rustatio_api_request "faker/${1}/start" "${2}" "POST"
}

rustatio_patch_instance() {
    rustatio_api_request "instances/${1}/config" "${2}" "PATCH"
}

rustatio_delete_file() {
    local ENCODED

    ENCODED=$(url_encode "${1}")
    rustatio_api_request "watch/files?path=${ENCODED}" "" "DELETE"
}

rustatio_get_files() {
    rustatio_api_request "watch/files" "" "GET"
}

rustatio_tags() {
    rustatio_api_request "grid/tag" "${1}" "POST"
}

load_rules_file() {
    local F="${1:-${RULES_FILE}}"

    if [[ -f "${F}" ]]; then
        RULES_TEXT=$(sed -e 's/\r$//' "${F}")
        log "Rules loaded from ${F}" start
    else
        log "Rules file ${F} not found" error
    fi
}

load_defaults_file() {
    local F="${1:-${DEFAULTS_FILE}}"

    if [[ -f "${F}" ]]; then
        local RAW

        RAW="$(sed -e 's/\r$//' "${F}")"

        if ! is_valid_json "${RAW}"; then
            log "Defaults file ${F} is not valid JSON" error
            GLOBAL_DEFAULTS_JSON="{}"
            return
        fi

        if jq -e '.default_config' >/dev/null 2>&1 <<<"${RAW}"; then
            GLOBAL_DEFAULTS_JSON="$(jq -c '.default_config' <<<"${RAW}")"
        else
            GLOBAL_DEFAULTS_JSON="$(jq -c '.' <<<"${RAW}")"
        fi

        log "Defaults loaded from ${F}" start
    else
        log "Defaults file ${F} not found" error
        GLOBAL_DEFAULTS_JSON="{}"
    fi
}

_trim() { sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' <<<"${1}"; }

parse_rule_line() {
    local LINE="${1}"

    LINE="${LINE%%#*}"
    LINE="$(_trim "${LINE}")"

    [[ -z "${LINE}" ]] && return 1

    IFS='|' read -r COND ACTION ASSIGN <<<"${LINE}"

    printf '%s\x1F%s\x1F%s' "$(_trim "${COND}")" "$(_trim "${ACTION}")" "$(_trim "${ASSIGN}")"
}

resolve_default_config() {
    local RHS="${1}"
    local KEY VAL

    local RANGE_REGEX='^default_config\.([A-Za-z0-9_.]+)$'

    if [[ ${RHS} =~ ${RANGE_REGEX} ]]; then
        KEY="${BASH_REMATCH[1]}"
        VAL="$(jq -c --arg k "${KEY}" '.[$k] // "__MISSING__"' <<<"${GLOBAL_DEFAULTS_JSON}")"

        if [[ "${VAL}" == '"__MISSING__"' ]]; then
            return 1
        fi

        printf '%s' "${VAL}"
        return 0
    fi

    printf '%s' "${RHS}"
}

validate_rule_keys() {
    local EXPR="${1}"
    local SAMPLE_INST="${2}"
    local RAW_KEYS=()
    local INST_KEYS=()

    check_key_in_json() {
        local KEY="${1}"
        local SAMPLE_INST_JSON="$(printf '%s' "${2}" | base64 -d)"
        local ERR_PREFIX="${3}"
        local JQ_EXPR=""
        local PREFIX=""
        local P PARTS JQ_ARRAY VAL

        IFS='.' read -r -a PARTS <<< "${KEY}"

        for I in "${!PARTS[@]}"; do
            P="${PARTS[I]//\"/\\\"}"
            if [[ ${I} -eq 0 ]]; then
                JQ_EXPR="has(\"${P}\")"
                PREFIX=".${P}"
            else
                JQ_EXPR+=" and (${PREFIX} | has(\"${P}\"))"
                PREFIX+=".${P}"
            fi
        done

        if ! jq -e "${JQ_EXPR}" <<<"${SAMPLE_INST_JSON}" >/dev/null 2>&1; then
            read -r -a PARTS <<< "$(sed 's/\./ /g' <<<"${KEY}")"
            JQ_ARRAY=$(printf '"%s",' "${PARTS[@]}" | sed 's/,$//')
            VAL="$(jq -c "getpath([${JQ_ARRAY}]) // \"__MISSING__\"" <<<"${SAMPLE_INST_JSON}" 2>/dev/null || echo null)"
            if [[ "${VAL}" == '"__MISSING__"' ]]; then
                log "${ERR_PREFIX}${KEY}' not found" f_error
                return 1
            fi
        fi
        return 0
    }

    mapfile -t RAW_KEYS < <(grep -oE 'default_config\.([A-Za-z0-9_\.]+)' <<<"${EXPR}" | sed 's/^default_config\.//' | sort -u)
    for K in "${RAW_KEYS[@]}"; do
        if ! check_key_in_json "${K}" "$(printf '%s' "${GLOBAL_DEFAULTS_JSON}" | base64 -w0)" "default_config key 'default_config."; then
            return 1
        fi
    done

    mapfile -t RAW_KEYS < <(grep -oE '([A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)+)' <<<"${EXPR}" | sort -u | grep -v '^default_config\.')
    for K in "${RAW_KEYS[@]}"; do
        if ! check_key_in_json "${K}" "${SAMPLE_INST}" "Instance key '"; then
            return 1
        fi
    done

    return 0
}

get_cached_rand() {
    local KEY="${1}"
    local NOW TS VAL

    NOW=$(date +%s)

    TS=$(jq -r --arg k "${KEY}" '.ts[$k] // 0' <<<"${RAND_CACHE_JSON}")
    if [[ "${TS}" =~ ^[0-9]+$ ]] && (( RAND_TTL == 0 || NOW - TS < RAND_TTL )); then
        VAL=$(jq -r --arg k "${KEY}" '.vals[$k] // empty' <<<"${RAND_CACHE_JSON}")
        if [[ -n "${VAL}" ]]; then
            printf '%s' "${VAL}"
            return 0
        fi
    fi
    return 1
}

set_cached_rand() {
    local KEY="${1}"
    local VAL="${2}"
    local NOW
    NOW=$(date +%s)

    RAND_CACHE_JSON=$(jq -c --arg k "${KEY}" --arg v "${VAL}" --argjson t "${NOW}" \
        '.vals[$k] = $v | .ts[$k] = ($t|tonumber)' <<<"${RAND_CACHE_JSON}")
}

set_used_rand() {
    local KEY="${1}"
    local VAL="${2}"
    local NOW
    NOW=$(date +%s)

    RAND_USED_JSON=$(jq -c --arg k "${KEY}" --arg v "${VAL}" --argjson t "${NOW}" \
        '.vals[$k] = $v | .ts[$k] = ($t|tonumber)' <<<"${RAND_USED_JSON}")
}

clear_cached_rand() {
    local KEY KEYS

    if ! is_valid_json "${RAND_USED_JSON}" || [[ "${RAND_USED_JSON}" == "{}" ]]; then
        return 0
    fi

    mapfile -t KEYS < <(jq -r '(.vals // {}) | keys[]' <<<"${RAND_USED_JSON}" 2>/dev/null || printf '')

    if [[ "${#KEYS[@]}" -eq 0 ]]; then
        return 0
    fi

    for KEY in "${KEYS[@]}"; do
        RAND_CACHE_JSON=$(jq -c "del(.vals[\"${KEY}\"], .ts[\"${KEY}\"])" <<<"${RAND_CACHE_JSON}")
    done

    RAND_USED_JSON='{"vals":{},"ts":{}}'
}

cond_to_jq() {
    local ORIG_EXPR="${1}"
    local EXPR="${ORIG_EXPR}"
    local VAL RAND KEY KEYS FULL_MATCH WAY A B

    local RANGE_REGEX='([A-Za-z0-9_.]+):[[:space:]]*([0-9]+(\.[0-9]+)?) *- *([0-9]+(\.[0-9]+)?)'

    if [[ ! ${EXPR} =~ ${RANGE_REGEX} ]]; then
        local KEY_HASH="$(printf '%s' "${ORIG_EXPR}" | md5sum | awk '{print $1}')"
        if [[ -n "${COND_CACHE[${KEY_HASH}]:-}" ]]; then
            JQCOND="${COND_CACHE[${KEY_HASH}]}"
            return 0
        fi
    fi

    RAND_USED_JSON='{"vals":{},"ts":{}}'
    EXPR="$(echo "${EXPR}" | sed -E 's/\bAND\b/ and /g; s/\bOR\b/ or /g')"

    while [[ ${EXPR} =~ ${RANGE_REGEX} ]]; do
        FULL_MATCH="${BASH_REMATCH[0]}"
        WAY="${BASH_REMATCH[1]}"
        A="${BASH_REMATCH[2]}"
        B="${BASH_REMATCH[4]}"

        KEY="$(printf '%s' "${WAY}:${A}:${B}:${FULL_MATCH}" | md5sum | awk '{print $1}')"
        if ! RAND="$(get_cached_rand "${KEY}")"; then
            RAND="$(awk -v a="${A}" -v b="${B}" 'BEGIN {
                if (a == b) { printf("%.2f", a); exit }
                if (a > b) { t = a; a = b; b = t }
                srand(systime() + PROCINFO["pid"])
                r = a + rand() * (b - a)
                printf("%.2f", r)
            }')"

            set_cached_rand "${KEY}" "${RAND}"
        fi

        set_used_rand "${KEY}" "${RAND}"

        EXPR="${EXPR/"${FULL_MATCH}"/((.${WAY} // 0) | tonumber) > ${RAND}}"
    done

    EXPR="$(echo "${EXPR}" | sed -E \
    -e 's#torrent\.announce[[:space:]]*~[[:space:]]*\"([^\"]+)\"#((.torrent.announce // \"\") | tostring | ascii_downcase) | contains(\"\1\")#g' \
    -e 's#torrent\.announce[[:space:]]*~[[:space:]]*([A-Za-z0-9_@./:-]+)#((.torrent.announce // \"\") | tostring | ascii_downcase) | contains(\"\1\")#g' \
    -e 's#torrent\.name[[:space:]]*~[[:space:]]*\"([^\"]+)\"#((torrent.name // \"\") | tostring | ascii_downcase) | contains(\"\1\")#g' \
    -e 's#torrent\.name[[:space:]]*~[[:space:]]*([A-Za-z0-9_@./:-]+)#((torrent.name // \"\") | tostring | ascii_downcase) | contains(\"\1\")#g' \
    -e 's#torrent\.comment[[:space:]]*~[[:space:]]*\"([^\"]+)\"#((torrent.comment // \"\") | tostring | ascii_downcase) | contains(\"\1\")#g' \
    -e 's#torrent\.comment[[:space:]]*~[[:space:]]*([A-Za-z0-9_@./:-]+)#((torrent.comment // \"\") | tostring | ascii_downcase) | contains(\"\1\")#g' \
    -e 's#torrent\.created_by[[:space:]]*~[[:space:]]*\"([^\"]+)\"#((torrent.created_by // \"\") | tostring | ascii_downcase) | contains(\"\1\")#g' \
    -e 's#torrent\.created_by[[:space:]]*~[[:space:]]*([A-Za-z0-9_@./:-]+)#((torrent.created_by // \"\") | tostring | ascii_downcase) | contains(\"\1\")#g')"

    EXPR="$(echo "${EXPR}" | sed -E \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*!=[[:space:]]*([0-9]+(\.[0-9]+)?)#((.\1 // 0) | tonumber) != \2#g' \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*>=[[:space:]]*([0-9]+(\.[0-9]+)?)#((.\1 // 0) | tonumber) >= \2#g' \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*<=[[:space:]]*([0-9]+(\.[0-9]+)?)#((.\1 // 0) | tonumber) <= \2#g' \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*<[[:space:]]*([0-9]+(\.[0-9]+)?)#((.\1 // 0) | tonumber) < \2#g' \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*>[[:space:]]*([0-9]+(\.[0-9]+)?)#((.\1 // 0) | tonumber) > \2#g')"

    EXPR="$(echo "${EXPR}" | sed -E \
    -e 's#tags[[:space:]]*!=[[:space:]]*\"?([A-Za-z0-9_@./:-]+)\"?#((.tags // []) | index("\1") == null)#g' \
    -e 's#tags:[[:space:]]*\"([^\"]+)\"#((.tags // []) | index("\1") != null)#g' \
    -e 's#tags:[[:space:]]*([A-Za-z0-9_@./:-]+)#((.tags // []) | index("\1") != null)#g' \
    -e 's#tags[[:space:]]*=[[:space:]]*\"([^\"]+)\"#((.tags // []) | index("\1") != null)#g' \
    -e 's#tags[[:space:]]*=[[:space:]]*([A-Za-z0-9_@./:-]+)#((.tags // []) | index("\1") != null)#g')"

    EXPR="$(echo "${EXPR}" | sed -E \
    -e 's#torrent\.info_hash:[[:space:]]*\"?([A-Fa-f0-9]+)\"?#((.torrent.info_hash // []) | map(printf("%02x"; .)) | join("") == "\1")#g' \
    -e 's#torrent\.info_hash[[:space:]]*\"?([A-Fa-f0-9]+)\"?#((.torrent.info_hash // []) | map(printf("%02x"; .)) | join("") == "\1")#g' \
    -e 's#torrent\.info_hash[[:space:]]*=[[:space:]]*\"?([A-Fa-f0-9]+)\"?#((.torrent.info_hash // []) | map(printf("%02x"; .)) | join("") == "\1")#g' \
    -e 's#torrent\.info_hash[[:space:]]*=[[:space:]]*?([A-Fa-f0-9]+)?#((.torrent.info_hash // []) | map(printf("%02x"; .)) | join("") == "\1")#g')"

    EXPR="$(echo "${EXPR}" | sed -E \
    -e 's#([a-zA-Z0-9_.]+):[[:space:]]*(true|false|null)#(.\1 == \2)#g' \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*=[[:space:]]*(true|false|null)#(.\1 == \2)#g')"

    EXPR="$(echo "${EXPR}" | sed -E \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*!=[[:space:]]*\"([^\"]+)\"#((.\1 // \"\") | tostring) != \"\2\"#g' \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*!=[[:space:]]*([A-Za-z0-9_@./:-]+)#((.\1 // \"\") | tostring) != \"\2\"#g' \
    -e 's#([a-zA-Z0-9_.]+):[[:space:]]*\"([^\"]+)\"#((.\1 // \"\") | tostring) == \"\2\"#g' \
    -e 's#([a-zA-Z0-9_.]+):[[:space:]]*([A-Za-z0-9_@./:-]+)#((.\1 // \"\") | tostring) == \"\2\"#g' \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*=[[:space:]]*\"([^\"]+)\"#((.\1 // \"\") | tostring) == \"\2\"#g' \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*=[[:space:]]*([A-Za-z0-9_@./:-]+)#((.\1 // \"\") | tostring) == \"\2\"#g')"

    mapfile -t KEYS < <(grep -oE 'default_config\.([A-Za-z0-9_\.]+)' <<<"${EXPR}" | sort -u)

    for KEY in "${KEYS[@]}"; do
        VAL="$(resolve_default_config "${KEY}")"
        EXPR="${EXPR//${KEY}/${VAL}}"
    done

    JQCOND="${EXPR}"

    if [[ ! ${ORIG_EXPR} =~ ${RANGE_REGEX} ]] && [[ -n "${KEY_HASH}" ]]; then
        COND_CACHE["${KEY_HASH}"]="${JQCOND}"
    fi

    return 0
}

action_pause() {
    local ID="${1}"
    local RESP

    if [[ "${DRY_RUN}" = true ]]; then
        log "Would pause ID='${ID}'" f_recycle
        return 0
    else
        if RESP="$(rustatio_pause_instance "${ID}" 2>&1)"; then
            PAYLOAD=$(jq -c '.data // {}' <<<"${RESP}")
            printf '%s' "${PAYLOAD}"
            return 0
        else
            log "Pause failed" f_error
            log "${RESP}" f_data
            return 1
        fi
    fi
}

action_resume() {
    local ID="${1}"
    local RESP

    if [[ "${DRY_RUN}" = true ]]; then
        log "Would resume ID='${ID}'" f_recycle
        return 0
    else
        if RESP="$(rustatio_resume_instance "${ID}" 2>&1)"; then
            PAYLOAD=$(jq -c '.data // {}' <<<"${RESP}")
            printf '%s' "${PAYLOAD}"
            return 0
        else
            log "Resume failed" f_error
            log "${RESP}" f_data
            return 1
        fi
    fi
}

action_stop() {
    local ID="${1}"
    local RESP

    if [[ "${DRY_RUN}" = true ]]; then
        log "Would stop ID='${ID}'" f_recycle
        return 0
    else
        if RESP="$(rustatio_stop_instance "${ID}" 2>&1)"; then
            PAYLOAD=$(jq -c '.data // {}' <<<"${RESP}")
            printf '%s' "${PAYLOAD}"
            return 0
        else
            log "Stop failed" f_error
            log "${RESP}" f_data
            return 1
        fi
    fi
}

action_update() {
    local INST_JSON="${1}"
    local ASSIGN="${2}"
    local RESP NEW_RHS LHS RHS ID PAYLOAD

    LHS=$(sed -E 's/[[:space:]]*=.*$//' <<<"${ASSIGN}" | sed -E 's/^[[:space:]]+|[[:space:]]+$//g')
    RHS=$(sed -E 's/^.*=[[:space:]]*//' <<<"${ASSIGN}" | sed -E 's/^[[:space:]]+|[[:space:]]+$//g' | sed -E 's/;$//')

    [[ -z "${LHS}" || -z "${RHS}" ]] && { log "Invalid assign '${ASSIGN}'" f_warning; return 1; }

    if [[ "${LHS}" != config.* ]]; then
        log "LHS must start with 'config.' (got '${LHS}')" f_warning
        return 1
    fi

    local FIELD="${LHS#config.}"
    if [[ -z "${FIELD}" ]]; then
        log "Field after 'config.' is empty in '${ASSIGN}'" f_warning
        return 1
    fi

    local JQ_EXPR=".${FIELD} = \$val"

    ID=$(jq -r '.id // empty' <<<"${INST_JSON}")
    PAYLOAD=$(jq -c '.config // {}' <<<"${INST_JSON}")

    if ! NEW_RHS="$(resolve_default_config "${RHS}")"; then
        log "default_config key '${RHS}' not found in defaults" f_error
        return 1
    fi

    RHS="${NEW_RHS}"

    if printf '%s' "${RHS}" | jq -e . >/dev/null 2>&1; then
        if ! PAYLOAD=$(jq -c --argjson val "${RHS}" "${JQ_EXPR}" <<<"${PAYLOAD}"); then
            log "jq --argjson failed for '${ASSIGN}'" f_error
            return 1
        fi
    else
        if ! PAYLOAD=$(jq -c --arg val "${RHS}" "${JQ_EXPR}" <<<"${PAYLOAD}"); then
            log "jq --arg failed for '${ASSIGN}'" f_error
            return 1
        fi
    fi

    if [[ "${DRY_RUN}" = true ]]; then
        log "Would patch config ID='${ID}' PAYLOAD='${PAYLOAD}'" f_recycle
        return 0
    else
        if RESP="$(rustatio_patch_instance "${ID}" "${PAYLOAD}" 2>&1)"; then
            printf '%s' "${PAYLOAD}"
            return 0
        else
            log "Patch failed" f_error
            log "${RESP}" f_data
            return 1
        fi
    fi
}

action_start() {
    local INST_JSON="${1}"
    local ASSIGN="${2}"
    local RESP NEW_RHS LHS RHS PAYLOAD ID

    LHS=$(sed -E 's/[[:space:]]*=.*$//' <<<"${ASSIGN}" | sed -E 's/^[[:space:]]+|[[:space:]]+$//g')
    RHS=$(sed -E 's/^.*=[[:space:]]*//' <<<"${ASSIGN}" | sed -E 's/^[[:space:]]+|[[:space:]]+$//g' | sed -E 's/;$//')

    [[ -z "${LHS}" || -z "${RHS}" ]] && { log "update: invalid assign '${ASSIGN}'" warning; return 1; }

    local JQ_EXPR=".${LHS} = \$val"

    PAYLOAD=$(jq -c '
        {
            torrent: (.torrent // {}),
            config: (.config // {})
        }
    ' <<<"${INST_JSON}")

    if ! NEW_RHS="$(resolve_default_config "${RHS}")"; then
        log "default_config key '${RHS}' not found in defaults" f_error
        return 1
    fi

    RHS="${NEW_RHS}"

    if printf '%s' "${RHS}" | jq -e . >/dev/null 2>&1; then
        if ! PAYLOAD=$(jq -c --argjson val "${RHS}" "${JQ_EXPR}" <<<"${INST_JSON}"); then
            log "jq --argjson failed for '${ASSIGN}'" f_error
            return 1
        fi
    else
        if ! PAYLOAD=$(jq -c --arg val "${RHS}" "${JQ_EXPR}" <<<"${INST_JSON}"); then
            log "jq --arg failed for '${ASSIGN}'" f_error
            return 1
        fi
    fi

    ID=$(jq -r '.id // empty' <<<"${INST_JSON}")

    if [[ "${DRY_RUN}" = true ]]; then
        log "Would start ID='${ID}' PAYLOAD='${PAYLOAD}'" f_recycle

        printf '%s' "${PAYLOAD}"
        return 0
    else
        if RESP="$(rustatio_start_instance "${ID}" "${PAYLOAD}" 2>&1)"; then
            PAYLOAD=$(jq -c '.data // {}' <<<"${RESP}")
            printf '%s' "${PAYLOAD}"
            return 0
        else
            log "Start failed" f_error
            log "${RESP}" f_data
            return 1
        fi
    fi
}

action_delete() {
    local INST_JSON="${1}"
    local ASSIGN="${2}"
    local FILENAME WAY FILE_OBJ MATCHES BYTES HEX RESP ID

    if [[ "${ASSIGN}" == *"instance"* ]]; then
        ID=$(jq -r '.id // empty' <<<"${INST_JSON}")
        if RESP="$(rustatio_delete_instance "${ID}" 2>&1)"; then
            log "Delete succeeded" f_succes
            return 0
        else
            log "Failed to delete" f_error
            log "${RESP}" f_data
            return 1
        fi
    fi

    if [[ "${ASSIGN}" == *"watchfile"* ]] || [[ "${ASSIGN}" == *"archive"* ]]; then
        BYTES=$(jq -r '.torrent.info_hash | map(tostring) | join(" ")' <<<"${INST_JSON}")
        HEX=$(bytes_to_hex "${BYTES}")

        if [[ -z "${HEX}" ]]; then
            log "info_hash not found for instance" f_warning
            return 1
        fi

        MATCHES=$(jq -r --arg h "${HEX}" '.data[] | select(.info_hash == $h) | {filename: (.filename // ""), path: (.path // "/torrents")} | @base64' <<<"${FILES_JSON}")

        if [[ -z "${MATCHES}" ]]; then
            log "No file found for info_hash=${HEX}" f_warning
            return 0
        fi

        while IFS= read -r M; do
            FILE_OBJ=$(base64 --decode <<<"${M}")
            FILENAME=$(jq -r '.filename' <<<"${FILE_OBJ}")
            WAY=$(jq -r '.path' <<<"${FILE_OBJ}")

            if [[ -n "${ARCHIVE_FOLDER}" ]] && [[ "${ASSIGN}" == *"archive"* ]]; then
                if [[ ! -e "${ARCHIVE_FOLDER}/${FILENAME}" ]]; then
                    if [[ "${DRY_RUN}" = true ]]; then
                        log "Would archive file '${FILENAME}' at '${WAY}'" f_recycle
                        return 0
                    else
                        mkdir -p "${ARCHIVE_FOLDER}"

                        if RESP=$(cp -f -- "${WAY}" "${ARCHIVE_FOLDER}/${FILENAME}" 2>&1); then
                            log "Torrent archived ${ARCHIVE_FOLDER}/${FILENAME}" f_saving
                            return 0
                        else
                            log "Failed to archive ${WAY}" f_error
                            log "${RESP}" f_data
                            return 1
                        fi
                    fi
                fi
            fi

            if [[ "${ASSIGN}" == *"watchfile"* ]]; then
                if [[ "${DRY_RUN}" = true ]]; then
                    log "Would delete file '${FILENAME}' at '${WAY}'" f_recycle
                    return 0
                else
                    if RESP="$(rustatio_delete_file "${FILENAME}" 2>&1)"; then
                        log "Delete succeeded" f_succes
                        return 0
                    else
                        log "Failed to delete" f_error
                        log "${RESP}" f_data
                        return 1
                    fi
                fi
            fi
        done <<<"${MATCHES}"
    fi
}

action_addtags() {
    local INST_JSON="${1}"
    local ASSIGN="${2}"
    local ID
    local RESP TAGS_JSON EXISTING_TAGS_JSON NEW_TAGS_JSON PAYLOAD

    ID="$(jq -r '.id // empty' <<<"${INST_JSON}")"
   
    TAGS_JSON=$(jq -Rn --arg assign "${ASSIGN}" '($assign | split(",") | map(select(length>0)))')

    EXISTING_TAGS_JSON=$(jq -c '.tags // []' <<<"${INST_JSON}")

    NEW_TAGS_JSON=$(jq -n --argjson assign "${TAGS_JSON}" --argjson existing "${EXISTING_TAGS_JSON}" \
        '$assign | map(select(. as $t | $existing | index($t) | not))' | jq -c .)

    if [[ "$(jq 'length' <<<"${NEW_TAGS_JSON}")" -eq 0 ]]; then
        return 0
    fi

    PAYLOAD=$(jq -n --arg id "${ID}" --argjson add_tags "${NEW_TAGS_JSON}" \
        '{ids: [$id], add_tags: $add_tags, remove_tags: []}' | jq -c .)

    if [[ "${DRY_RUN}" = true ]]; then
        log "Would add tags to ID='${ID}' TAGS='${PAYLOAD}'" f_recycle
        return 0
    else
        if RESP="$(rustatio_tags "${PAYLOAD}" 2>&1)"; then
            printf '%s' "${PAYLOAD}"
            return 0
        else
            log "Failed to add tags (${ASSIGN}) for instance ${ID}" f_error
            log "${RESP}" f_data
            return 1
        fi
    fi
}

action_removetags() {
    local INST_JSON="${1}"
    local ASSIGN="${2}"
    local ID
    local RESP TAGS_JSON EXISTING_TAGS_JSON DEL_TAGS_JSON PAYLOAD

    ID="$(jq -r '.id // empty' <<<"${INST_JSON}")"

    TAGS_JSON=$(jq -Rn --arg assign "${ASSIGN}" '($assign | split(",") | map(select(length>0)))')

    EXISTING_TAGS_JSON=$(jq -c '.tags // []' <<<"${INST_JSON}")

    DEL_TAGS_JSON=$(jq -n --argjson want "${TAGS_JSON}" --argjson exist "${EXISTING_TAGS_JSON}" \
        '$want | map(select(. as $t | $exist | index($t)))' | jq -c .)

    if [[ "$(jq 'length' <<<"${DEL_TAGS_JSON}")" -eq 0 ]]; then
        return 0
    fi

    PAYLOAD=$(jq -n --arg id "${ID}" --argjson del_tags "${DEL_TAGS_JSON}" \
        '{ids: [$id], add_tags: [], remove_tags: $del_tags}' | jq -c .)

    if [[ "${DRY_RUN}" = true ]]; then
        log "Would remove tags from ID='${ID}' TAGS='${PAYLOAD}'" f_recycle
        return 0
    else
        if RESP="$(rustatio_tags "${PAYLOAD}" 2>&1)"; then
            printf '%s' "${PAYLOAD}"
            return 0
        else
            log "Failed to remove tags (${ASSIGN}) for instance ${ID}" f_error
            log "${RESP}" f_data
            return 1
        fi
    fi
}

is_action_valid() {
    local ACTION="${1}"
    local STATE="${2}"

    case "${ACTION}" in
        start)
            [[ "${STATE}" == "Stopped" ]] && return 0 || return 1
            ;;
        stop)
            [[ "${STATE}" == "Running" ]] && return 0 || return 1
            ;;
        pause)
            [[ "${STATE}" == "Running" ]] && return 0 || return 1
            ;;
        resume)
            [[ "${STATE}" == "Paused" ]] && return 0 || return 1
            ;;
        update|addtags|removetags|delete)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

run_action_for_instance() {
    local ACTION="${1}"
    local INST_JSON="${2}"
    local ASSIGN="${3}"

    case "${ACTION}" in
        update)
            action_update "${INST_JSON}" "${ASSIGN}"
            ;;
        start)
            action_start "${INST_JSON}" "${ASSIGN}"
            ;;
        stop)
            action_stop "$(jq -r '.id // empty' <<<"${INST_JSON}")"
            ;;
        pause)
            action_pause "$(jq -r '.id // empty' <<<"${INST_JSON}")"
            ;;
        resume)
            action_resume "$(jq -r '.id // empty' <<<"${INST_JSON}")"
            ;;
        delete)
            action_delete "${INST_JSON}" "${ASSIGN}"
            ;;
        addtags)
            action_addtags "${INST_JSON}" "${ASSIGN}"
            ;;
        removetags)
            action_removetags "${INST_JSON}" "${ASSIGN}"
            ;;
        *) log "Unknown action: ${ACTION}" warning
            ;;
    esac
}

process_rules() {
    local JQCOND UPDATED_OUT RET INSTANCES_JSON SAMPLE_INST VAL_RET

    INSTANCES_JSON="$(rustatio_get_instances 2>&1)"
    RET=$?
    if ! is_valid_json "${INSTANCES_JSON}" && (( RET != 0 )); then
        log "INSTANCES_JSON invalid or non-JSON" error
        echo "${INSTANCES_JSON}"
        return 1
    fi

    if (( LOGS_WATCHER != 0 )); then
        check_logs
    fi

    FILES_JSON="$(rustatio_get_files 2>&1)"
    RET=$?
    if ! is_valid_json "${FILES_JSON}" && (( RET != 0 )); then
        log "FILES_JSON invalid or non-JSON" error
        echo "${FILES_JSON}"
        return 1
    fi

    #local RULES_TEXT="$(load_rules_file "${RULES_FILE}")"
    #load_defaults_file "${DEFAULTS_FILE}"

    SAMPLE_INST="$(jq -c '.data[0] // {}' <<<"${INSTANCES_JSON}")"
    if ! is_valid_json "${GLOBAL_DEFAULTS_JSON}" \
       || [[ "${GLOBAL_DEFAULTS_JSON}" == "{}" ]] \
       || ! is_valid_json "${SAMPLE_INST}" \
       || [[ "${SAMPLE_INST}" == "{}" ]]
    then
        return 1
    fi

    while IFS= read -r LINE || [[ -n "${LINE}" ]]; do
        [[ -z "${LINE//[[:space:]]/}" ]] && continue
        [[ "${LINE}" =~ ^[[:space:]]*# ]] && continue

        PARSED="$(parse_rule_line "${LINE}" 2>/dev/null || printf '')"
        if [[ -z "${PARSED}" ]]; then
            log "Invalid rule: ${LINE}" denied
            continue
        fi

        SAMPLE_INST="$(jq -c '.data[0] // {}' <<<"${INSTANCES_JSON}" | base64 -w0)"
        VAL_RET="$(validate_rule_keys "${LINE}" "${SAMPLE_INST}" 2>&1)"
        RET=$?
        if (( RET != 0 )); then
            log "Invalid rule: ${LINE}" denied
            echo "${VAL_RET}"
            continue
        fi

        IFS=$'\x1F' read -r COND ACTION ASSIGN <<<"${PARSED}"

        JQCOND=""
        cond_to_jq "${COND}"
        RET=$?
        if (( RET != 0 )) || [[ -z "${JQCOND//[[:space:]]/}" ]]; then
            if (( RET != 0 )); then
                log "Invalid rule: ${LINE}" denied
                log "${JQCOND}"
            fi

            continue
        fi

        mapfile -t MATCHES < <(jq -c ".data[] | select(${JQCOND})" <<<"${INSTANCES_JSON}")
        if [[ "${#MATCHES[@]}" -eq 0 ]]; then
            continue
        fi

        for INST in "${MATCHES[@]}"; do
            local STATE=$(jq -r '.stats.state // empty' <<<"${INST}")

            if ! is_action_valid "${ACTION}" "${STATE}"; then
                continue
            fi

            if (( TOR_KEEP_LAST == 1 )); then
                if [[ "${ACTION}" == "stop" ]] || [[ "${ACTION}" == "delete" ]]; then
                    local ANNOUNCE="$(jq -r '.torrent.announce // ""' <<<"${INST}" | tr '[:upper:]' '[:lower:]')"
                    local ANNOUNCE_COUNT=$(jq -r --arg a "${ANNOUNCE}" '.data[] | select((.torrent.announce // "") | ascii_downcase == $a) | .id' <<<"${INSTANCES_JSON}" | wc -l)
                    ANNOUNCE_COUNT=$((ANNOUNCE_COUNT + 0))

                    if (( ANNOUNCE_COUNT == 1 )); then
                        continue
                    fi
                fi
            fi

            UPDATED_OUT="$(run_action_for_instance "${ACTION}" "${INST}" "${ASSIGN}" 2>&1)"
            RET=$?

            if [[ -n "${UPDATED_OUT//[[:space:]]/}" ]]; then
                local ID=$(jq -r '.id // empty' <<<"${INST}")
                local NAME=$(jq -r '.torrent.name // empty' <<<"${INST}")

                log "Rule applied: ${LINE}" task
                log "Torrent name : ${NAME}" f_data

                if is_valid_json "${UPDATED_OUT}"; then
                    if [[ "${ACTION}" == "update" ]]; then
                        NEW_INST=$(jq --argjson cfg "${UPDATED_OUT}" '.config = $cfg' <<<"${INST}")

                        log "Patch succeeded (${ASSIGN})" f_succes
                    fi

                    if [[ "${ACTION}" == "stop" ]]; then
                        NEW_INST=$(
                          jq \
                            --argjson cfg "${UPDATED_OUT}" \
                            '.stats = $cfg
                             | .stats.state = "Stopped"' <<< "${INST}"
                        )

                        log "Stop succeeded" f_succes
                    fi

                    if [[ "${ACTION}" == "start" ]]; then
                        NEW_INST=$(
                          jq \
                            --argjson cfg "${UPDATED_OUT}" \
                            '.stats = $cfg
                             | .stats.state = "Running"' <<< "${INST}"
                        )

                        log "Start succeeded" f_succes
                    fi

                    if [[ "${ACTION}" == "pause" ]]; then
                        NEW_INST=$(
                          jq \
                            --argjson cfg "${UPDATED_OUT}" \
                            '.stats = $cfg
                             | .stats.state = "Paused"' <<< "${INST}"
                        )

                        log "Pause succeeded" f_succes
                    fi

                    if [[ "${ACTION}" == "resume" ]]; then
                        NEW_INST=$(
                          jq \
                            --argjson cfg "${UPDATED_OUT}" \
                            '.stats = $cfg
                             | .stats.state = "Running"' <<< "${INST}"
                        )

                        log "Resume succeeded" f_succes
                    fi

                    if [[ "${ACTION}" == "addtags" ]] || [[ "${ACTION}" == "removetags" ]]; then
                        NEW_INST=$(jq -n --argjson inst "${INST}" --argjson p "${UPDATED_OUT}" \
                            '($inst) as $i | $i | .tags = (
                            ((($i.tags // []) + ($p.add_tags // [])) | unique)
                            | map(. as $t | select((($p.remove_tags // []) | index($t)) | not))
                            )')

                        log "Tags $( [[ ${ACTION} == addtags ]] && echo added || echo removed ) (${ASSIGN})" f_succes
                    fi

                    INSTANCES_JSON=$(jq --arg id "${ID}" --argjson new "${NEW_INST}" \
                        '.data |= map(if .id == $id then $new else . end) | .' <<<"${INSTANCES_JSON}")

                    INST=$(jq -c '.data[] | select(.id == "'"${ID}"'")' <<<"${INSTANCES_JSON}")

                    clear_cached_rand
                else
                    if (( RET == 0 )) && [[ "${ACTION}" == "delete" ]]; then
                        if [[ "${ASSIGN}" == *"watchfile"* ]]; then
                            INSTANCES_JSON=$(
                                jq --arg id "${ID}" \
                                   '.data |= map(select(.id != $id))' \
                                   <<< "${INSTANCES_JSON}"
                            )
                            INST=""
                        fi

                        clear_cached_rand
                    fi

                    echo "${UPDATED_OUT}"
                fi

                sleep 1
            fi
        done
    done <<<"${RULES_TEXT}"
}

run_loop() {
    RULES_TEXT=""
    GLOBAL_DEFAULTS_JSON="{}"
    INITIAL_INTERVAL=5
    REFRESH_INTERVAL=$(( REFRESH_INTERVAL + 0 ))
    RAND_CACHE_JSON='{"vals":{},"ts":{}}'
    RAND_TTL=0
    CHECK_LOGS_PID=""
    LOGS_WATCHER=$(( LOGS_WATCHER + 0 ))
    declare -A COND_CACHE=()

    load_rules_file "${RULES_FILE}"
    load_defaults_file "${DEFAULTS_FILE}"

    if is_valid_json "${GLOBAL_DEFAULTS_JSON}" && [[ "${GLOBAL_DEFAULTS_JSON}" != "{}" ]] && (( REFRESH_INTERVAL == 0 )); then
        REFRESH_INTERVAL="$(resolve_default_config "default_config.scrape_interval")"
        REFRESH_INTERVAL=$(( REFRESH_INTERVAL + 0 ))
    fi

    REFRESH_INTERVAL=$(( REFRESH_INTERVAL - INITIAL_INTERVAL ))

    if (( REFRESH_INTERVAL <= 5 )); then
        REFRESH_INTERVAL=0
    fi

    log "REFRESH INTERVAL : $(( REFRESH_INTERVAL + INITIAL_INTERVAL ))s" data

    while true; do
        CHECK_HEALTH="$(curl -s --max-time 3 "${RUSTATIO_API}/health" || true)"

        if [[ "${CHECK_HEALTH}" == "OK" ]]; then
            break
        fi

        log "Rustatio not ready. Retry in $(( REFRESH_INTERVAL + INITIAL_INTERVAL ))s" warning

        sleep $(( REFRESH_INTERVAL + INITIAL_INTERVAL ))
    done

handle_chld() {
    while true; do
        terminated_pid=$(wait -n 2>/dev/null) || break
        if [[ -n "${CHECK_LOGS_PID:-}" ]] && [[ "${terminated_pid}" -eq "${CHECK_LOGS_PID}" ]]; then
            unset CHECK_LOGS_PID
        fi
    done
}

trap handle_chld CHLD

    while true; do
        rotate_logs
        sleep ${INITIAL_INTERVAL}
        if [[ ! "${LOGFILE}" == "/dev/null" ]]; then
            if [[ ! -e "${LOGFILE}" ]]; then
                touch "${LOGFILE}"
                exec >> "${LOGFILE}" 2>&1
                log "Log recreated automatically" start
                load_rules_file "${RULES_FILE}"
                load_defaults_file "${DEFAULTS_FILE}"
                COND_CACHE=()
                if (( REFRESH_INTERVAL <= 5 )); then
                    REFRESH_INTERVAL=0
                else
                    if is_valid_json "${GLOBAL_DEFAULTS_JSON}" && [[ "${GLOBAL_DEFAULTS_JSON}" != "{}" ]]; then
                        REFRESH_INTERVAL="$(resolve_default_config "default_config.scrape_interval")"
                        REFRESH_INTERVAL=$(( REFRESH_INTERVAL + 0 ))
                        REFRESH_INTERVAL=$(( REFRESH_INTERVAL - INITIAL_INTERVAL ))
                        if (( REFRESH_INTERVAL <= 5 )); then
                            REFRESH_INTERVAL=0
                        fi
                    fi
                fi
                log "REFRESH INTERVAL : $(( REFRESH_INTERVAL + INITIAL_INTERVAL ))s" data
            fi
        fi
        process_rules
        sleep ${REFRESH_INTERVAL}
    done
}

if [[ -n "${RUSTATIO_API}" ]]; then
    PIDFILE="/data/${BASE%.*}.pid"
    PIDFILE="/dev/null"
    echo $$ > "${PIDFILE}"
    nohup bash -c "
    trap cleanup EXIT;
    trap cleanup SIGTERM;
    trap cleanup SIGINT;
    trap cleanup SIGHUP;
    RUSTATIO_API='${RUSTATIO_API}';
    REFRESH_INTERVAL='${REFRESH_INTERVAL}';
    ARCHIVE_FOLDER='${ARCHIVE_FOLDER}';
    RULES_FILE='${RULES_FILE}';
    DEFAULTS_FILE='${DEFAULTS_FILE}';
    DRY_RUN='${DRY_RUN}';
    LOGFILE='${LOGFILE}';
    LOGS_WATCHER='${LOGS_WATCHER}';
    WATCHER_MAX_STRIKE='${WATCHER_MAX_STRIKE}';
    WATCHER_STRIKE_TIME='${WATCHER_STRIKE_TIME}';
    WATCHER_PAUSE_TIME='${WATCHER_PAUSE_TIME}';
    TOR_KEEP_LAST='${TOR_KEEP_LAST}'
    $(declare -f);
    run_loop" >> "${LOGFILE}" 2>&1 &
fi

: '
{
    "success": true,
    "data": [
        {
            "id": "_Mf6squnrq",
            "torrent": {
                "info_hash": [
                    123,
                    123,
                    123,
                    123,
                    12,
                    123,
                    12,
                    12,
                    12,
                    123,
                    12,
                    12,
                    123,
                    123,
                    123,
                    123,
                    123,
                    123,
                    12,
                    12
                ],
                "announce": "https://test.com/announce",
                "name": "Test.2025.MULTi.VF2.1080p.WEB.H264-SUPPLY",
                "total_size": 1234567890,
                "piece_length": 1234567,
                "num_pieces": 1234,
                "comment": "Ce torrent a été téléchargé depuis Test. https://Test.com/torrents/12345",
                "created_by": "Edited by UNIT3D",
                "is_single_file": false,
                "file_count": 0
            },
            "config": {
                "upload_rate": 1000,
                "download_rate": 0.0,
                "port": 6881,
                "vpn_port_sync": false,
                "client_type": "transmission",
                "client_version": "4.0.5",
                "initial_uploaded": 0,
                "initial_downloaded": 0,
                "completion_percent": 100.0,
                "num_want": 50,
                "randomize_rates": true,
                "random_range_percent": 50.0,
                "randomize_ratio": false,
                "random_ratio_range_percent": 10.0,
                "stop_at_ratio": null,
                "effective_stop_at_ratio": null,
                "stop_at_uploaded": null,
                "stop_at_downloaded": null,
                "stop_at_seed_time": 2678400,
                "idle_when_no_leechers": true,
                "idle_when_no_seeders": false,
                "scrape_interval": 60,
                "progressive_rates": false,
                "target_upload_rate": 100.0,
                "target_download_rate": 200.0,
                "progressive_duration": 3600,
                "post_stop_action": "idle"
            },
            "stats": {
                "uploaded": 0,
                "downloaded": 0,
                "ratio": 0.0,
                "left": 0,
                "torrent_completion": 100.0,
                "seeders": 136,
                "leechers": 0,
                "state": "Running",
                "is_idling": true,
                "idling_reason": "no_leechers",
                "session_uploaded": 0,
                "session_downloaded": 0,
                "session_ratio": 0.0,
                "elapsed_time": {
                    "secs": 123,
                    "nanos": 123
                },
                "current_upload_rate": 0.0,
                "current_download_rate": 0.0,
                "average_upload_rate": 0.0,
                "average_download_rate": 0.0,
                "upload_progress": 0.0,
                "download_progress": 0.0,
                "ratio_progress": 0.0,
                "seed_time_progress": 0.0,
                "effective_stop_at_ratio": null,
                "eta_ratio": null,
                "eta_uploaded": null,
                "eta_seed_time": {
                    "secs": 123,
                    "nanos": 0
                },
                "eta_download_completion": null,
                "upload_rate_history": [],
                "download_rate_history": [],
                "ratio_history": [],
                "history_timestamps": [],
                "stop_condition_met": false,
                "post_stop_action": "idle",
                "announce_count": 1
            },
            "created_at": 1771234567,
            "source": "watch_folder",
            "tags": [
                "Test",
                "Forced"
            ]
        }
    ]
}
'

: '
{
    "instances": {},
    "default_config": {
        "upload_rate": 5000.0,
        "download_rate": 0.0,
        "port": 6881,
        "vpn_port_sync": false,
        "client_type": "transmission",
        "client_version": null,
        "initial_uploaded": 0,
        "initial_downloaded": 0,
        "completion_percent": 100.0,
        "num_want": 50,
        "randomize_rates": true,
        "random_range_percent": 10.0,
        "randomize_ratio": false,
        "random_ratio_range_percent": 10.0,
        "stop_at_ratio": null,
        "effective_stop_at_ratio": null,
        "stop_at_uploaded": null,
        "stop_at_downloaded": null,
        "stop_at_seed_time": null,
        "idle_when_no_leechers": false,
        "idle_when_no_seeders": false,
        "scrape_interval": 60,
        "progressive_rates": false,
        "target_upload_rate": null,
        "target_download_rate": null,
        "progressive_duration": 3600,
        "post_stop_action": "idle"
    },
    "default_preset": null,
    "watch_settings": {
        "max_depth": 1,
        "auto_start": true
    },
    "custom_presets": [],
    "version": 1
}
'
