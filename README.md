<div align="center">
  <img src="rustatio-desktop/icons/icon.png" alt="Rustatio Logo" width="128" height="128">
</div>

```yaml
services:
  rustatio:
    image: ghcr.io/takitsu21/rustatio:latest
    <b>entrypoint: ["/entrypoint.sh"]</b>
    container_name: rustatio
    environment:
      - PORT=8080
      - RUST_LOG=${RUST_LOG:-trace}
      - PUID=${PUID:-1000}
      - PGID=${PGID:-1000}
      # Optional authentication for your server (Recommended if exposing on internet)
      # - AUTH_TOKEN=${AUTH_TOKEN:-CHANGE_ME}
    volumes:
      - /your/path/to/entrypoint.sh:/entrypoint.sh:ro
      - /your/path/to/rustatio_daemon.sh:/rustatio_daemon.sh:ro
      - rustatio_data:/data
      # Optional: Uncomment to enable watch folder feature
      # - ${TORRENTS_DIR:-./torrents}:/torrents
    restart: unless-stopped
    network_mode: service:gluetun
    depends_on:
      gluetun:
        condition: service_healthy

volumes:
  rustatio_data:
```
