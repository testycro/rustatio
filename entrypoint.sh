#!/usr/bin/env bash

# Met à jour les paquets et installe les dépendances
apt-get update && apt-get install -y bash jq gawk sed tzdata tini

# Boucle de supervision du daemon en arrière-plan
(
  while true; do
    echo "[Daemon] Lancement de rustatio_daemon.sh..."
    bash /rustatio_daemon.sh
    EXIT_CODE=$?
    echo "[Daemon] Arrêt inattendu (code $EXIT_CODE). Redémarrage dans 3 secondes..."
    sleep 3
  done
) &

# Lance le processus principal
exec tini -s -g -- /app/entrypoint.sh /app/rustatio-server "$@"
