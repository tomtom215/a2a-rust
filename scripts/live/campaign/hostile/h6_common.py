# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Shared between h6_h2_ws.py and h6_sse_idle.py: a CPU-seconds reader, an RSS
watcher, a liveness check, and the one `verdict()` every case in both files
reports through. Not meant to be run on its own.
"""

import os
import threading
import time

from lib import task_of
from lib import send as _send


def cpu_s(pid):
    try:
        f = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
        return (int(f[11]) + int(f[12])) / os.sysconf("SC_CLK_TCK")
    except OSError:
        return -1.0


class Watch:
    """Samples the target's RSS while an attack runs."""

    def __init__(self, t):
        self.t, self.peak, self.stop = t, 0, False
        self.th = threading.Thread(target=self.loop, daemon=True)
        self.th.start()

    def loop(self):
        while not self.stop:
            self.peak = max(self.peak, self.t.rss_kb())
            time.sleep(0.1)

    def done(self):
        self.stop = True
        self.th.join()
        return self.peak


def healthy(t):
    try:
        st, r = _send(t.port, "echo:alive")
        return st == 200 and task_of(r) is not None
    except Exception as e:  # noqa: BLE001
        return f"{type(e).__name__}: {e}"


def verdict(t, r, name, run, bound_rss_mb=400, bound_cpu_s=60, extra_ok=None):
    base_rss, base_fds, c0 = t.rss_kb(), t.fds(), cpu_s(t.pid)
    w = Watch(t)
    t0 = time.time()
    try:
        detail = run() or {}
    except Exception as e:  # noqa: BLE001
        detail = {"harness_exception": f"{type(e).__name__}: {e}"}
    secs = time.time() - t0
    peak = w.done()
    time.sleep(1.0)
    h = healthy(t)
    st = t.state()
    problems = []
    if not st["alive"]:
        problems.append("TARGET DEAD")
    if st["panics"]:
        problems.append(f"{st['panics']} panicked line(s)")
    if h is not True:
        problems.append(f"legit send failed: {h}")
    if peak > bound_rss_mb * 1024:
        problems.append(f"RSS peak {peak // 1024} MB > {bound_rss_mb} MB")
    spent = round(cpu_s(t.pid) - c0, 1)
    if spent > bound_cpu_s:
        problems.append(f"CPU {spent}s > {bound_cpu_s}s")
    if extra_ok and not extra_ok(detail):
        problems.append(f"unexpected outcome {detail}")
    r.rec(name, "held" if not problems else "DEVIATION",
          {"problems": problems, "seconds": round(secs, 1), "rss_kb": [base_rss, peak, st["rss_kb"]],
           "fds": [base_fds, st["fds"]], "cpu_s": spent, **detail})
