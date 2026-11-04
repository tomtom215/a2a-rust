#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Live proof of the delegation hand-off (audit group M, ADR 0030).

Starts a worker (a2a-live-worker, tasks in PostgreSQL) and a supervisor
(a2a-live-supervisor, a PostgresDelegationJournal) against a FRESH database per
run, and runs, each `--runs` times:

  a   kill -9 the supervisor `--kill-at` seconds in, restart it as `recover`
      within 10 s. Expects every child Completed, nothing cancelled, no open
      record, no task left non-terminal.
  c   SIGTERM the supervisor instead. Expects it to exit promptly with every
      child handed off: records still open, tasks still working, nothing
      cancelled; then `recover` completes them all.
  c2  as c, with the supervisor as an agent behind Server::serve_with_shutdown
      (`serve`), the executor's cancellation token as the cancel future.
  al  as a, with 15 s leases: the worker grants them and the supervisor asks for
      them, on start and on reattach. The book's recovery loop is sequential, so
      only the child it is waiting on is renewed: predicts that the others are
      stopped when their leases lapse (a conjecture; see the evidence README).
  alold, alfix, allate, afix  after the library fix of 291aaf89 (a reattach with a lease
      renews at once; the book's loop reattaches every record before it waits):
      alold = al with the OLD serial `recover` (the control), alfix = al with
      `recover-all`, allate = alfix restarted after the leases lapsed (documents the
      limit), afix = a with `recover-all`, aledge = alfix restarted a second or so before the
      lapse (no outcome predicted). Restart delays are set with --restart-delays.
  cx  a control for c: SIGTERM, but the supervisor wires the signal to the cancel
      future (HAND_OFF=off), as the book's pattern did before wait_or_hand_off.
      Expects every child cancelled and every record settled: the harness must
      be able to see the failure c rules out.

Builds nothing: pass the two binaries. Counts come from psql on the journal
table (a2a_delegations) and the task table (tasks), and from the processes'
own logs. Exit status 1 if any expectation fails.

    delegation_live.py --worker W --supervisor S [--scenarios a,c,c2,cx,al,alold,alfix,allate,afix] [--restart-delays 3,7,7.3] [--runs 3]
        [--children 50] [--hold-secs 60] [--kill-at 20] [--seed N] [--out DIR]
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import random
import re
import signal
import socket
import statistics
import subprocess
import sys
import time
import urllib.request
import uuid

import pglocal

ROOT = pathlib.Path(__file__).resolve().parents[3]
SUP_LINE = re.compile(r"^t=(\d+) ([A-Z_]+)(?: (.*))?$")
WORKER_LINE = re.compile(r"^\[[^\]]*\] t=(\d+) (start|completed|token-fired|cancel-hook) task=(\S+)")
LIVE = {"completed", "failed", "canceled", "rejected"}
# scenario -> (leases, recovery mode, what is expected of the end state). Every one kills the supervisor
# with SIGKILL; c, c2 and cx (not here) send SIGTERM.
SCEN = {"a": (False, "recover", "complete"), "afix": (False, "recover-all", "complete"),
        "al": (True, "recover", "many-lost"), "alold": (True, "recover", "one-of-n"),
        "alfix": (True, "recover-all", "complete"), "allate": (True, "recover-all", "late"),
        "aledge": (True, "recover-all", "edge"),
        # one record that names a task the worker does not have, in the middle of the list:
        "afail": (False, "recover-all", "fail"), "afailold": (False, "recover", "fail")}


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def rpc(port: int, method: str, params: dict, timeout: float = 20) -> dict:
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    req = urllib.request.Request(f"http://127.0.0.1:{port}/", body,
                                 {"content-type": "application/json", "A2A-Version": "1.0"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.load(r)


def wait_serving(port: int) -> None:
    for _ in range(300):
        try:
            rpc(port, "GetTask", {"id": "none"})
            return
        except Exception:
            time.sleep(0.05)
    raise RuntimeError(f"nothing answered on {port}")


def spawn(binary: str, args: list[str], env: dict, log: pathlib.Path) -> subprocess.Popen:
    full = {"PATH": os.environ["PATH"], **env}
    return subprocess.Popen([binary, *args], env=full, stdout=open(log, "w"), stderr=subprocess.STDOUT)


def sup_lines(path: pathlib.Path) -> list[tuple[int, str, dict]]:
    """The supervisor's `t=<ms> KIND k=v ...` lines as (ms, kind, fields)."""
    out = []
    for line in path.read_text(errors="replace").splitlines():
        m = SUP_LINE.match(line)
        if m:
            fields = {k: v.strip('"') for k, v in re.findall(r'(\w+)=("[^"]*"|\S+)', m[3] or "")}
            out.append((int(m[1]), m[2], fields))
    return out


def worker_events(path: pathlib.Path) -> dict[str, list[int]]:
    ev: dict[str, list[int]] = {"start": [], "completed": [], "token-fired": [], "cancel-hook": []}
    for line in path.read_text(errors="replace").splitlines():
        m = WORKER_LINE.match(line)
        if m:
            ev[m[2]].append(int(m[1]))
    return ev


def wait_for(cond, timeout: float, step: float = 0.2) -> bool:
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if cond():
            return True
        time.sleep(step)
    return cond()


def st_is(snap: dict, word: str, n: int) -> bool:
    return state_n(snap, word) == n


def nonterminal(snap: dict) -> int:
    return sum(n for s, n in snap["tasks"].items() if not any(t in s.lower() for t in LIVE))


def state_n(snap: dict, word: str) -> int:
    return sum(n for s, n in snap["tasks"].items() if word in s.lower())


class Check:
    def __init__(self) -> None:
        self.items: list[tuple[str, bool, str]] = []

    def __call__(self, name: str, ok: bool, detail: str) -> None:
        self.items.append((name, bool(ok), detail))

    @property
    def failed(self) -> list[str]:
        return [f"{n}: {d}" for n, ok, d in self.items if not ok]


class Done(Exception):
    """The scenario has made all its checks."""


def control(chk: Check, res: dict, d: pathlib.Path, db: str, code, n: int, say) -> None:
    """cx: with the signal wired to the cancel future, SIGTERM cancels every child and
    settles every record, as a graceful redeploy did before wait_or_hand_off."""
    time.sleep(3)  # the CancelTask answers are in by now
    snap = pglocal.snapshot(db)
    w = worker_events(d / "worker.log")
    res.update(post=snap, worker={k: len(v) for k, v in w.items()})
    say(f"control, after the exit: journal {snap['journal']} tasks {snap['tasks']} worker cancel hooks {len(w['cancel-hook'])}")
    cancelled = sum(v for k, v in res["supervisor_outcomes"].items() if k in ("canceled", "cancel_requested"))
    chk("supervisor exited 0", code == 0, f"exit {code}")
    chk(f"all {n} children cancelled by the supervisor", cancelled == n, str(res["supervisor_outcomes"]))
    chk(f"all {n} records settled, none open", sum(snap["journal"].values()) == n and "<open>" not in snap["journal"], str(snap["journal"]))
    chk(f"all {n} tasks canceled", state_n(snap, "cancel") == n, str(snap["tasks"]))
    chk(f"{n} cancel hooks ran at the worker", len(w["cancel-hook"]) == n, str(len(w["cancel-hook"])))
    raise Done


def run_one(kind: str, run_no: int, cfg: argparse.Namespace, out: pathlib.Path, rec: list) -> dict:
    """One run of scenario `kind`; returns its measurements and checks."""
    seed = f"{cfg.seed}-{kind}-{run_no}"
    rng = random.Random(seed)
    n = cfg.children
    d = out / f"{kind}-{run_no}"
    d.mkdir(parents=True, exist_ok=True)
    db = f"a2a_deleg_{kind}{run_no}_{uuid.uuid4().hex[:8]}"
    url = pglocal.fresh_db(db)
    wport, sport = free_port(), free_port()
    owner = f"sup-{kind}-{run_no}"
    chk, res = Check(), {"scenario": kind, "run": run_no, "seed": seed, "children": n, "db": db}
    t_run = time.time()

    def say(msg: str) -> None:
        rec.append(f"{time.time() - t_run:8.2f}s [{kind}-{run_no}] {msg}")
        print(rec[-1], flush=True)

    leases, mode, expect = SCEN.get(kind, (False, "recover", "complete"))
    leased = {"LEASE": "1"} if leases else {}
    worker = spawn(cfg.worker, [], {"PORT": str(wport), "DATABASE_URL": url, "NAME": "worker",
                                    "HOLD_SECS": str(cfg.hold_secs), **leased}, d / "worker.log")
    sup = None
    try:
        wait_serving(wport)
        env = {"WORKER_URL": f"http://127.0.0.1:{wport}", "DATABASE_URL": url, "A2A_OWNER": owner}
        if leases:
            env["LEASE_SECS"] = str(cfg.lease_secs)
        slog = d / "supervisor.log"
        if kind == "c2":
            sup = spawn(cfg.supervisor, ["serve"], {**env, "PORT": str(sport)}, slog)
            wait_serving(sport)
            t0 = time.time()
            r = rpc(sport, "SendMessage", {"message": {"messageId": str(uuid.uuid4()), "role": "ROLE_USER",
                    "parts": [{"text": f"fan-out {n}"}]}, "configuration": {"returnImmediately": True}})
            say(f"fan-out accepted: {'error' if 'error' in r else 'ok'}")
        else:
            t0 = time.time()
            sup = spawn(cfg.supervisor, ["start", str(n)], {**env, **({"HAND_OFF": "off"} if kind == "cx" else {})}, slog)
        named = lambda: sum(1 for _, k, _f in sup_lines(slog) if k == "NAMED")
        wait_for(lambda: named() >= n, 30)
        say(f"children named: {named()} of {n} after {time.time() - t0:.1f}s")
        time.sleep(max(0.0, t0 + cfg.kill_at - time.time()))

        t_sig = time.time()
        sig = signal.SIGKILL if kind in SCEN else signal.SIGTERM
        sup.send_signal(sig)
        try:
            code = sup.wait(timeout=60)
        except subprocess.TimeoutExpired:
            sup.kill()
            code = "no exit in 60 s"
        t_dead = time.time()
        say(f"{sig.name} sent at {t_sig - t0:.2f}s of the run; supervisor exit {code} after {t_dead - t_sig:.2f}s")
        w = worker_events(d / "worker.log")
        starts = w["start"]
        res.update(kill_at_s=round(t_sig - t0, 2), named=named(), exit=code, exit_after_s=round(t_dead - t_sig, 2),
                   child_age_at_signal_s=[round((t_sig * 1000 - max(starts)) / 1000, 1),
                                          round((t_sig * 1000 - min(starts)) / 1000, 1)] if starts else None,
                   start_err=sum(1 for _, k, _f in sup_lines(slog) if k == "START_ERR"))
        outs = [f["outcome"].split()[0] for _, k, f in sup_lines(slog) if k == "OUTCOME" and "outcome" in f]
        res["supervisor_outcomes"] = {o: outs.count(o) for o in sorted(set(outs))}
        res["start_spread_ms"] = (max(starts) - min(starts)) if starts else None
        done_line = next((f for _, k, f in sup_lines(slog) if k == "EXECUTOR_DONE"), {})
        res["token_cancelled_at_handoff"] = done_line.get("token_cancelled")

        if kind == "cx":
            control(chk, res, d, db, code, n, say)
        # --restart-delays sets the delay from the kill to the launch of `recover`; without it
        # the delay is drawn from the seed (2 to 10 s, "restart within 10 s").
        if expect == "fail":  # a record whose task the worker has never heard of, sorting in the middle
            # `open` lists records by (started_at_ms, id): the bogus one takes the id of record n/2
            # plus a 'z', so it sorts straight after it, with n/2 + 1 real records before it.
            at, mid = pglocal.psql("SELECT id || '|' || started_at_ms FROM a2a_delegations ORDER BY started_at_ms, id OFFSET %d LIMIT 1" % (n // 2), db).split("|")
            pglocal.psql("INSERT INTO a2a_delegations (id, owner, agent, task_id, started_at_ms, updated_at_ms) "
                         f"SELECT '{at}z', owner, agent, 'no-such-task', {mid}, {mid} FROM a2a_delegations LIMIT 1", db)
            res["real_records_before_bogus"] = n // 2 + 1
        delay = cfg.restart_delays[(run_no - 1) % len(cfg.restart_delays)] if cfg.restart_delays else round(rng.uniform(2.0, 10.0), 1)
        time.sleep(max(0.0, delay - 0.5 - (time.time() - t_dead)))
        pre = pglocal.snapshot(db)  # about 0.2 s; taken just before the launch
        w = worker_events(d / "worker.log")
        res.update(restart_delay_s=delay, pre=pre, pre_worker_cancels=len(w["cancel-hook"]) + len(w["token-fired"]))
        time.sleep(max(0.0, delay - (time.time() - t_dead)))
        say(f"before recover: journal {pre['journal']} tasks {pre['tasks']} worker cancels {res['pre_worker_cancels']}")

        if kind in SCEN:
            chk("supervisor died by SIGKILL", code == -9, f"exit {code}")
        else:
            chk("supervisor exited 0 after SIGTERM", code == 0, f"exit {code}")
            chk(f"exit within {cfg.exit_bound:g}s of SIGTERM", isinstance(code, int) and t_dead - t_sig <= cfg.exit_bound,
                f"{t_dead - t_sig:.2f}s")
            chk("every child reported HandedOff", res["supervisor_outcomes"] == {"handed_off": n}, str(res["supervisor_outcomes"]))
            chk("no record settled by the exit", pre["journal"] == {"<open>": n}, str(pre["journal"]))
            chk("no task cancelled by the exit", state_n(pre, "cancel") == 0 and res["pre_worker_cancels"] == 0, str(pre["tasks"]))
        if kind == "c2":
            chk("executor token not yet fired when the children handed off", res["token_cancelled_at_handoff"] == "false",
                str(res["token_cancelled_at_handoff"]))
        chk(f"all {n} children named before the signal", res["named"] == n, str(res["named"]))
        chk("no rejected send", res["start_err"] == 0, f"{res['start_err']} START_ERR")
        if expect == "fail":
            chk(f"{n + 1} records open (one bogus) and {n} tasks working before recover",
                pre["journal"].get("<open>") == n + 1 and nonterminal(pre) == n, f"{pre['journal']} / {pre['tasks']}")
            chk("supervisor's death cancelled nothing", res["pre_worker_cancels"] == 0, f"{res['pre_worker_cancels']} cancel lines")
        elif expect == "late":  # the leases may have lapsed before the restart: only the records must be open
            chk(f"{n} records open before recover", pre["journal"].get("<open>") == n, str(pre["journal"]))
        else:
            chk(f"{n} records open and {n} tasks working before recover",
                pre["journal"].get("<open>") == n and nonterminal(pre) == n, f"{pre['journal']} / {pre['tasks']}")
            chk("supervisor's death cancelled nothing", res["pre_worker_cancels"] == 0, f"{res['pre_worker_cancels']} cancel lines")

        rlog = d / "recover.log"
        t_rec = time.time()
        res.update(recover_mode=mode, restart_after_s=round(t_rec - t_dead, 2), recover_launch_in_run_s=round(t_rec - t0, 2))
        rec_p = spawn(cfg.supervisor, [mode], {**env, **({"LINGER_SECS": str(cfg.linger_secs)} if expect == "fail" else {})}, rlog)
        try:
            rcode = rec_p.wait(timeout=cfg.hold_secs + 90)
        except subprocess.TimeoutExpired:
            rec_p.kill()
            rcode = "timeout"
        t_rec_end = time.time()
        res["recover_exit"], res["recover_s"] = rcode, round(time.time() - t_rec, 1)
        if expect == "fail":  # the state right after the failure, then once the unreached children have ended
            res["post_early"] = pglocal.snapshot(db)
            time.sleep(max(0.0, t0 + cfg.hold_secs + 6 - time.time()))
        post = pglocal.snapshot(db)
        w = worker_events(d / "worker.log")
        lines = sup_lines(rlog)
        routs = [f["outcome"].split()[0] for _, k, f in lines if k == "OUTCOME" and "outcome" in f]
        head = next((f for _, k, f in lines if k == "RECOVER"), {})
        last_done = max(w["completed"]) / 1000 if w["completed"] else None
        res["recover_exit_after_last_child_s"] = round(t_rec_end - last_done, 2) if last_done else None
        res["recover_line_in_run_s"] = next((round((ms - t0 * 1000) / 1000, 2) for ms, k, _f in lines if k == "RECOVER"), None)
        res["reattach_all_ms"] = next((int(f["took_ms"]) for _, k, f in lines if k == "REATTACHED"), None)
        res.update(post=post, recover_outcomes={o: routs.count(o) for o in sorted(set(routs))}, recover_head=head,
                   worker={k: len(v) for k, v in w.items()}, tasks_total=sum(post["tasks"].values()))
        say(f"recover exit {rcode} in {res['recover_s']}s: {res['recover_outcomes']}; journal {post['journal']} tasks {post['tasks']}")

        if expect == "fail":
            reattached = sum(1 for _, k, _f in lines if k == "REATTACH")
            err = next((f for _, k, f in lines if k == "ERROR"), None)
            res.update(reattach_lines=reattached, recover_error=bool(err))
            chk("recover failed on the bogus record (exit 1)", rcode == 1 and err is not None, f"exit {rcode}, ERROR line {bool(err)}")
        else:
            chk("recover exited 0", rcode == 0, str(rcode))
        want = n + 1 if expect == "fail" else n
        chk(f"recover found {want} open records, all with a task id", head.get("open") == str(want) and head.get("with_task_id") == str(want), str(head))
        if expect == "fail":  # a prediction, see the evidence README: the new loop cancels what it had reattached
            cancelled = state_n(post, "cancel")
            res["cancelled_by_failure"] = cancelled
            if mode == "recover-all":
                before_bogus = res["real_records_before_bogus"]
                chk(f"prediction: the {before_bogus} children reattached before the failure were cancelled", cancelled == before_bogus,
                    f"{cancelled} tasks canceled; journal {post['journal']}")
                chk("prediction: the others ran on to completion, their records open", state_n(post, "completed") == n - before_bogus and post["journal"].get("<open>") == n - before_bogus + 1,
                    f"tasks {post['tasks']}; journal {post['journal']}")
            else:
                chk("prediction: the serial loop cancelled nothing", cancelled == 0 and not w["cancel-hook"], f"{cancelled} tasks canceled; cancel-hook {len(w['cancel-hook'])}")
                chk("prediction: the serial loop completed what it had reached, the rest stay open", post["journal"].get("completed") == res["real_records_before_bogus"]
                    and post["journal"].get("<open>") == n - res["real_records_before_bogus"] + 1 and state_n(post, "completed") == n, f"journal {post['journal']}; tasks {post['tasks']}")
        elif expect == "one-of-n":  # fixed library, serial loop: the child waited on is renewed at once, the rest lapse
            j, tk = post["journal"], post["tasks"]
            chk("exactly 1 completed and the rest canceled", j == {"completed": 1, "canceled": n - 1}, str(j))
            chk("tasks: 1 completed, the rest canceled", st_is(post, "completed", 1) and st_is(post, "cancel", n - 1), str(tk))
            chk("the other children were stopped by their leases", len(w["token-fired"]) == n - 1, f"token-fired {len(w['token-fired'])}")
        elif expect == "edge":  # restarted close to the lapse: only reported, no outcome is predicted
            chk("no record left open", "<open>" not in post["journal"], str(post["journal"]))
        elif expect == "late":  # restarted after the lapse: documents the limit
            lost = post["journal"].get("canceled", 0)
            chk("limit: most children were stopped before the restart", lost >= n // 2, f"{lost} of {n} canceled; journal {post['journal']}")
            chk("no record left open", "<open>" not in post["journal"], str(post["journal"]))
        elif expect == "many-lost":  # the prediction: only the child the loop waits on keeps its lease
            lost = post["journal"].get("canceled", 0)
            chk("prediction: many children lost to a lapsed lease", lost >= n // 2, f"{lost} of {n} canceled; journal {post['journal']}")
            chk("prediction: at least one child completed", post["journal"].get("completed", 0) >= 1, str(post["journal"]))
            chk("no record left open", "<open>" not in post["journal"], str(post["journal"]))
            chk("lapsed leases stopped tasks at the worker", len(w["token-fired"]) == lost, f"token-fired {len(w['token-fired'])}")
        else:
            chk(f"recover: {n} outcomes completed", res["recover_outcomes"] == {"completed": n}, str(res["recover_outcomes"]))
            chk(f"journal: {n} completed, 0 canceled, 0 open", post["journal"] == {"completed": n}, str(post["journal"]))
            chk("tasks: all completed, none non-terminal", nonterminal(post) == 0 and state_n(post, "completed") == n, str(post["tasks"]))
            chk(f"worker: {n} tasks, none cancelled", res["tasks_total"] == n and not w["cancel-hook"] and not w["token-fired"],
                f"tasks {res['tasks_total']}, cancel-hook {len(w['cancel-hook'])}, token-fired {len(w['token-fired'])}")
            chk(f"worker: {n} completed log lines", len(w["completed"]) == n, str(len(w["completed"])))
    except Done:
        pass
    except Exception as e:  # a harness failure is a failed run, with the reason
        chk("run finished", False, f"{type(e).__name__}: {e}")
        say(f"harness error {type(e).__name__}: {e}")
    finally:
        for p in (sup, worker):
            if p and p.poll() is None:
                p.send_signal(signal.SIGINT if p is worker else signal.SIGKILL)
                try:
                    p.wait(timeout=20)
                except subprocess.TimeoutExpired:
                    p.kill()
        (d / "db-final.json").write_text(json.dumps(res.get("post"), indent=1))
        if not cfg.keep_db:
            pglocal.drop_db(db)
    res["checks"] = [{"name": a, "ok": b, "detail": c} for a, b, c in chk.items]
    res["failed"] = chk.failed
    say("PASS" if not chk.failed else f"FAIL {chk.failed}")
    return res


def table(results: list[dict]) -> str:
    cols = ["scn", "run", "seed", "sig@s", "age_s", "exit", "exit+s", "starts_ms", "restart", "mode", "rec@s", "reatt_ms", "open", "work", "cncl", "rec_s", "rec_last", "done",
            "j_comp", "j_canc", "j_open", "t_comp", "t_nonterm", "w_canc", "rej", "result"]
    rows = []
    for r in results:
        pre, post = r.get("pre", {}), r.get("post", {})
        rows.append([r["scenario"], r["run"], r["seed"].rsplit("-", 2)[0], r.get("kill_at_s", "-"),
                     (r.get("child_age_at_signal_s") or ["-"])[0], r.get("exit", "-"), r.get("exit_after_s", "-"),
                     r.get("start_spread_ms", "-"), r.get("restart_after_s", "-"), r.get("recover_mode", "-"),
                     r.get("recover_line_in_run_s", "-"), r.get("reattach_all_ms", "-"), pre.get("journal", {}).get("<open>", 0), state_n(pre, "working") if pre else "-",
                     r.get("pre_worker_cancels", "-"), r.get("recover_s", "-"), r.get("recover_exit_after_last_child_s", "-"),
                     r.get("recover_outcomes", {}).get("completed", 0),
                     post.get("journal", {}).get("completed", 0), post.get("journal", {}).get("canceled", 0),
                     post.get("journal", {}).get("<open>", 0), state_n(post, "completed") if post else "-",
                     nonterminal(post) if post else "-", r.get("worker", {}).get("cancel-hook", "-"), r.get("start_err", "-"),
                     "PASS" if not r["failed"] else "FAIL"])
    widths = [max(len(str(x)) for x in [c] + [row[i] for row in rows]) for i, c in enumerate(cols)]
    fmt = lambda row: "  ".join(str(x).rjust(w) for x, w in zip(row, widths))
    return "\n".join([fmt(cols), *map(fmt, rows)])


def spread(results: list[dict], key: str) -> str:
    v = [r[key] for r in results if isinstance(r.get(key), (int, float))]
    return f"min {min(v):g} / median {statistics.median(v):g} / max {max(v):g} (n={len(v)})" if v else "n/a"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--worker", required=True, help="path to a2a-live-worker")
    ap.add_argument("--supervisor", required=True, help="path to a2a-live-supervisor")
    ap.add_argument("--scenarios", default="a,c,c2,cx,al,alold,alfix,allate,afix")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--children", type=int, default=50)
    ap.add_argument("--hold-secs", type=int, default=60, help="how long a child holds, passed to the worker")
    ap.add_argument("--kill-at", type=float, default=20.0, help="seconds after the children start")
    ap.add_argument("--restart-delays", type=lambda v: [float(x) for x in v.split(",")], default=None,
                    help="seconds from the kill to the launch of recover, one per run in turn (default: drawn from the seed)")
    ap.add_argument("--linger-secs", type=int, default=3, help="afail, afailold: how long a failed recovery stays alive before it exits (LINGER_SECS)")
    ap.add_argument("--lease-secs", type=int, default=15, help="the lease TTL of scenario al")
    ap.add_argument("--exit-bound", type=float, default=10.0, help="'promptly': the longest SIGTERM-to-exit allowed")
    ap.add_argument("--seed", default=str(int(time.time())), help="seeds the restart delay of each run")
    ap.add_argument("--out", default=str(pathlib.Path(os.environ.get("A2A_LIVE_OUT", ROOT / "target" / "live")) / "delegation"))
    ap.add_argument("--keep-db", action="store_true")
    cfg = ap.parse_args()
    out = pathlib.Path(cfg.out) / time.strftime("%Y%m%dT%H%M%S")
    out.mkdir(parents=True, exist_ok=True)
    rec = [f"server_version {pglocal.ensure_running()}  seed {cfg.seed}  out {out}"]
    print(rec[0], flush=True)
    results = []
    for kind in cfg.scenarios.split(","):
        for run_no in range(1, cfg.runs + 1):
            results.append(run_one(kind, run_no, cfg, out, rec))
    summary = ["", table(results), ""]
    for kind in dict.fromkeys(r["scenario"] for r in results):
        sel = [r for r in results if r["scenario"] == kind]
        summary += [f"{kind}: exit_after_s {spread(sel, 'exit_after_s')}",
                    f"{kind}: restart_after_s {spread(sel, 'restart_after_s')}",
                    f"{kind}: recover_s {spread(sel, 'recover_s')}",
                    f"{kind}: recover_exit_after_last_child_s {spread(sel, 'recover_exit_after_last_child_s')}",
                    f"{kind}: start_spread_ms {spread(sel, 'start_spread_ms')}",
                    f"{kind}: passed {sum(1 for r in sel if not r['failed'])} of {len(sel)}"]
    summary += [f"FAILED {r['scenario']}-{r['run']}: {f}" for r in results for f in r["failed"]]
    print("\n".join(summary))
    rec += summary
    (out / "raw.log").write_text("\n".join(rec) + "\n")
    (out / "results.json").write_text(json.dumps(results, indent=1))
    print(f"raw log and per-run logs: {out}")
    return 1 if any(r["failed"] for r in results) else 0


if __name__ == "__main__":
    sys.exit(main())
