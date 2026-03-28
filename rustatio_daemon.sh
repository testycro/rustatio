#!/usr/bin/env bash

set -euo pipefail
BASE=$(basename "${0}")





# Configuration (à adapter)
RUSTATIO_API="http://127.0.0.1:8080/api"    # A adapter en gardan "/api"

REFRESH_INTERVAL=5                          # Temps d'attente en seconds entre chaques traitement des règles de rules_file. Minimum 5s 

ARCHIVE_FOLDER="/data/archived"             # Chemin vers le dossier d'archivage des .torrent

RULES_FILE="/data/rules.txt"                # Chemin vers le fichier des règles. Se charge au démarrage ou lorsque LOGFILE est suprimé

DEFAULTS_FILE="/data/state.json"            # Chemin vers le fichier state.json contenant default_config, etc. Se charge au démarrage ou lorsque LOGFILE est suprimé

DRY_RUN=false                               # Aucuns appel API, test seulement true/false

LOGFILE="/data/${BASE%.*}.log"              # Chemins vers le fichier log ou /dev/null pour désactiver
#LOGFILE="/dev/null"





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

    echo "${log_time} :: ${prefix}${message}"
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
        printf -v H "%02x" "$((10#$N))"
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
            RESPONSE=$(curl --fail -S -s -X ${METHOD} "${RUSTATIO_API}/${WAY}" \
                    -H "Accept: application/json" 2>&1)
            CURL_EXIT=$?
        else
            RESPONSE=$(printf '%s' "${PAYLOAD}" | \
                curl --fail -S -s -X ${METHOD} "${RUSTATIO_API}/${WAY}" \
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

# --- Rustatio convenience wrappers ---
rustatio_get_instances() {
    rustatio_api_request "instances" "" "GET"
}

rustatio_delete_instance() {
    rustatio_api_request "instances/${1}?force=true" "" "DELETE"
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

normalize_default_ref() {
    local S="${1}"
    sed -E 's#(^|[^a-zA-Z0-9_])default_config\.#\1.default_config.#g' <<<"${S}"
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
    local EXPR="${1:-}"
    local SAMPLE_INST="$(printf '%s' "${2}" | base64 -d)"
    local INST_KEYS=()
    local VAL JQERR DEFAULT_KEYS RAW_KEYS PARTS JQ_ARRAY

	if is_valid_json "${GLOBAL_DEFAULTS_JSON}" && [[ "${GLOBAL_DEFAULTS_JSON}" != "{}" ]]; then
		mapfile -t DEFAULT_KEYS < <(grep -oE 'default_config\.([A-Za-z0-9_\.]+)' <<<"${EXPR}" | sed 's/^default_config\.//' | sort -u)
		for K in "${DEFAULT_KEYS[@]}"; do
			read -r -a PARTS <<< "$(sed 's/\./ /g' <<<"${K}")"
			JQ_ARRAY=$(printf '"%s",' "${PARTS[@]}" | sed 's/,$//')
			VAL="$(jq -c "getpath([${JQ_ARRAY}]) // \"__MISSING__\"" <<<"${GLOBAL_DEFAULTS_JSON}" 2>/dev/null || echo null)"
			if [[ "${VAL}" == '"__MISSING__"' ]]; then
				log "default_config key 'default_config.${K}' not found in defaults" f_error
				return 1
			fi
		done
	fi

	if is_valid_json "${SAMPLE_INST}" && [[ "${SAMPLE_INST}" != "{}" ]]; then
		mapfile -t RAW_KEYS < <(grep -oE '([A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)+)' <<<"${EXPR}" | sort -u | grep -v '^default_config\.')

		for K in "${RAW_KEYS[@]}"; do
			INST_KEYS+=("${K}")
		done

		for K in "${INST_KEYS[@]}"; do
			if [[ "${K}" == "stats.is_idling" || "${K}" == "stats.idling_reason" ]]; then
				if ! jq -e 'has("stats") and (.stats | has("is_idling"))' <<<"${SAMPLE_INST}" >/dev/null 2>&1 && [[ "${K}" == "stats.is_idling" ]]; then
					log "Instance key '${K}' not found in Instance" f_error
					return 1
				fi
				if ! jq -e 'has("stats") and (.stats | has("idling_reason"))' <<<"${SAMPLE_INST}" >/dev/null 2>&1 && [[ "${K}" == "stats.idling_reason" ]]; then
					log "Instance key '${K}' not found in Instance" f_error
					return 1
				fi
				continue
			fi

			read -r -a PARTS <<< "$(sed 's/\./ /g' <<<"${K}")"
			JQ_ARRAY=$(printf '"%s",' "${PARTS[@]}" | sed 's/,$//')
			VAL="$(jq -c "getpath([${JQ_ARRAY}]) // \"__MISSING__\"" <<<"${SAMPLE_INST}" 2>/dev/null || echo null)"
			if [[ "${VAL}" == '"__MISSING__"' ]]; then
				log "Instance key '${K}' not found in Instance" f_error
				return 1
			fi
		done
	fi

    return 0
}

cond_to_jq() {
    local EXPR="${1}"

    EXPR="$(echo "${EXPR}" | sed -E 's/\bAND\b/ and /g; s/\bOR\b/ or /g')"

    local RANGE_REGEX='([A-Za-z0-9_.]+):[[:space:]]*([0-9]+(\.[0-9]+)?) *- *([0-9]+(\.[0-9]+)?)'
    while [[ ${EXPR} =~ ${RANGE_REGEX} ]]; do
        local FULL_MATCH="${BASH_REMATCH[0]}"
        local WAY="${BASH_REMATCH[1]}"
        local a="${BASH_REMATCH[2]}"
        local b="${BASH_REMATCH[4]}"

        local rand
        rand="$(awk -v a="$a" -v b="$b" 'BEGIN {
            if (a == b) { printf("%.2f", a); exit }
            if (a > b) { t = a; a = b; b = t }
            srand(systime() + PROCINFO["pid"])
            r = a + rand() * (b - a)
            printf("%.2f", r)
        }')"

        EXPR="${EXPR/"${FULL_MATCH}"/((.${WAY} // 0) | tonumber) > ${rand}}"
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

	mapfile -t KEYS < <(grep -oE 'default_config\.([A-Za-z0-9_\.]+)' <<<"${EXPR}" | sed 's/^default_config\.//' | sort -u)

	for KEY in "${KEYS[@]}"; do
		VAL="$(jq -c --arg k "${KEY}" '.[$k] // "__MISSING__"' <<<"${GLOBAL_DEFAULTS_JSON}" 2>/dev/null || echo null)"
		EXPR="${EXPR//default_config.${KEY}/${VAL}}"
	done

    printf '%s' "${EXPR}"
	return 0
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
            log "Stop failed for instance ${ID}" f_error
            log "${RESP}"
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
            log "Patch failed for instance ${ID}" f_error
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
            log "Start failed for instance ${ID}" f_error
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
            log "Delete succeeded for ID='${ID}'" f_succes
			return 0
        else
            log "Failed to delete for ID='${ID}'" f_error
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
                            log "Failed to archive ${WAY}" f_er
