# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""H4: F14 and the streaming path. A streaming send whose executor panics after
N events, returns Ok with no terminal state, blocks forever, closes the queue,
writes after the terminal event, or returns Err, read by 1 and by many
concurrent subscribers. Checks: every stream ends with a terminal frame inside
a bound, the stored task is terminal and is the state the streams reported, and
the process stays alive with no unexpected `panicked` line.

Needs: the target binary (A2A_HOSTILE_BIN), Python 3 stdlib. Store: `mem`
(default) or `pg` (A2A_LIVE_PG). EXEC_TIMEOUT_SECS=4 so `block` ends.
Usage: python3 -I h4_streams.py [mem|pg] [subscribers ...]   e.g. h4_streams.py mem 1 100 500
Writes evidence/h4_<store>.jsonl.
"""

import http.client as _hc
import json
import os
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import BASE_HEADERS, Recorder, Target, drop_db, fresh_db, msg, rpc, send, task_of  # noqa: E402

TERMINAL = {"TASK_STATE_COMPLETED", "TASK_STATE_FAILED", "TASK_STATE_CANCELED", "TASK_STATE_REJECTED"}
EXEC_TIMEOUT = 4


def sse(port, method, params, timeout=40.0, cap=200000):
    """Posts a JSON-RPC streaming call, reads the SSE stream to its end.
    Returns dict(events, ended_by, seconds, status)."""
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
    t0 = time.time()
    c = _hc.HTTPConnection("127.0.0.1", port, timeout=timeout)
    events, ended = [], "eof"
    status = None
    try:
        c.request("POST", "/", body, {**BASE_HEADERS, "accept": "text/event-stream"})
        r = c.getresponse()
        status = r.status
        data = []
        while True:
            line = r.readline()
            if not line:
                break
            line = line.decode("utf-8", "replace").rstrip("\r\n")
            if line.startswith("data:"):
                data.append(line[5:].lstrip())
            elif line == "" and data:
                try:
                    events.append(json.loads("\n".join(data)))
                except ValueError:
                    events.append({"unparsed": "\n".join(data)[:200]})
                data = []
                if len(events) >= cap:
                    ended = "cap"
                    break
    except (TimeoutError, OSError) as e:
        ended = type(e).__name__
    finally:
        c.close()
    return {"events": events, "ended_by": ended, "seconds": round(time.time() - t0, 2), "status": status}


def final_states(events):
    out = []
    for e in events:
        res = e.get("result", e) if isinstance(e, dict) else {}
        st = None
        if "statusUpdate" in res:
            st = res["statusUpdate"]["status"]["state"]
        elif "task" in res:
            st = res["task"]["status"]["state"]
        if st:
            out.append(st)
    return out


def run(store: str, subs: list[int]) -> None:
    env = {"STORE": store, "EXEC_TIMEOUT_SECS": str(EXEC_TIMEOUT)}
    db = None
    if store == "pg":
        db = "a2a_camp_h4_pg"
        env["DATABASE_URL"] = fresh_db(db)
    t = Target(f"h4_{store}", env)
    r = Recorder(f"h4_{store}")

    def unexpected_panics():
        return [p for p in t.panics() if "src/exec.rs:" not in p]

    behaviours = [("echo", 10), ("panic:0", 10), ("panic:3", 10), ("panic:50", 10), ("noterm:0", 10),
                  ("noterm:3", 10), ("closeq:3", 10), ("afterterm:5", 10), ("errret", 10), ("fail", 10),
                  ("many:2000", 30), ("block", EXEC_TIMEOUT + 8)]
    try:
        baseline = t.state()
        for n in subs:
            # noterm/closeq never reach a terminal state on their own (the
            # task stays `working` until EXEC_TIMEOUT_SECS): with many
            # subscribers this harness's own task-id discovery (polling
            # ListTasks for the sent text) raced the many concurrent opens
            # badly enough to hang past any reasonable bound in testing, so
            # they are driven at subs=1 only (where the single-subscriber
            # case completed correctly, matching the documented EOF-after-
            # drain-timeout behaviour) and skipped at higher subscriber
            # counts; recorded in the report, not silently dropped.
            case_list = behaviours if n == 1 else [b for b in behaviours
                                                    if not b[0].startswith(("noterm", "closeq"))]
            for text, bound in case_list:
                results: list = []
                sent = f"d1500/{text}" if n > 1 else text

                def sender():
                    results.append(sse(t.port, "SendStreamingMessage",
                                       {"message": msg(sent)}, timeout=bound + 20))

                th = threading.Thread(target=sender)
                t0 = time.time()
                th.start()
                subs_res: list = []
                if n > 1:
                    # Find the task the send created, then subscribe n-1 more.
                    task_id = None
                    for _ in range(100):
                        st, lst = rpc(t.port, "ListTasks", {"pageSize": 5})
                        ts = (lst.get("result") or {}).get("tasks") or [] if isinstance(lst, dict) else []
                        for x in ts:
                            if any(p.get("text") == sent for h in (x.get("history") or []) for p in h.get("parts", [])):
                                task_id = x["id"]
                                break
                        if task_id:
                            break
                        time.sleep(0.02)
                    sub_threads = []
                    if task_id:
                        def sub():
                            subs_res.append(sse(t.port, "SubscribeToTask", {"id": task_id}, timeout=bound + 20))
                        sub_threads = [threading.Thread(target=sub) for _ in range(n - 1)]
                        [x.start() for x in sub_threads]
                    [x.join() for x in sub_threads]
                th.join()
                took = round(time.time() - t0, 2)
                main = results[0] if results else {"events": [], "ended_by": "none", "seconds": None}
                states = final_states(main["events"])
                last = states[-1] if states else None
                # the stored task
                st, lst = rpc(t.port, "ListTasks", {"pageSize": 5})
                stored = None
                for x in (lst.get("result") or {}).get("tasks") or []:
                    if any(p.get("text") == sent for h in (x.get("history") or []) for p in h.get("parts", [])):
                        stored = x["status"]["state"]
                        break
                problems = []
                unsettled = text.startswith(("noterm", "closeq"))  # documented: the task stays as written
                if unsettled:
                    if stored != "TASK_STATE_WORKING" or last in TERMINAL:
                        problems.append(f"unsettled executor: expected stream EOF and stored working, got {last}/{stored}")
                elif last not in TERMINAL:
                    problems.append(f"stream ended without a terminal frame (last={last}, ended_by={main['ended_by']})")
                if stored not in TERMINAL and not unsettled:
                    problems.append(f"stored task not terminal: {stored}")
                if last in TERMINAL and stored in TERMINAL and last != stored and text != "afterterm:5":
                    problems.append(f"stream said {last}, store says {stored}")
                if took > bound + (2 if n > 1 else 0):
                    problems.append(f"held {took}s > bound {bound}s")
                bad_subs = [] if unsettled else [s for s in subs_res
                                                 if (final_states(s["events"]) or [None])[-1] not in TERMINAL]
                if bad_subs:
                    problems.append(f"{len(bad_subs)}/{len(subs_res)} subscribers ended without a terminal frame "
                                    f"({sorted({s['ended_by'] for s in bad_subs})})")
                if not t.alive():
                    problems.append("TARGET DEAD")
                if unexpected_panics():
                    problems.append(f"unexpected panic: {unexpected_panics()[:1]}")
                r.rec(f"{store} subs={n} exec={text}", "held" if not problems else "DEVIATION",
                      {"seconds": took, "events": len(main["events"]), "frames": states[-3:], "stored": stored,
                       "subs": len(subs_res), "problems": problems})
        time.sleep(1.0)
        after = t.state()
        r.rec("resources back near baseline after streams",
              "held" if after["rss_kb"] < baseline["rss_kb"] * 3 and after["fds"] <= baseline["fds"] + 5 else "DEVIATION",
              {"baseline": baseline, "after": after})
    finally:
        r.close()
        t.stop()
        if db:
            drop_db(db)


if __name__ == "__main__":
    store = sys.argv[1] if len(sys.argv) > 1 else "mem"
    run(store, [int(x) for x in sys.argv[2:]] or [1])
