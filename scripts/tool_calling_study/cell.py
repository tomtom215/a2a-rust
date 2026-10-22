# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Runs one study cell a seed at a time, so a killed cell resumes where it stopped.

  cell.py --out FILE --n N --seed-base S -- <run.py or sdk_arm.py command without --n/--seed-base/--out>

run.py and sdk_arm.py rewrite their output after every run, but rerun a cell
from its first seed: a container restart on 2026-10-21 cost two thinking-on
cells 27 and 14 runs. This wrapper calls the unchanged harness once per
missing seed and merges, so the harness hashes recorded in finished cells
still describe every cell.

- run.py is called with --prompts KIND --n 1 --seed-base SEED: with one
  prompt kind, k is 0 and the run's seed is SEED, the seed it has in a whole
  cell (seed_base + 1000*k + i).
- sdk_arm.py has no --prompts, so one call with --n 1 --seed-base base+i runs
  the pair (base+i, base+1000+i). A seed already stored is kept, and the
  re-run copy is compared with it: `replays` records whether the two match.
- Every chunk's header must equal the cell's on every field but n and
  seed_base; a server or harness that changed between chunks stops the cell.
- The merged file is written to FILE.tmp and renamed, so a full disk leaves
  the last good file in place.
- The server's /health must answer 200 before each chunk, and again after a
  chunk that recorded a server or HTTP error; if it does not, the chunk is
  discarded and the cell stops. An error from a healthy server is kept, as
  in cells run whole. On 2026-10-22 a driver started before its servers had
  loaded filled two SDK cells with 60 failed runs each in 7 seconds.

Each run is independent of the ones before it (cache_prompt is off in both
arms), which is what lets the order of runs differ from a whole-cell run;
replay was measured identical sequentially (8 of 8, 2026-10-20).
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request

KINDS = ("billing", "checkout")
VARYING = ("n", "seed_base", "runs")


def header(d):
    return {k: v for k, v in d.items() if k not in VARYING and k != "assembly"}


def comparable(run):
    return {k: v for k, v in run.items() if k != "secs"}


def healthy(port):
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=10) as resp:
            return resp.status == 200
    except (urllib.error.URLError, OSError):
        return False


def errored(run):
    return run.get("server_error") or any("http_error" in t for t in run.get("turns", []))


def save(path, cell):
    tmp = path + ".tmp"
    with open(tmp, "w") as f:
        json.dump(cell, f, indent=1)
        f.flush()
        os.fsync(f.fileno())
    os.replace(tmp, path)


def main():
    argv = sys.argv[1:]
    if "--" not in argv:
        sys.exit("usage: cell.py --out FILE --n N --seed-base S -- COMMAND...")
    split = argv.index("--")
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--n", type=int, required=True)
    ap.add_argument("--seed-base", type=int, required=True)
    a = ap.parse_args(argv[:split])
    cmd = argv[split + 1:]
    sdk = any(part.endswith("sdk_arm.py") for part in cmd)
    port = cmd[cmd.index("--port") + 1]
    want = {kind: [a.seed_base + 1000 * k + i for i in range(a.n)] for k, kind in enumerate(KINDS)}
    with open(__file__, "rb") as f:
        me = hashlib.sha256(f.read()).hexdigest()

    cell = None
    if os.path.exists(a.out):
        with open(a.out) as f:
            cell = json.load(f)
        if cell["n"] != a.n or cell["seed_base"] != a.seed_base:
            sys.exit(f"{a.out}: n/seed_base {cell['n']}/{cell['seed_base']}, asked {a.n}/{a.seed_base}")
    have = {r["seed"]: r for r in cell["runs"]} if cell else {}
    if cell is not None:
        cell.setdefault("assembly", {"cell_py_sha256": me, "resumed_with": len(have), "replays": []})

    with tempfile.TemporaryDirectory() as tmpdir:
        chunk_out = os.path.join(tmpdir, "chunk.json")
        for i in range(a.n):
            missing = [kind for kind in KINDS if want[kind][i] not in have]
            if not missing:
                continue
            if sdk:
                chunks = [["--n", "1", "--seed-base", str(a.seed_base + i)]]
            else:
                chunks = [["--prompts", kind, "--n", "1", "--seed-base", str(want[kind][i])] for kind in missing]
            for extra in chunks:
                if os.path.exists(chunk_out):
                    os.remove(chunk_out)
                if not healthy(port):
                    sys.exit(f"server on port {port} is not healthy; chunk {extra} not run")
                proc = subprocess.run(cmd + extra + ["--out", chunk_out])
                if proc.returncode != 0 or not os.path.exists(chunk_out):
                    sys.exit(f"chunk {extra} exited {proc.returncode}")
                with open(chunk_out) as f:
                    got = json.load(f)
                if cell is None:
                    cell = dict(got, n=a.n, seed_base=a.seed_base, runs=[],
                                assembly={"cell_py_sha256": me, "resumed_with": 0, "replays": []})
                elif header(got) != header(cell):
                    diff = sorted(k for k in set(header(got)) | set(header(cell))
                                  if header(got).get(k) != header(cell).get(k))
                    sys.exit(f"chunk {extra}: header differs from the cell in {diff}")
                if any(errored(run) for run in got["runs"]) and not healthy(port):
                    sys.exit(f"chunk {extra} errored and the server is no longer healthy; discarded")
                for run in got["runs"]:
                    old = have.get(run["seed"])
                    if old is None:
                        have[run["seed"]] = run
                    else:
                        cell["assembly"]["replays"].append(
                            {"seed": run["seed"], "identical": comparable(old) == comparable(run)})
                cell["runs"] = [have[s] for kind in KINDS for s in want[kind] if s in have]
                save(a.out, cell)
    done = sum(s in have for kind in KINDS for s in want[kind])
    print(f"{a.out}: {done} of {2 * a.n} runs")
    sys.exit(0 if done == 2 * a.n else 1)


if __name__ == "__main__":
    main()
