- Place the files according to the indicated paths.
- Edit "rustatio_daemon.sh" to match your config
- Don't forget to create your rules's file in the right folder, there is a sample
- Add following lines to your docker-compose

```yaml
services:
  rustatio:
    entrypoint: ["/entrypoint.sh"]
    volumes:
      - /your/path/to/entrypoint.sh:/entrypoint.sh:ro
      - /your/path/to/rustatio_daemon.sh:/rustatio_daemon.sh:ro
```
