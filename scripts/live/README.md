<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Live measurements against PostgreSQL

The scripts behind the figures the CHANGELOG and `docs/handoff.md` give for
round trips per send, store outages mid-turn, network partitions between
replicas, the rolling upgrade from 0.15 and the soak matrix. None runs in CI:
each starts real replica processes against a real database and takes seconds
to minutes. The CI-sized versions of the outage cases are
`crates/a2a-protocol-server/tests/transient_store_errors`.

## What they need

* PostgreSQL in a Docker container named `pg` (or `$A2A_LIVE_CONTAINER`),
  reachable at `$A2A_LIVE_PG` (default `postgres://a2a:a2a@127.0.0.1:5432`).
  The scripts run `psql`, `pg_isready` and `docker stop/start` in it.

      docker run -d --name pg -p 5432:5432 -e POSTGRES_USER=a2a \
        -e POSTGRES_PASSWORD=a2a -e POSTGRES_DB=a2a postgres:17

* For `partition_live.py` and `soak_matrix.sh`, toxiproxy with its API on
  127.0.0.1:8474 and two links to the database, `pg_a` on 15432 and `pg_b`
  on 15433:

      docker run -d --name toxiproxy --network host ghcr.io/shopify/toxiproxy:2.12.0
      for p in "pg_a 15432" "pg_b 15433"; do set -- $p
        curl -s --noproxy '*' 127.0.0.1:8474/proxies \
          -d "{\"name\":\"$1\",\"listen\":\"127.0.0.1:$2\",\"upstream\":\"127.0.0.1:5432\"}"
      done

* The replica (`replica/`), built from this checkout:

      cargo build --release --manifest-path scripts/live/replica/Cargo.toml \
        --target-dir target/live/build
      R=target/live/build/release/a2a-live-replica

  and, for the upgrade, against v0.15.0:

      git worktree add target/live/v015 v0.15.0
      cargo build --release --manifest-path scripts/live/replica/v015/Cargo.toml \
        --target-dir target/live/build-v015

`replica/src/main.rs` says what each message text makes it do. Logs and
recordings go to `$A2A_LIVE_OUT` (default `target/live`).

## The scripts

| Script | What it does | Usage |
|---|---|---|
| `rt_measure.py` | Records the round trips one send and one read make, through `pgcount.py`; `PG_LAT_MS` adds latency to each reply | `rt_measure.py $R LABEL [new\|continue]` |
| `rt_cat.py` | Splits a recording into statements, prepares, pings and the rest | `rt_cat.py target/live/rt-LABEL.jsonl` |
| `pgcount.py` | The recording proxy: one line per round trip, with its statements | started by `rt_measure.py` |
| `terminate_live.py` | Ends every backend mid-turn; reports parts lost | `terminate_live.py $R LABEL` |
| `lock_live.py` | Locks the task's row 2 s under a 200 ms statement timeout; reports gaps | `lock_live.py $R LABEL` |
| `restart_live.py` | Stops the database 4 s mid-turn; reports gaps | `restart_live.py $R LABEL` |
| `burst_live.py` | 2,000 or 6,000 events through an 8 s row lock under a 100 ms timeout | `burst_live.py $R LABEL [burst\|burst6k]` |
| `partition_live.py` | Cuts one replica's link 5 s and 20 s, restarts the database; PASS/FAIL | `partition_live.py $R` |
| `upgrade_live.py` | A 0.15.0 replica beside a new one; PASS/FAIL per scenario | `upgrade_live.py $OLD $R [--no-guard]` |
| `soak_matrix.sh` | The soak with the upgrade guard and turn claims on and off, at 0, 1 and 5 ms | `soak_matrix.sh` |
| `rtt.py` | The round trip a port adds, to state the latency a run had | `rtt.py 15432` |

Keep `A2A_SOAK_SECS` at 30 or more for the soak matrix. A 3 s run at 5 ms
measured 120 pairs after warm-up, and one 0.8 MiB step of the allocator read
as 6,963 bytes per pair, over the soak's 4,096 ceiling; the same run for
30 s read 431.

To compare two builds, build the replica at each commit into its own
`--target-dir` and run a script once per binary with a label each.
