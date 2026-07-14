#!/usr/bin/env bash

# Met à jour les paquets et installe les dépendances
apt-get update && apt-get install -y bash jq gawk sed tzdata tini

# Exécute le script en arrière-plan
bash /rustatio_daemon.sh &

# Lance le processus principal
exec tini -g -- /app/entrypoint.sh /app/rustatio-server "$@"
#exec /app/entrypoint.sh /app/rustatio-server "$@" > >(tee -a /data/rustatio-server.log) 2> >(tee -a /data/rustatio-server.log >&2)
