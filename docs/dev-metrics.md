# Development metrics for Prometheus and Grafana

Spec FORGE-015 and FORGE-016, issue #368. `scripts/land.sh` records every landing in the git
folder; `scripts/dev-metrics.sh` turns that, the machine's processes and the data of GitHub and
Forgejo into a Prometheus text file that node_exporter's textfile collector serves. A Grafana
dashboard shows it.

## One-time setup (root)

```sh
sudo install -d -o raba -g node_exporter -m 0755 /var/lib/node_exporter/textfile
# in /etc/conf.d/prometheus-node-exporter:
NODE_EXPORTER_ARGS="--collector.textfile.directory=/var/lib/node_exporter/textfile"
sudo systemctl restart prometheus-node-exporter
```

`ProtectSystem=strict` in the node_exporter unit is fine: it only reads the folder.

## The timer (user)

```sh
systemctl --user link "$PWD/tools/dev-metrics/shale-dev-metrics.service" "$PWD/tools/dev-metrics/shale-dev-metrics.timer"
systemctl --user enable --now shale-dev-metrics.timer
```

The service runs `%h/Documents/Craft Anything/obsidian/app/scripts/dev-metrics.sh` every 30 seconds
(change the path in the unit when the main checkout is elsewhere). Without the folder the script
says so and does nothing. Try it by hand with `scripts/dev-metrics.sh`; `SHALE_METRICS_DIR=/tmp/x`
writes somewhere else. Data from the network (`gh`, `fj`, `git fetch --tags`) is cached in
`~/.cache/shale-dev-metrics/` and refreshed every `SHALE_METRICS_NET_TTL` seconds (default 300); a
failing source keeps its old data and shows as `shale_dev_metrics_source_ok{...} 0`.

## Dashboard

Import `tools/grafana/shale-landing.json` in Grafana (Dashboards, New, Import) and pick the Prometheus
data source that scrapes this machine's node_exporter.

## Metrics

All metrics are in the file `shale.prom`; node_exporter adds `instance` and `job`. Landings are read
from `<git common dir>/land/history.tsv` (FORGE-015). A Forgejo run counts as `failure` for a tag when
any of its tasks failed (or was cancelled), `running` when one is running or waiting and none failed,
`success` when all succeeded or were skipped, `none` when none of the last five pages of tasks has the
tag's commit.

| Metric | Meaning |
|---|---|
| `shale_dev_metrics_last_run_timestamp_seconds` | gauge, unix time of this write |
| `shale_dev_metrics_source_ok{source="github\|forgejo\|git"}` | 1 when the last fetch of that source worked, else 0 |
| `shale_dev_metrics_source_age_seconds{source=...}` | age of the cached data of that source |
| `shale_land_running` | 1 while a landing holds the lock, else 0 |
| `shale_land_queue_waiting` | number of land.sh processes waiting for the lock |
| `shale_land_current_info{branch="…",step="…"}` | 1, only while running |
| `shale_land_current_started_timestamp_seconds` | start of the running landing (absent when idle) |
| `shale_land_current_step_started_timestamp_seconds` | start of its current step (absent when idle) |
| `shale_land_total{result="landed\|failed\|conflict\|nothing\|error"}` | counter |
| `shale_land_duration_seconds_sum{result=…}` | counter, sum of landing durations |
| `shale_land_duration_seconds_count{result=…}` | counter |
| `shale_land_last_end_timestamp_seconds` | end of the last finished landing |
| `shale_land_last_duration_seconds` | its duration |
| `shale_land_last_info{branch="…",result="…",failed_step="…"}` | 1 (failed_step "" when none) |
| `shale_land_last_success_timestamp_seconds` | end of the last landing with result landed |
| `shale_land_step_last_duration_seconds{step="…"}` | last duration of each step name |
| `shale_land_step_duration_seconds_sum{step="…"}` | counter |
| `shale_land_step_duration_seconds_count{step="…"}` | counter |
| `shale_land_failed_step_total{step="…"}` | counter, landings that failed at that step |
| `shale_land_last_load1` | 1-minute load at the end of the last landing |
| `shale_build_processes{kind="rustc\|cargo\|podman\|lualatex"}` | process count |
| `shale_podman_containers_running` | (podman ps -q \| wc -l; 0 when podman is missing) |
| `shale_claude_processes` | Claude Code processes on the machine (pgrep -x claude) |
| `shale_claude_worktrees` | directories under <main checkout>/.claude/worktrees/ |
| `shale_git_worktrees` | entries of `git worktree list` |
| `shale_github_issues{state="open\|closed"}` |  |
| `shale_github_issues_open_by_label{label="…"}` | ("none" for unlabelled) |
| `shale_github_issues_created_24h, shale_github_issues_closed_24h` |  |
| `shale_git_tags{kind="release\|rc"}` | count of v* tags (rc = contains "-") |
| `shale_github_releases` | count of GitHub releases |
| `shale_github_release_published_timestamp_seconds{tag="vX.Y.Z"}` | one per GitHub release |
| `shale_release_attempt{tag="…",kind="release\|rc",forgejo="success\|failure\|running\|none",github_release="yes\|no"}` | value = unix time of the tag (tagger date) |
| `shale_release_attempts{kind=…,forgejo=…}` | count of tags per kind and Forgejo result |
| `shale_forgejo_job_last_duration_seconds{job="build\|env-tests\|coverage\|audit\|unused-deps\|mirror\|…"}` | last finished run of each job |
| `shale_forgejo_job_last_result{job=…,result=…}` | 1 for the last result of each job |
