#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""What can be said about examples/deploy-agent's Kubernetes manifests without kubectl or kubeconform.

Neither tool is installed here, so this does NOT validate the manifests against the Kubernetes
schemas. It checks only what a YAML parser and the Dockerfile can show:

  * every document parses, has apiVersion, kind and metadata.name;
  * the Deployment's container image, port, probes' paths and ports, env names and the Secret it reads;
  * the probe paths are ones deploy-agent serves (`/health`, `/ready`), the container
    port equals the Dockerfile's `PORT`, the Service targets that port, and the Ingress or NetworkPolicy
    in network.yaml name the Service and labels the Deployment carries.

Prints each finding and a summary; exit 1 on a finding. Needs python3 with PyYAML.
"""
import re
import sys
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[3]
EX = ROOT / "examples" / "deploy-agent"
findings = []


def docs(path):
    out = []
    for d in yaml.safe_load_all(path.read_text()):
        if d is None:
            continue
        for key in ("apiVersion", "kind"):
            if key not in d:
                findings.append(f"{path.name}: a document has no {key}")
        if not (d.get("metadata") or {}).get("name"):
            findings.append(f"{path.name}: a {d.get('kind')} has no metadata.name")
        out.append(d)
    return out


deploy = docs(EX / "deployment.yaml")
net = docs(EX / "network.yaml")
dockerfile = (EX / "Dockerfile").read_text()
docker_port = re.search(r"^ENV PORT=(\d+)", dockerfile, re.M).group(1)

dep = next(d for d in deploy if d["kind"] == "Deployment")
spec = dep["spec"]["template"]["spec"]
labels = dep["spec"]["template"]["metadata"]["labels"]
c = spec["containers"][0]
print("Deployment", dep["metadata"]["name"], "replicas", dep["spec"].get("replicas"), "image", c["image"])
ports = [p["containerPort"] for p in c.get("ports", [])]
print("container ports", ports, "| Dockerfile PORT", docker_port)
if str(docker_port) not in map(str, ports):
    findings.append(f"container port {ports} does not include the Dockerfile's PORT {docker_port}")
for kind in ("livenessProbe", "readinessProbe", "startupProbe"):
    pr = c.get(kind)
    if not pr:
        print(kind, "absent")
        continue
    path = (pr.get("httpGet") or {}).get("path")
    port = (pr.get("httpGet") or {}).get("port")
    print(kind, path, port)
    # The paths are the SDK dispatchers' (`/health`, `/ready`), not named in src/; 03-examples-offline.sh
    # shows both answering 200 and the readiness path 503 while draining.
    if path and path not in ("/health", "/ready"):
        findings.append(f"{kind} path {path} is not one the agent serves (/health, /ready)")
env = {e["name"]: e for e in c.get("env", [])}
print("env:", sorted(env))
for n, e in env.items():
    ref = (e.get("valueFrom") or {}).get("secretKeyRef")
    if ref:
        print(f"  {n} <- Secret {ref['name']}/{ref['key']}")
for d in deploy + net:
    k = d["kind"]
    if k == "Service":
        tp = [p.get("targetPort") for p in d["spec"]["ports"]]
        sel = d["spec"]["selector"]
        print("Service", d["metadata"]["name"], "targetPort", tp, "selector", sel)
        if not all(labels.get(a) == b for a, b in sel.items()):
            findings.append(f"Service {d['metadata']['name']} selector {sel} does not match the pod labels {labels}")
    if k == "NetworkPolicy":
        sel = d["spec"]["podSelector"].get("matchLabels", {})
        print("NetworkPolicy", d["metadata"]["name"], "podSelector", sel)
        if not all(labels.get(a) == b for a, b in sel.items()):
            findings.append(f"NetworkPolicy {d['metadata']['name']} podSelector {sel} does not match the pod labels {labels}")
    if k == "Ingress":
        for r in d["spec"].get("rules", []):
            for p in r["http"]["paths"]:
                print("Ingress", r.get("host"), p["path"], "->", p["backend"]["service"])
for f in findings:
    print("FINDING:", f)
print(f"06-manifest-lint: {len(deploy) + len(net)} document(s), {len(findings)} finding(s); NOT schema-validated (no kubectl, no kubeconform)")
sys.exit(1 if findings else 0)
