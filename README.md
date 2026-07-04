<h1>This is a shell script that allows you to modify the behavior of Rustatio (Docker) with custom rules.</h1>

- Work with rustatio 2.x.x in the same container
- Work better with https://github.com/takitsu21/rustatio/releases/tag/v2.6.0 (less bugs)
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

<h1>Rules</h1>

- < CONDITIONS > |  < ACTION > | < ASSIGN >
- Any data can be used for conditions exept history (upload_rate_history, download_rate_history, ratio_history, history_timestamps)
- Operator are, : , = , < , > , != , <= , >=
- No multi-value, no spaces in values or use " "
- Use AND / OR to add more conditions
- Info_hash must be a tring
- Special test can be done, some.data: 1.1 - 2.4 , test if some.data is greater than a random generated number within the range
- Announce have special operator ~ to test if string exist
- Actions are start / stop / pause / resume / delete / update / addtags / removetags
- Update action can update any config value only, 1 value at a time
- You can add or remove many tags at a time
- Delete can accept 3 params comma separated, instance,watchfile,archive
- Delete's watchfile param will delete file AND instance (internal API feature), can't delete file without instance
- It'not perfect but can do all the things i need
- There is a full data sample at the end of the script

Rules sample

```txt
torrent.announce ~ "test1" | addtags | Hidden
torrent.announce ~ "test2" | addtags | 1337x
torrent.announce ~ "test3" | addtags | NameLess

stats.seeders <= 2 AND tags != Protected | addtags | Protected
stats.seeders > 2 AND tags = Protected | removetags | Protected

stats.leechers = 0 AND tags != Idle AND stats.is_idling = false | addtags | Idle
stats.leechers = 0 AND tags = Idle AND stats.is_idling = true | removetags | Idle
stats.leechers > 0 AND tags = Idle | removetags | Idle

stats.ratio: 1.0 - 2.5 AND config.upload_rate > 2 AND tags != SlowRatio AND tags != Protected | addtags | SlowRatio

stats.leechers > 15 AND stats.seeders > 100 AND tags = SlowRatio AND tags != Protected AND tags != Idle | addtags | Forced
stats.leechers < 15 AND tags = Forced AND tags = SlowRatio AND tags != Protected AND tags != Idle | removetags | Forced

tags = Protected AND config.upload_rate > 0 | update | config.upload_rate = 0
tags = Idle AND config.upload_rate > 0 | update | config.upload_rate = 0

tags != SlowRatio AND tags != Protected AND tags != Idle AND config.upload_rate <= 10 | update | config.upload_rate = default_config.upload_rate
tags = Forced AND config.upload_rate != default_config.upload_rate | update | config.upload_rate = default_config.upload_rate
tags = SlowRatio AND tags != Protected AND tags != Idle AND tags != Forced AND config.upload_rate != 10 | update | config.upload_rate = 10

stats.stop_condition_met = true AND stats.is_idling = true AND stats.idling_reason = "stop_condition_met" | addtags | ended
tags = ended AND stats.state != "Stopped" | stop |

stats.state: Stopped AND tags = ended | delete | watchfile,archive
```

Data sample

```txt
id : _Mf6squnrq

torrent.info_hash : "123"
torrent.announce : "https://test.com/announce"
torrent.name : "Test.2025.MULTi.VF2.1080p.WEB.H264-SUPPLY"
torrent.total_size : 1234567890
torrent.piece_length : 1234567
torrent.num_pieces : 1234
torrent.comment : "Ce torrent a été téléchargé depuis Test. https://Test.com/torrents/12345"
torrent.created_by : "Edited by UNIT3D"
torrent.is_single_file : false
torrent.file_count : 0

config.upload_rate : 1000
config.download_rate : 0.0
config.port : 6881
config.vpn_port_sync : false
config.client_type : "transmission"
config.client_version : "4.0.5"
config.initial_uploaded : 0
config.initial_downloaded : 0
config.completion_percent : 100.0
config.num_want : 50
config.randomize_rates : true
config.random_range_percent : 50.0
config.randomize_ratio : false
config.random_ratio_range_percent : 10.0
config.stop_at_ratio : null
config.effective_stop_at_ratio : null
config.stop_at_uploaded : null
config.stop_at_downloaded : null
config.stop_at_seed_time : 2678400
config.idle_when_no_leechers : true
config.idle_when_no_seeders : false
config.scrape_interval : 60
config.progressive_rates : false
config.target_upload_rate : 100.0
config.target_download_rate : 200.0
config.progressive_duration : 3600
config.post_stop_action : "idle"

stats.uploaded : 0
stats.downloaded : 0
stats.ratio: 0.0
stats.left: 0
stats.torrent_completion : 100.0
stats.seeders : 136
stats.leechers : 0
stats.state : "Running"
stats.is_idling : true
stats.idling_reason : "no_leechers"
stats.session_uploaded : 0
stats.session_downloaded : 0
stats.session_ratio : 0.0
stats.current_upload_rate : 0.0
stats.current_download_rate : 0.0
stats.average_upload_rate : 0.0
stats.average_download_rate : 0.0
stats.upload_progress : 0.0
stats.download_progress : 0.0
stats.ratio_progress : 0.0
stats.seed_time_progress : 0.0
stats.effective_stop_at_ratio : null
stats.eta_ratio : null
stats.eta_uploaded : null
stats.eta_download_completion : null
stats.stop_condition_met : false
stats.post_stop_action : "idle"
stats.stats.announce_count : 1

tags : [
    "Test",
    "Forced"
]

default_config.upload_rate : 5000.0
default_config.download_rate : 0.0
default_config.port : 6881
default_config.vpn_port_sync : false
default_config.client_type : "transmission"
default_config.client_version : null
default_config.initial_uploaded : 0
default_config.initial_downloaded : 0
default_config.completion_percent : 100.0
default_config.num_want : 50
default_config.randomize_rates : true
default_config.random_range_percent : 10.0
default_config.randomize_ratio : false
default_config.random_ratio_range_percent : 10.0
default_config.stop_at_ratio : null
default_config.effective_stop_at_ratio : null
default_config.stop_at_uploaded : null
default_config.stop_at_downloaded : null
default_config.stop_at_seed_time : null
default_config.idle_when_no_leechers : false
default_config.idle_when_no_seeders : false
default_config.scrape_interval : 60
default_config.progressive_rates : false
default_config.target_upload_rate : null
default_config.target_download_rate : null
default_config.progressive_duration : 3600
default_config.post_stop_action : "idle"
```
