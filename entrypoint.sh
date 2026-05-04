#!/usr/bin/env bash

# Met à jour les paquets et installe les dépendances
apt-get update && apt-get install -y bash jq gawk sed tzdata

# Exécute le script en arrière-plan
bash /rustatio_daemon.sh &

# Lance le processus principal
exec /app/entrypoint.sh /app/rustatio-server "$@"
