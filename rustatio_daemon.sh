#!/usr/bin/env bash

set -euo pipefail
BASE=$(basename "${0}")





# Configuration (à adapter)
RUSTATIO_API="http://127.0.0.1:8080/api"    # A adapter en gardan "/api"

REFRESH_INTERVAL=5                          # Temps d'attente en seconds entre chaques traitement des règles de rules_file. Minimum 5s 

ARCHIVE_FOLDER="/data/archived"             # Chemin vers le dossier d'archivage des .torrent

RULES_FILE="/data/rules.txt"                # Chemin vers le fichier des règles. Se charge au démarrage

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
    rm -f "${PIDFILE}"
}

url_encode() {
    local S="${1}"
    printf '%s' "${S}" | jq -s -R -r @uri
}

is_valid_json() {
    local S="${1}"
    printf '%s' "$S" | jq -e . >/dev/null 2>&1
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

rustatio_api_request() (
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
                    -H "Accept: application/json")
            CURL_EXIT=$?
        else
            RESPONSE=$(printf '%s' "${PAYLOAD}" | \
                curl --fail -S -s -X ${METHOD} "${RUSTATIO_API}/${WAY}" \
                    -H "Content-Type: application/json" \
                    -H "Accept: application/json" \
                    --data-binary @-)
            CURL_EXIT=$?
        fi

        if [ ${CURL_EXIT} -eq 0 ]; then
            set -e
            if is_valid_json "${RESPONSE}"; then
                if jq -e -c '.success == true' <<< "${RESPONSE}" >/dev/null 2>&1; then
                    # imprimer la réponse si .data non vide ou .stats non vide ou .config non vide
                    if jq -e -c '(.data and .data != {} and .data != []) or (.stats and .stats != {}) or (.config and .config != {})' <<< "${RESPONSE}" >/dev/null 2>&1; then
                        printf '%s\n' "${RESPONSE}"
                    fi
                    return 0;
                fi
            fi

            log "Error" ff_error
            printf '%s\n' "${RESPONSE}"
            return 1
        fi
        log "Attempt ${ATTEMPT} failed (curl exit ${CURL_EXIT}). Retrying in 2 seconds..." ff_error
        sleep 2
        (( ATTEMPT++ ))
    done

    log "Failed to ${METHOD} ${WAY} after ${MAX_RETRIES} attempts. Last curl exit: ${CURL_EXIT}" ff_error
    set -e
    return 1
)

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

# Load rules file (returns content)
load_rules_file() {
    local F="${1:-${RULES_FILE}}"
    [[ -f "${F}" ]] && sed -e 's/\r$//' "${F}" || printf ''
}

# Trim helper
_trim() { sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' <<<"${1}"; }

# Parse a rule line into condition, action, assignment (null-separated)
parse_rule_line() {
    local LINE="${1}"

    LINE="${LINE%%#*}"
    LINE="$(_trim "${LINE}")"

    [[ -z "${LINE}" ]] && return 1

    IFS='|' read -r COND ACTION ASSIGN <<<"${LINE}"

    printf '%s\x1F%s\x1F%s' "$(_trim "${COND}")" "$(_trim "${ACTION}")" "$(_trim "${ASSIGN}")"
}

# Convert a simple condition expression into a jq expression
cond_to_jq() {
    local EXPR="${1}"

    EXPR="$(echo "${EXPR}" | sed -E 's/\bAND\b/ and /g; s/\bOR\b/ or /g')"

    # regex pour les ranges
    local RANGE_REGEX='([A-Za-z0-9_.]+):[[:space:]]*([0-9]+(\.[0-9]+)?) *- *([0-9]+(\.[0-9]+)?)'

    # traiter chaque occurrence path: a - b
    while [[ ${EXPR} =~ ${RANGE_REGEX} ]]; do
        local FULL_MATCH="${BASH_REMATCH[0]}"
        local WAY="${BASH_REMATCH[1]}"
        local a="${BASH_REMATCH[2]}"
        local b="${BASH_REMATCH[4]}"

        # générer un nombre aléatoire à 2 décimales entre a et b avec awk
        local rand
        rand="$(awk -v a="$a" -v b="$b" 'BEGIN {
            if (a == b) { printf("%.2f", a); exit }
            if (a > b) { t = a; a = b; b = t }
            srand(systime() + PROCINFO["pid"])
            r = a + rand() * (b - a)
            printf("%.2f", r)
        }')"

        # remplacer la première occurrence trouvée
        EXPR="${EXPR/"${FULL_MATCH}"/((.${WAY} // 0) | tonumber) > ${rand}}"
    done

    # support "contains" operator: path ~ "value"
    EXPR="$(echo "${EXPR}" | sed -E \
	-e 's#torrent\.announce[[:space:]]*~[[:space:]]*\"([^\"]+)\"#((.torrent.announce // \"\") | tostring | ascii_downcase) | contains(\"\1\")#g' \
	-e 's#torrent\.announce[[:space:]]*~[[:space:]]*([A-Za-z0-9_@./:-]+)#((.torrent.announce // \"\") | tostring | ascii_downcase) | contains(\"\1\")#g' \
	-e 's#torrent\.name[[:space:]]*~[[:space:]]*\"([^\"]+)\"#((torrent.name // \"\") | tostring | ascii_downcase) | contains(\"\1\")#g' \
	-e 's#torrent\.name[[:space:]]*~[[:space:]]*([A-Za-z0-9_@./:-]+)#((torrent.name // \"\") | tostring | ascii_downcase) | contains(\"\1\")#g' \
	-e 's#torrent\.comment[[:space:]]*~[[:space:]]*\"([^\"]+)\"#((torrent.comment // \"\") | tostring | ascii_downcase) | contains(\"\1\")#g' \
	-e 's#torrent\.comment[[:space:]]*~[[:space:]]*([A-Za-z0-9_@./:-]+)#((torrent.comment // \"\") | tostring | ascii_downcase) | contains(\"\1\")#g' \
	-e 's#torrent\.created_by[[:space:]]*~[[:space:]]*\"([^\"]+)\"#((torrent.created_by // \"\") | tostring | ascii_downcase) | contains(\"\1\")#g' \
	-e 's#torrent\.created_by[[:space:]]*~[[:space:]]*([A-Za-z0-9_@./:-]+)#((torrent.created_by // \"\") | tostring | ascii_downcase) | contains(\"\1\")#g')"

    # numeric comparisons
    EXPR="$(echo "${EXPR}" | sed -E \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*!=[[:space:]]*([0-9]+(\.[0-9]+)?)#((.\1 // 0) | tonumber) != \2#g' \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*>=[[:space:]]*([0-9]+(\.[0-9]+)?)#((.\1 // 0) | tonumber) >= \2#g' \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*<=[[:space:]]*([0-9]+(\.[0-9]+)?)#((.\1 // 0) | tonumber) <= \2#g' \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*<[[:space:]]*([0-9]+(\.[0-9]+)?)#((.\1 // 0) | tonumber) < \2#g' \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*>[[:space:]]*([0-9]+(\.[0-9]+)?)#((.\1 // 0) | tonumber) > \2#g')"

    # tags: value  →  (.tags // []) | index("value") != null
    # tags = value  →  (.tags // []) | index("value") != null
    EXPR="$(echo "${EXPR}" | sed -E \
    -e 's#tags[[:space:]]*!=[[:space:]]*\"?([A-Za-z0-9_@./:-]+)\"?#((.tags // []) | index("\1") == null)#g' \
    -e 's#tags:[[:space:]]*\"([^\"]+)\"#((.tags // []) | index("\1") != null)#g' \
    -e 's#tags:[[:space:]]*([A-Za-z0-9_@./:-]+)#((.tags // []) | index("\1") != null)#g' \
    -e 's#tags[[:space:]]*=[[:space:]]*\"([^\"]+)\"#((.tags // []) | index("\1") != null)#g' \
    -e 's#tags[[:space:]]*=[[:space:]]*([A-Za-z0-9_@./:-]+)#((.tags // []) | index("\1") != null)#g')"

    # torrent.info_hash: HEX  → comparer le hash hexadécimal
    EXPR="$(echo "${EXPR}" | sed -E \
	-e 's#torrent\.info_hash:[[:space:]]*\"?([A-Fa-f0-9]+)\"?#((.torrent.info_hash // []) | map(printf("%02x"; .)) | join("") == "\1")#g' \
	-e 's#torrent\.info_hash[[:space:]]*\"?([A-Fa-f0-9]+)\"?#((.torrent.info_hash // []) | map(printf("%02x"; .)) | join("") == "\1")#g' \
	-e 's#torrent\.info_hash[[:space:]]*=[[:space:]]*\"?([A-Fa-f0-9]+)\"?#((.torrent.info_hash // []) | map(printf("%02x"; .)) | join("") == "\1")#g' \
	-e 's#torrent\.info_hash[[:space:]]*=[[:space:]]*?([A-Fa-f0-9]+)?#((.torrent.info_hash // []) | map(printf("%02x"; .)) | join("") == "\1")#g')"

    # boolean/null literal equality: key = true|false|null
    EXPR="$(echo "${EXPR}" | sed -E \
	-e 's#([a-zA-Z0-9_.]+):[[:space:]]*(true|false|null)#((.\1 // null) == \2)#g' \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*=[[:space:]]*(true|false|null)#((.\1 // null) == \2)#g')"

    # string equality: path: Value or path = "Value"
    EXPR="$(echo "${EXPR}" | sed -E \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*!=[[:space:]]*\"([^\"]+)\"#((.\1 // \"\") | tostring) != \"\2\"#g' \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*!=[[:space:]]*([A-Za-z0-9_@./:-]+)#((.\1 // \"\") | tostring) != \"\2\"#g' \
    -e 's#([a-zA-Z0-9_.]+):[[:space:]]*\"([^\"]+)\"#((.\1 // \"\") | tostring) == \"\2\"#g' \
    -e 's#([a-zA-Z0-9_.]+):[[:space:]]*([A-Za-z0-9_@./:-]+)#((.\1 // \"\") | tostring) == \"\2\"#g' \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*=[[:space:]]*\"([^\"]+)\"#((.\1 // \"\") | tostring) == \"\2\"#g' \
    -e 's#([a-zA-Z0-9_.]+)[[:space:]]*=[[:space:]]*([A-Za-z0-9_@./:-]+)#((.\1 // \"\") | tostring) == \"\2\"#g')"

    printf '%s' "${EXPR}"
}

# -------------------------
# Actions implementations
# -------------------------

# action: stop
action_stop() {
    local ID="${1}"
    local RESP

    if [[ "${DRY_RUN}" = true ]]; then
        log "Would stop ID='${ID}'" f_recycle
    else
        if RESP="$(rustatio_stop_instance "${ID}" 2>&1)"; then
            # retourner le JSON modifié pour mise à jour d'INSTANCES_JSON
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

# action: update (patch config.upload_rate)
action_update() {
    local INST_JSON="${1}"
    local ASSIGN="${2}"   # ex: config.upload_rate = 1.0
    local RESP

    # extraire gauche et droite, trim espaces
    local LHS=$(sed -E 's/[[:space:]]*=.*$//' <<<"${ASSIGN}" | sed -E 's/^[[:space:]]+|[[:space:]]+$//g')
    local RHS=$(sed -E 's/^.*=[[:space:]]*//' <<<"${ASSIGN}" | sed -E 's/^[[:space:]]+|[[:space:]]+$//g' | sed -E 's/;$//')

    [[ -z "${LHS}" || -z "${RHS}" ]] && { log "Invalid assign '${ASSIGN}'" f_warning; return 1; }

    # vérifier que LHS commence par config.
    if [[ "${LHS}" != config.* ]]; then
        log "LHS must start with 'config.' (got '${LHS}')" f_warning
        return 1
    fi

    # enlever le préfixe config. pour la suite
    local FIELD="${LHS#config.}"
    # protéger contre LHS = "config." seul
    if [[ -z "${FIELD}" ]]; then
        log "Field after 'config.' is empty in '${ASSIGN}'" f_warning
        return 1
    fi

    # construire l'expression jq dynamique
    local JQ_EXPR=".${FIELD} = \$val"

    local ID=$(jq -r '.id // empty' <<<"${INST_JSON}")
    local PAYLOAD=$(jq -c '.config // {}' <<<"${INST_JSON}")

    # déterminer si RHS est du JSON valide
    if printf '%s' "${RHS}" | jq -e . >/dev/null 2>&1; then
        # RHS est du JSON (nombre, bool, objet, array, string JSON)
        if ! PAYLOAD=$(jq -c --argjson val "${RHS}" "${JQ_EXPR}" <<<"${PAYLOAD}"); then
            log "jq --argjson failed for '${ASSIGN}'" f_error
            return 1
        fi
    else
        # RHS n'est pas du JSON valide -> traiter comme chaîne
        if ! PAYLOAD=$(jq -c --arg val "${RHS}" "${JQ_EXPR}" <<<"${PAYLOAD}"); then
            log "jq --arg failed for '${ASSIGN}'" f_error
            return 1
        fi
    fi

    if [[ "${DRY_RUN}" = true ]]; then
        log "Would patch config ID='${ID}' PAYLOAD='${PAYLOAD}'" f_recycle
    else
        if RESP="$(rustatio_patch_instance "${ID}" "${PAYLOAD}" 2>&1)"; then
            # retourner le JSON modifié pour mise à jour d'INSTANCES_JSON
            printf '%s' "${PAYLOAD}"
            return 0
        else
            log "Patch failed for instance ${ID}" f_error
            log "${RESP}" data
            return 1
    fi
    fi
}

# action: start (start instance with modified config.upload_rate)
action_start() {
    local INST_JSON="${1}"
    local ASSIGN="${2}"   # ex: config.upload_rate = 1.0
    local RESP

    # extraire gauche et droite, trim espaces
    local LHS=$(sed -E 's/[[:space:]]*=.*$//' <<<"${ASSIGN}" | sed -E 's/^[[:space:]]+|[[:space:]]+$//g')
    local RHS=$(sed -E 's/^.*=[[:space:]]*//' <<<"${ASSIGN}" | sed -E 's/^[[:space:]]+|[[:space:]]+$//g' | sed -E 's/;$//')

    [[ -z "${LHS}" || -z "${RHS}" ]] && { log "update: invalid assign '${ASSIGN}'" warning; return 1; }

    # construire l'expression jq dynamique
    local JQ_EXPR=".$LHS = \$val"

    local PAYLOAD=$(jq -c '
        {
            torrent: (.torrent // {}),
            config: (.config // {})
        }
    ' <<<"${INST_JSON}")

    # déterminer si RHS est du JSON valide
    if printf '%s' "${RHS}" | jq -e . >/dev/null 2>&1; then
        # RHS est du JSON (nombre, bool, objet, array, string JSON)
        if ! PAYLOAD=$(jq -c --argjson val "${RHS}" "${JQ_EXPR}" <<<"${INST_JSON}"); then
            log "jq --argjson failed for '${ASSIGN}'" f_error
            return 1
        fi
    else
        # RHS n'est pas du JSON valide -> traiter comme chaîne
        if ! PAYLOAD=$(jq -c --arg val "${RHS}" "${JQ_EXPR}" <<<"${INST_JSON}"); then
            log "jq --arg failed for '${ASSIGN}'" f_error
            return 1
        fi
    fi

    local ID=$(jq -r '.id // empty' <<<"${INST_JSON}")

    if [[ "${DRY_RUN}" = true ]]; then
        log "Would start ID='${ID}' PAYLOAD='${PAYLOAD}'" f_recycle

        # en dry-run on retourne quand même le payload pour que la boucle puisse l'utiliser
        printf '%s' "${PAYLOAD}"
        return 0
    else
        if RESP="$(rustatio_start_instance "${ID}" "${PAYLOAD}" 2>&1)"; then
            # retourner le JSON modifié pour mise à jour d'INSTANCES_JSON
            PAYLOAD=$(jq -c '.data // {}' <<<"${RESP}")
            printf '%s' "${PAYLOAD}"
            return 0
        else
            log "Start failed for instance ${ID}" f_error
            log "${RESP}" data
            return 1
        fi
    fi
}

# action: delete (delete watch file by filename or by info_hash)
# Requires FILES_JSON_GLOBAL to be set
action_delete() {
    local INST_JSON="${1}"
    local ASSIGN="${2}"
    local FILENAME WAY FILE_OBJ MATCHES BYTES HEX RESP

    if [[ "${ASSIGN}" == *"watchfile"* ]] && [[ "${ASSIGN}" == *"fileonly"* ]] ; then
        log "You can't set watchfile and fileonly at same time" f_warning
        return 1
    fi

    if [[ "${ASSIGN}" == *"instance"* ]]; then
        local ID=$(jq -r '.id // empty' <<<"${INST_JSON}")
        if RESP="$(rustatio_delete_instance "${ID}" 2>&1)"; then
            log "Delete succeeded for ID='${ID}'" f_succes
        else
            log "Failed to delete for ID='${ID}'" f_error
            log "${RESP}" data
        fi
    fi

    if [[ "${ASSIGN}" == *"watchfile"* ]] || [[ "${ASSIGN}" == *"fileonly"* ]] || [[ "${ASSIGN}" == *"archive"* ]]; then
        # search by info_hash
        BYTES=$(jq -r '.torrent.info_hash | map(tostring) | join(" ")' <<<"${INST_JSON}")
        HEX=$(bytes_to_hex "${BYTES}")

        if [[ -z "${HEX}" ]]; then
            log "info_hash not found for instance" f_warning
            return 1
        fi

        MATCHES=$(jq -r --arg h "${HEX}" '.data[] | select(.info_hash == $h) | {filename: (.filename // ""), path: (.path // "/torrents")} | @base64' <<<"${FILES_JSON_GLOBAL}")

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
                    else
                        mkdir -p "${ARCHIVE_FOLDER}"

                        if RESP=$(cp -f -- "${WAY}" "${ARCHIVE_FOLDER}/${FILENAME}" 2>&1); then
                            log "Torrent archived ${ARCHIVE_FOLDER}/${FILENAME}" f_succes
                        else
                            log "Failed to archive ${WAY}" f_error
                            log "${RESP}" data
                        fi
                    fi
                fi
            fi

            if [[ "${ASSIGN}" == *"watchfile"* ]]; then
                if [[ "${DRY_RUN}" = true ]]; then
                    log "Would delete file '${FILENAME}' at '${WAY}'" f_recycle
                else
                    if RESP="$(rustatio_delete_file "${FILENAME}" 2>&1)"; then
                        log "Delete succeeded for watchfile='${FILENAME}'" f_succes
                    else
                        log "Failed to delete for watchfile='${FILENAME}'" f_error
                        log "${RESP}" data
                    fi
                fi
            fi

#           if [[ "${ASSIGN}" == *"fileonly"* ]]; then
#               if [[ "${DRY_RUN}" = true ]]; then
#                   log "Would delete file '${FILENAME}' at '${WAY}'" f_recycle
#               else
#                   if RESP=$(rm -rf -- "${WAY}" 2>&1); then
#                       log "Delete succeeded for filename='${FILENAME}'" f_succes
#                   else
#                       log "Failed to delete for filename='${FILENAME}'" f_error
#                       log "${RESP}" data
#                   fi
#               fi
#           fi
        done <<<"${MATCHES}"
    fi
}

action_addtags() {
    local INST_JSON="${1}"
    local ASSIGN="${2}"
    local ID
    local RESP TAGS_JSON EXISTING_TAGS_JSON NEW_TAGS_JSON PAYLOAD

    ID="$(jq -r '.id // empty' <<<"${INST_JSON}")"
    IFS=',' read -r -a TAGS_ARRAY <<< "${ASSIGN}"

    # Construire un JSON array pour assign (comme avant)
    TAGS_JSON=$(printf '%s\n' "${TAGS_ARRAY[@]}" | jq -R . | jq -s -c .)

    # Récupérer les tags existants depuis INST_JSON (attend un array de strings)
    EXISTING_TAGS_JSON=$(jq -c '.tags // []' <<<"${INST_JSON}")

    # Calculer les tags qui ne sont pas déjà présents
    NEW_TAGS_JSON=$(jq -n --argjson assign "${TAGS_JSON}" --argjson existing "${EXISTING_TAGS_JSON}" \
        '$assign | map(select(. as $t | $existing | index($t) | not))' | jq -c .)

    # Si aucun tag nouveau, ne rien faire
    if [[ "$(jq 'length' <<<"${NEW_TAGS_JSON}")" -eq 0 ]]; then
        # log "No new tags to add for ID='${ID}'" warning
        return 0
    fi

    # Construire le payload avec seulement les nouveaux tags
    PAYLOAD=$(jq -n --arg id "${ID}" --argjson add_tags "${NEW_TAGS_JSON}" \
        '{ids: [$id], add_tags: $add_tags, remove_tags: []}' | jq -c .)

    if [[ "${DRY_RUN}" = true ]]; then
        log "Would add tags to ID='${ID}' TAGS='${PAYLOAD}'" f_recycle
    else
        if RESP="$(rustatio_tags "${PAYLOAD}" 2>&1)"; then
            # retourner le JSON pour mise à jour d'INSTANCES_JSON
            printf '%s' "${PAYLOAD}"
            return 0
        else
            log "Failed to add tags for instance ${ID}" f_error
            log "${RESP}" data
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
    IFS=',' read -r -a TAGS_ARRAY <<< "${ASSIGN}"

    # Construire un JSON array pour les tags demandés
    TAGS_JSON=$(printf '%s\n' "${TAGS_ARRAY[@]}" | jq -R . | jq -s -c .)

    # Récupérer les tags existants depuis INST_JSON (attend un array de strings)
    EXISTING_TAGS_JSON=$(jq -c '.tags // []' <<<"${INST_JSON}")

    # Calculer les tags à supprimer qui existent réellement (intersection)
    DEL_TAGS_JSON=$(jq -n --argjson want "${TAGS_JSON}" --argjson exist "${EXISTING_TAGS_JSON}" \
        '$want | map(select(. as $t | $exist | index($t)))' | jq -c .)

    # Si aucun tag à supprimer, ne rien faire
    if [[ "$(jq 'length' <<<"${DEL_TAGS_JSON}")" -eq 0 ]]; then
        # log "No existing tags to remove for ID='${ID}'" task
        return 0
    fi

    # Construire le payload avec seulement les tags à supprimer
    PAYLOAD=$(jq -n --arg id "${ID}" --argjson del_tags "${DEL_TAGS_JSON}" \
        '{ids: [$id], add_tags: [], remove_tags: $del_tags}' | jq -c .)

    if [[ "${DRY_RUN}" = true ]]; then
        log "Would remove tags from ID='${ID}' TAGS='${PAYLOAD}'" f_recycle
    else
        if RESP="$(rustatio_tags "${PAYLOAD}" 2>&1)"; then
            # retourner le JSON pour mise à jour d'INSTANCES_JSON
            printf '%s' "${PAYLOAD}"
            return 0
        else
            log "Failed to remove tags for instance ${ID}" f_error
            log "${RESP}" data
            return 1
        fi
    fi
}

# Dispatcher action -> implementation
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

# -------------------------
# Main processing: process_rules refactorisé pour règles
# -------------------------
process_rules() {
    local INSTANCES_JSON="$(rustatio_get_instances)" || { log "${INSTANCES_JSON}"; }

    if ! is_valid_json "${INSTANCES_JSON}"; then
        log "INSTANCES_JSON invalid or non-JSON" error
        return 1
    fi

    local FILES_JSON="$(rustatio_get_files)" || { log "${FILES_JSON}"; }
    if ! is_valid_json "${FILES_JSON}"; then
        log "FILES_JSON invalid or non-JSON" error
        return 1
    fi

    # rendre disponible globalement pour action_delete
    FILES_JSON_GLOBAL="${FILES_JSON}"

    # charger règles
    #local RULES_TEXT="$(load_rules_file "${RULES_FILE}")"

    # itérer sur chaque ligne de règle
    while IFS= read -r LINE || [[ -n "${LINE}" ]]; do
        # ignorer vides/commentaires
        [[ -z "${LINE//[[:space:]]/}" ]] && continue
        [[ "${LINE}" =~ ^[[:space:]]*# ]] && continue

        # call parse_rule_line and read fields using unit separator
        PARSED="$(parse_rule_line "${LINE}" 2>/dev/null || printf '')"
        if [[ -z "${PARSED}" ]]; then
            log "Invalid rule: ${LINE}" warning
            continue
        fi
        IFS=$'\x1F' read -r COND ACTION ASSIGN <<<"${PARSED}"

        JQCOND="$(cond_to_jq "${COND}")"
        if [[ -z "${JQCOND}" ]]; then
            log "Unable to convert condition: ${COND}" error
            continue
        fi

        # récupérer instances correspondantes
        mapfile -t MATCHES < <(jq -c ".data[] | select(${JQCOND})" <<<"${INSTANCES_JSON}")
        if [[ "${#MATCHES[@]}" -eq 0 ]]; then
            # log "Aucune instance ne correspond à la règle: ${LINE}" data
            continue
        fi

        for INST in "${MATCHES[@]}"; do
            # appeler l'action et capturer la sortie JSON (si présente)
            local UPDATED_OUT="$(run_action_for_instance "${ACTION}" "${INST}" "${ASSIGN}" 2>/dev/null || printf '')"
            local ID=$(jq -r '.id // empty' <<<"${INST}")
            local STATE=$(jq -r '.stats.state // empty' <<<"${INST}")

            # si rien de retourné, continuer
            if [[ -z "${UPDATED_OUT//[[:space:]]/}" ]]; then
                continue
            fi

            if [[ "${ACTION}" == "update" ]]; then
                if is_valid_json "${UPDATED_OUT}" && is_valid_json "${INST}"; then
                    log "ID: ${ID} => Rule applied: ${LINE}" task

                    NEW_INST=$(jq --argjson cfg "${UPDATED_OUT}" '.config = $cfg' <<<"${INST}")
                    INSTANCES_JSON=$(jq --arg id "${ID}" --argjson new "${NEW_INST}" \
                        '.data |= map(if .id == $id then $new else . end) | .' <<<"${INSTANCES_JSON}")
                    INST=$(jq -c '.data[] | select(.id == "'"${ID}"'")' <<<"${INSTANCES_JSON}")

                    log "Patch succeeded for instance ${ID}" f_succes
                fi
            fi

            if [[ "${ACTION}" == "stop" ]] && [[ ! "${STATE}" == "Stopped" ]]; then
                if is_valid_json "${UPDATED_OUT}" && is_valid_json "${INST}"; then
                    log "ID: ${ID} => Rule applied: ${LINE}" task

                    NEW_INST=$(
                      jq \
                        --argjson cfg "${UPDATED_OUT}" \
                        '.stats = $cfg
                         | if (.stats.state // "") != "Stopped" then
                             .stats.state = "Stopped"
                           else
                             .
                           end' <<< "$INST"
                    )
                    INSTANCES_JSON=$(jq --arg id "${ID}" --argjson new "${NEW_INST}" \
                        '.data |= map(if .id == $id then $new else . end) | .' <<<"${INSTANCES_JSON}")
                    INST=$(jq -c '.data[] | select(.id == "'"${ID}"'")' <<<"${INSTANCES_JSON}")

                    log "Stop succeeded for instance ${ID}" f_succes
                fi
            fi

            if [[ "${ACTION}" == "start" ]] && [[ ! "${STATE}" == "Running" ]]; then
                if is_valid_json "${UPDATED_OUT}" && is_valid_json "${INST}"; then
                    log "ID: ${ID} => Rule applied: ${LINE}" task

                    NEW_INST=$(
                      jq \
                        --argjson cfg "${UPDATED_OUT}" \
                        '.stats = $cfg
                         | if (.stats.state // "") != "Running" then
                             .stats.state = "Running"
                           else
                             .
                           end' <<< "$INST"
                    )
                    INSTANCES_JSON=$(jq --arg id "${ID}" --argjson new "${NEW_INST}" \
                        '.data |= map(if .id == $id then $new else . end) | .' <<<"${INSTANCES_JSON}")
                    INST=$(jq -c '.data[] | select(.id == "'"${ID}"'")' <<<"${INSTANCES_JSON}")

                    log "Start succeeded for instance ${ID}" f_succes
                fi
            fi

            if [[ "${ACTION}" == "addtags" ]] || [[ "${ACTION}" == "removetags" ]]; then
                if is_valid_json "${UPDATED_OUT}" && is_valid_json "${INST}"; then
                    log "ID: ${ID} => Rule applied: ${LINE}" task

                    # construire le nouvel objet instance en appliquant add/remove
                    NEW_INST=$(jq -n --argjson inst "${INST}" --argjson p "${UPDATED_OUT}" \
                        '($inst) as $i | $i | .tags = (
                        ((($i.tags // []) + ($p.add_tags // [])) | unique)
                        | map(select(($p.remove_tags // []) | index(.) | not))
                        )')

                    # remplacer l'instance dans INSTANCES_JSON
                    INSTANCES_JSON=$(jq --arg id "${ID}" --argjson new "${NEW_INST}" \
                        '.data |= map(if .id == $id then $new else . end) | .' <<<"${INSTANCES_JSON}")

                    # recharger INST pour la suite
                    INST=$(jq -c '.data[] | select(.id == "'"${ID}"'")' <<<"${INSTANCES_JSON}")

                    log "Tags applied for instance for ID='${ID}'" f_succes
                fi
            fi

            if [[ "${ACTION}" == "delete" ]]; then
                if is_valid_json "${INST}" && is_valid_json "${INSTANCES_JSON}"; then
                    log "ID: ${ID} => Rule applied: ${LINE}" task

                    INSTANCES_JSON=$(
                        jq --arg id "${ID}" \
                           '.data |= map(select(.id != $id))' \
                           <<< "${INSTANCES_JSON}"
                    )
                    INST=""
                fi
            fi

            if ! is_valid_json "${UPDATED_OUT}"; then
                echo "${UPDATED_OUT}"
            fi

            sleep 1
        done
    done <<<"${RULES_TEXT}"

    # log "Traitement terminé" finish
}

# -------------------------
# Loop runner (5 minutes cycle)
# -------------------------
run_loop() {
    # charger règles
    log "Loading rules" start
    RULES_TEXT="$(load_rules_file "${RULES_FILE}")"
    INITIAL_INTERVAL=5
    REFRESH_INTERVAL=$(( REFRESH_INTERVAL - INITIAL_INTERVAL ))

    if [[ ${REFRESH_INTERVAL} -lt 5 ]]; then
        REFRESH_INTERVAL=0
    fi

    while true; do
        sleep ${INITIAL_INTERVAL}
        # Recréer le log si supprimé
        if [[ ! "${LOGFILE}" == "/dev/null" ]]; then
            if [[ ! -e "${LOGFILE}" ]]; then
                touch "${LOGFILE}"
                exec >> "${LOGFILE}" 2>&1
                log "Log recreated automatically" start
                log "Reloading rules" start
                RULES_TEXT="$(load_rules_file "${RULES_FILE}")"
            fi
        fi
        process_rules
        sleep ${REFRESH_INTERVAL}
    done
}

# Start background runner if API configured
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
    DRY_RUN='${DRY_RUN}';
    LOGFILE='${LOGFILE}';
    $(declare -f);
    run_loop" >> "${LOGFILE}" 2>&1 &
fi

: '
{
    "success": true,
    "data": [
        {
            "id": "id-LGQsBtj",
            "torrent": {
                "info_hash": [
                    219,
                    212,
                    112,
                    78,
                    17,
                    229,
                    46,
                    24,
                    10,
                    41,
                    187,
                    13,
                    188,
                    246,
                    57,
                    27,
                    152,
                    24,
                    40,
                    145
                ],
                "announce": "https://sample.com/announce",
                "name": "Sample.2025.MULTi.TRUEFRENCH.1080p.WEB-DL.H264-Slay3R.mkv",
                "total_size": 9869094537,
                "piece_length": 2097152,
                "num_pieces": 4706,
                "comment": "Ce torrent a été téléchargé depuis Sample. https://sample.com/torrents/12345",
                "created_by": "ruTorrent (PHP Class - Adrien Gibrat). Edited by UNIT3D",
                "is_single_file": true,
                "file_count": 0
            },
            "config": {
                "upload_rate": 700.0,
                "download_rate": 0.0,
                "port": 59859,
                "client_type": "transmission",
                "client_version": "4.0.5",
                "initial_uploaded": 0,
                "initial_downloaded": 0,
                "completion_percent": 100.0,
                "num_want": 50,
                "randomize_rates": true,
                "random_range_percent": 50.0,
                "stop_at_ratio": null,
                "stop_at_uploaded": null,
                "stop_at_downloaded": null,
                "stop_at_seed_time": 2678400,
                "idle_when_no_leechers": true,
                "idle_when_no_seeders": false,
                "scrape_interval": 60,
                "progressive_rates": false,
                "target_upload_rate": 100.0,
                "target_download_rate": 200.0,
                "progressive_duration": 3600
            },
            "stats": {
                "uploaded": 0,
                "downloaded": 0,
                "ratio": 0.0,
                "left": 0,
                "torrent_completion": 100.0,
                "seeders": 16,
                "leechers": 0,
                "state": "Running",
                "is_idling": true,
                "idling_reason": "no_leechers",
                "session_uploaded": 0,
                "session_downloaded": 0,
                "session_ratio": 0.0,
                "elapsed_time": {
                    "secs": 333755,
                    "nanos": 498129348
                },
                "current_upload_rate": 0.0,
                "current_download_rate": 0.0,
                "average_upload_rate": 0.0,
                "average_download_rate": 0.0,
                "upload_progress": 0.0,
                "download_progress": 0.0,
                "ratio_progress": 0.0,
                "seed_time_progress": 12.460984169653525,
                "eta_ratio": null,
                "eta_uploaded": null,
                "eta_seed_time": {
                    "secs": 2344645,
                    "nanos": 0
                },
                "eta_download_completion": null,
                "announce_count": 121
            },
            "created_at": 1771680826,
            "source": "watch_folder",
            "tags": []
        }
    ]
}

'
