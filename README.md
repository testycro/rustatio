<h1>This is a shell script that allows you to modify the behavior of Rustatio (Docker) with custom rules.</h1>

- Work with rustatio 2.x.x in the same container
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

<h2>Rules</h2>

# < CONDITIONS > |  <ACTION > | < ASSIGN >
# Any data can be used for conditions exept history (upload_rate_history, download_rate_history, ratio_history, history_timestamps)
# Operator are, : , = , < , > , != , <= , >=
# No multi-value, no spaces in values or use " "
# Use AND / OR to add more conditions
# info_hash must be a tring
# Special test can be done, some.data: 1.1 - 2.4 , test if some.data is greater than a random generated number within the range
# announce have special operator ~ to test if string exist
# Actions are start / stop / pause / resume / delete / update / addtags / removetags
# Update action can update any config value only, 1 value at a time
# You can add or remove many tags at a time
# Delete can accept 3 params comma separated, instance,watchfile,archive
# Delete's watchfile param will delete file AND instance (internal API feature), can't delete file without instance
# It'not perfect but can do all the things i need
# There is a full data sample at the end of the script
