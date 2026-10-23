#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Checks the a2a-rust dashboards against a live Grafana and Prometheus.

For each dashboard JSON in DASHBOARD_DIR:

1. With --grafana: Grafana has provisioned it (GET /api/dashboards/uid/<uid>), in folder A2A,
   and its stored panels are the file's panels.
2. Every query of every panel, run through Grafana's own query API
   (POST /api/ds/query, the path a browser takes) against the provisioned
   data source — or, without --grafana, through Prometheus's range-query
   API — over the last LOOKBACK minutes, returns at least one series
   with at least one value — unless the panel is named in --expect-empty,
   whose queries must still run without an error.
3. Every template variable's query returns at least one value.

It also checks the Prometheus rules are loaded and healthy (GET
/api/v1/rules: every rule's health is "ok" and lastError empty) and that
every recording rule has data (GET /api/v1/query).

Exit 0 when all hold, 1 otherwise, 2 on a usage or connection error.

Usage: check_dashboards.py [--grafana URL] --prometheus URL DASHBOARD_DIR
       [--expect-empty "Panel title" ...] [--lookback MINUTES]
"""
import argparse
import base64
import json
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path


def call(url, body=None, auth=None):
    req = urllib.request.Request(url, None if body is None else json.dumps(body).encode(),
                                 {"content-type": "application/json"})
    if auth:
        req.add_header("authorization", "Basic " + base64.b64encode(auth.encode()).decode())
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.load(resp)


def substitute(expr):
    for name in ("$job", "$instance", "$server"):
        expr = expr.replace(f'"{name}"', '".*"')
    return expr


def frames_have_values(result):
    for frame in result.get("frames", []):
        values = frame.get("data", {}).get("values", [])
        if len(values) >= 2 and any(len(v) for v in values[1:]):
            return True
    return False


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--grafana")
    ap.add_argument("--prometheus", required=True)
    ap.add_argument("--auth", default="admin:admin")
    ap.add_argument("--expect-empty", nargs="*", default=[])
    ap.add_argument("--lookback", type=int, default=15)
    ap.add_argument("dashboards", type=Path)
    a = ap.parse_args()
    failures = []
    now = int(time.time() * 1000)
    start = now - a.lookback * 60 * 1000

    try:
        rules = call(f"{a.prometheus}/api/v1/rules")["data"]["groups"]
    except (urllib.error.URLError, OSError) as e:
        print(f"check_dashboards: cannot reach Prometheus: {e}", file=sys.stderr)
        return 2
    n_rules = 0
    for group in rules:
        for rule in group["rules"]:
            n_rules += 1
            if rule.get("health") != "ok" or rule.get("lastError"):
                failures.append(f"rule {rule['name']}: health {rule.get('health')} {rule.get('lastError', '')}")
            if rule.get("type") == "recording":
                res = call(f"{a.prometheus}/api/v1/query?query={urllib.request.quote(rule['name'])}")
                if not res["data"]["result"]:
                    failures.append(f"recording rule {rule['name']}: no series")
    print(f"prometheus: {n_rules} rules checked")

    expected_empty = set(a.expect_empty)
    seen_empty = set()
    for path in sorted(a.dashboards.glob("*.json")):
        board = json.loads(path.read_text())
        uid = board["uid"]
        if a.grafana:
            try:
                stored = call(f"{a.grafana}/api/dashboards/uid/{uid}", auth=a.auth)
            except urllib.error.HTTPError as e:
                failures.append(f"{path.name}: not provisioned ({e.code})")
                continue
            if stored["meta"].get("folderTitle") != "A2A":
                failures.append(f"{path.name}: in folder {stored['meta'].get('folderTitle')!r}, not 'A2A'")
            titles = [p["title"] for p in board["panels"]]
            if [p["title"] for p in stored["dashboard"]["panels"]] != titles:
                failures.append(f"{path.name}: stored panels differ from the file's")
        for var in board["templating"]["list"]:
            body = {"from": str(start), "to": str(now), "queries": [{
                "refId": "V", "datasource": var["datasource"], "expr": substitute(var["query"]["query"]),
                "qryType": 1, "queryType": "metricFindQuery"}]}
            # A label_values() variable is answered by the series API.
            q = substitute(var["query"]["query"])
            label = q[q.rindex(",") + 1:q.rindex(")")].strip() if "," in q else q[q.index("(") + 1:q.rindex(")")]
            selector = q[q.index("(") + 1:q.rindex(",")] if "," in q else None
            url = f"{a.prometheus}/api/v1/label/{label}/values"
            if selector:
                url += f"?match[]={urllib.request.quote(selector)}"
            values = call(url)["data"]
            if not values:
                failures.append(f"{path.name}: variable {var['name']} has no values")
        n = 0
        for panel in board["panels"]:
            if panel["type"] == "row":
                continue
            empty_ok = panel["title"] in expected_empty
            has_data = False
            for t in panel["targets"]:
                n += 1
                if not a.grafana:
                    q = urllib.parse.urlencode({"query": substitute(t["expr"]), "start": start / 1000,
                                                "end": now / 1000, "step": 15})
                    res = call(f"{a.prometheus}/api/v1/query_range?{q}")
                    if res.get("status") != "success":
                        failures.append(f"{path.name} / {panel['title']} / {t['refId']}: {res}")
                    has_data |= any(r["values"] for r in res["data"]["result"])
                    continue
                body = {"from": str(start), "to": str(now), "queries": [{
                    "refId": t["refId"], "datasource": t["datasource"], "expr": substitute(t["expr"]),
                    "intervalMs": 15000, "maxDataPoints": 500, "range": True}]}
                try:
                    res = call(f"{a.grafana}/api/ds/query", body, auth=a.auth)["results"][t["refId"]]
                except urllib.error.HTTPError as e:
                    failures.append(f"{path.name} / {panel['title']} / {t['refId']}: HTTP {e.code} {e.read()[:200]!r}")
                    continue
                if res.get("error"):
                    failures.append(f"{path.name} / {panel['title']} / {t['refId']}: {res['error']}")
                has_data |= frames_have_values(res)
            if empty_ok:
                seen_empty.add(panel["title"])
                print(f"  {path.name} / {panel['title']}: {'data' if has_data else 'no data'} (allowed empty)")
            elif not has_data:
                failures.append(f"{path.name} / {panel['title']}: no data")
            else:
                print(f"  {path.name} / {panel['title']}: data")
        print(f"{path.name}: {n} queries")
    for title in expected_empty - seen_empty:
        failures.append(f"--expect-empty names no panel: {title!r}")
    for f in failures:
        print(f"FAIL {f}")
    print("check_dashboards:", "FAILED" if failures else "ok")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
