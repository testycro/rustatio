#!/usr/bin/env bash

export DEBIAN_FRONTEND=noninteractive
if [ ! -f /etc/localtime ] && [ -z "$TZ" ]; then
    export TZ="Europe/Paris"
fi

# Met à jour les paquets et installe les dépendances
apt-get update && apt-get install -y --no-install-recommends tini python3 python3-requests && rm -rf /var/lib/apt/lists/*

# Boucle de supervision du daemon en arrière-plan
(
  while true; do
    echo "[Daemon] Lancement de rustatio_daemon..."
    tini -s -g -- /rustatio_daemon.py --daemon-mode
    EXIT_CODE=$?
    echo "[Daemon] Arrêt inattendu (code $EXIT_CODE). Redémarrage dans 3 secondes..."
    sleep 3
  done
) &

# Lance le processus principal
exec tini -s -g -- /app/entrypoint.sh /app/rustatio-server "$@"
