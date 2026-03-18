<div align="center">
  <img src="rustatio-desktop/icons/icon.png" alt="Rustatio Logo" width="128" height="128">
</div>

<b>
- Place the files according to the indicated paths.
- Edit "rustatio_daemon.sh" to match your config
- Don't forget tu create your rules's file in the right folder, there is a sample
- This is only the changes youy have to made
</b>

```yaml
services:
  rustatio:
    entrypoint: ["/entrypoint.sh"]
    volumes:
      - /your/path/to/entrypoint.sh:/entrypoint.sh:ro
      - /your/path/to/rustatio_daemon.sh:/rustatio_daemon.sh:ro
```
