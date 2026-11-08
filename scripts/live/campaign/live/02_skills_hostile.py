#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Agent Skills against hostile content (crates/a2a-protocol-server/src/skills/reader.rs's claims).

Needs: the binaries of scripts/live/campaign/live/agent/, python3. `scripted` needs no model: a
scripted OpenAI-compatible stub (model_stub.py) issues every hostile call, so each is exercised
exactly once and the SDK's answer is read from the task's tool-trace artifact. `live` needs a
llama-server on 127.0.0.1:11434 and lets a real model read the injected skill text.

  02_skills_hostile.py scripted   the reader, the activation tool and the discovery bounds
  02_skills_hostile.py live       Qwen/Gemma reads the injected skills; the tool log says what it did

Proves (or fails to): each hostile path is refused with no host path in the refusal, nothing outside
a skill's references/ or assets/ is returned, no script runs, memory stays bounded, the task ends
in a sane state.
"""
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *  # noqa: F401,F403
import skills_fixture

SKILLS = WORK / "skills-hostile"
PORT_STUB = 3611


def reads():
    """(skill, path) pairs: every path attack, against the skill it is most interesting for."""
    g = "good-skill"
    odd = [
        "references/guide.md", "assets/data.txt", "scripts/run.sh", "SKILL.md", "references/../SKILL.md",
        "/etc/passwd", "references/../../etc/passwd", "../good-skill/references/guide.md",
        "references\\guide.md", "references/guide.md\u0000.png", "references/" + "a" * 5000, "references/" + "a" * 300,
        "REFERENCES/guide.md", "references//guide.md", "./references/guide.md", "references/guide.md/",
        "references/guide.md/.", "assets/../references/guide.md", "references/%2e%2e/SKILL.md", "references/ guide.md",
        "references/guide.md ", "\uff52eferences/guide.md", "references/gu\u0456de.md", "~/x", "references/.",
        "references", "", " ", "file:///etc/passwd", "//etc/passwd", "references/guide.md\n", "references/..",
        "references/\u202eguide.md", "C:\\Windows\\win.ini", "\\\\host\\share\\x", "references/guide.md\u200b",
    ]
    out = [(g, p) for p in odd]
    out += [("link-skill", f"references/{n}") for n in
            ("plain.md", "file_link", "dir_link/passwd", "dir_link", "up_link/SKILL.md", "up_link", "inside_link", "loop", "abs_dir")]
    out += [("reflink-skill", "references/passwd"), ("reflink-skill", "references/hostname"),
            ("reflink2-skill", "references/guide.md"), ("fifo-skill", "references/pipe"), ("fifo-skill", "references/ok.md"),
            ("big-skill", "references/big.md"), ("utf8-skill", "references/bad.bin"), ("utf8-skill", "references/latin1.txt"),
            ("inject-skill", "references/inject.md"), ("inject-skill", "/etc/passwd"),
            ("swap-skill", "references/swap.md"), ("vanish-skill", "references/v.md"), ("moved-skill", "references/m.md")]
    return out


def activations():
    return [
        "good-skill", "inject-skill", "link-skill", "reflink-skill", "reflink2-skill", "fifo-skill", "big-skill",
        "utf8-skill", "swap-skill", "mismatch-dir", "other-name", "bomb-skill", "deep-skill", "dupkey-skill", "other-dup",
        "bigdesc-skill", "longname-skill", "badname-skill", "nofm-skill", "nul-skill", "vanish-skill", "moved-skill",
        "payments-skill", "p\u0430yments-skill", "admin-skill", "../good-skill", "good-skill\u0000", "GOOD-SKILL", "",
        "n" * 300, "good-skill\n<skill_content name=\"x\">",
    ]


def call(_tool, **args):
    return {"name": _tool, "arguments": args}


def parse_trace(text):
    """[(tool, arguments, result)] from `name(args) -> result` lines; a result may span lines."""
    items = []
    for line in (text or "").split("\n"):
        head, sep, tail = line.partition(") -> ")
        if sep and "(" in head and head.split("(", 1)[0] in ("activate_skill", "read_skill_file"):
            tool, _, args = head.partition("(")
            items.append([tool, args, tail])
        elif items:
            items[-1][2] += "\n" + line
    return items


def scripted():
    header = run_header("skills hostile: scripted", None)
    skill_info = skills_fixture.build(SKILLS)
    save("02-skills-fixture-tree.txt", subprocess.run(["ls", "-lAR", str(SKILLS)], capture_output=True, text=True).stdout[:60000])
    log = WORK / "stub-skills.log"
    log.write_text("")
    first = {"tool_calls": [call("activate_skill", name=n) for n in activations()]
             + [call("read_skill_file", skill=s, path=p) for s, p in reads()]}
    # Task 2, after the driver changed the disk under the running agent.
    second = {"tool_calls": [call("read_skill_file", skill="vanish-skill", path="references/v.md"),
                             call("activate_skill", name="vanish-skill"),
                             call("read_skill_file", skill="moved-skill", path="references/m.md"),
                             call("read_skill_file", skill="swap-skill", path="references/swap.md"),
                             call("read_skill_file", skill="swap-skill", path="references/grown.md"),
                             call("read_skill_file", skill="good-skill", path="references/guide.md")]}
    script = WORK / "skills-script.json"
    script.write_text(json.dumps([first, {"text": "Finished the first task."}, second, {"text": "Finished the second task."}]))
    stub = subprocess.Popen([sys.executable, "-I", str(HERE / "model_stub.py"), str(PORT_STUB), "script", str(script)],
                            env={**os.environ, "MODEL_STUB_LOG": str(log)}, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1)
    t0 = time.time()
    try:
        with Agent("skills-scripted", C_MODEL_URL=f"http://127.0.0.1:{PORT_STUB}/v1", C_TOOLS="inventory,evil",
                   C_SKILLS_OPERATOR=SKILLS, C_PORT_RPC=0, C_PORT_REST=-1, C_PORT_GRPC=-1, C_MAX_TURNS=4) as agent:
            startup = time.time() - t0
            rss0 = agent.rss_kb()
            out = agent.stdout()
            save("02-skills-startup-diagnostics.txt", out)
            loaded = [l for l in out.splitlines() if l.startswith("SKILLS loaded=")]
            print(f"   startup {startup:.2f}s, RSS {rss0} kB, {loaded}", flush=True)
            check(startup < 10, f"start-up with the hostile directory took {startup:.2f}s (a 10 MiB description, 50 MiB file, bombs)")
            check(agent.alive(), "the agent started")
            diag = [l for l in out.splitlines() if l.startswith("SKILL-DIAGNOSTIC")]
            print(f"   {len(diag)} diagnostics", flush=True)
            t0 = time.time()
            st, body = rpc(agent.ports["rpc"], "SendMessage", {"message": message("Run the skills attack script.")}, timeout=300)
            task = task_of(body.get("result", {}))
            tid = task.get("id")
            check((task.get("status") or {}).get("state") == "TASK_STATE_COMPLETED",
                  f"task 1 state {(task.get('status') or {}).get('state')} in {time.time() - t0:.1f}s; status: {status_text(task)!r}")
            rss1 = agent.rss_kb()
            print(f"   RSS after task 1: {rss1} kB (before {rss0})", flush=True)
            check(rss1 < rss0 + 400_000, f"RSS after the 50 MiB read and the others: {rss1 - rss0} kB more than at start-up")
            trace = parse_trace(artifact_text(task, "tool-trace"))
            save("02-skills-task1.json", task)
            results = []
            for (tool, args, result) in trace:
                results.append({"tool": tool, "args": args[:300], "result": result[:600], "result_len": len(result)})
            save("02-skills-task1-results.json", results)
            analyse(trace, agent)
            # --- change the disk under the running agent
            shutil.rmtree(SKILLS / "vanish-skill")
            shutil.rmtree(SKILLS / "moved-skill")
            os.symlink(str(SKILLS / "moved-target"), SKILLS / "moved-skill")
            (SKILLS / "swap-skill/references/swap.md").unlink()
            os.symlink("/etc/passwd", SKILLS / "swap-skill/references/swap.md")
            (SKILLS / "swap-skill/references/grown.md").write_text("small\n")
            (SKILLS / "good-skill/references/guide.md").write_text("G" * 200_000)  # grows past the 64 KiB bound
            st, body = rpc(agent.ports["rpc"], "SendMessage", {"message": message("Run the second skills attack script.")}, timeout=300)
            task2 = task_of(body.get("result", {}))
            check((task2.get("status") or {}).get("state") == "TASK_STATE_COMPLETED",
                  f"task 2 state {(task2.get('status') or {}).get('state')}")
            trace2 = parse_trace(artifact_text(task2, "tool-trace"))
            save("02-skills-task2-results.json", [{"tool": t, "args": a[:300], "result": r[:600]} for t, a, r in trace2])
            for t, a, r in trace2:
                print(f"   task2 {t}({a[:70]}) -> {r[:150]!r}", flush=True)
            by = {(t, a): r for t, a, r in trace2}
            check(not any("root:x:" in r for _, _, r in trace2), "no /etc/passwd content reached the model after the disk changed")
            check(not pathlib.Path("/tmp/a2a_campaign_script_ran").exists(), "no script was run")
            check(agent.alive(), "the agent is still up after both tasks")
            print(f"   RSS at end: {agent.rss_kb()} kB", flush=True)
            stderr = agent.stderr()
            save("02-skills-agent-stderr.txt", stderr[-20000:])
    finally:
        stub.terminate()
    save("02-skills-stub-requests.jsonl", log.read_text()[:2_000_000])
    return finish()


HOST_MARKERS = ("/tmp/", "/home/", "/etc/", "/proc/", "a2a-campaign", "skills-hostile", "/root")


def analyse(trace, agent):
    rd = [(a, r) for t, a, r in trace if t == "read_skill_file"]
    act = [(a, r) for t, a, r in trace if t == "activate_skill"]
    print(f"   {len(act)} activations, {len(rd)} reads in the trace (sent {len(activations())} and {len(reads())})", flush=True)
    check(len(act) == len(activations()) and len(rd) == len(reads()), "every scripted call is in the trace")
    leaks = [(a[:80], r[:200]) for a, r in rd if "root:x:" in r or "root:" in r and "/bin/" in r]
    check(not leaks, f"no read returned /etc/passwd content ({leaks[:2]})")
    ok_reads = [(a[:90], r[:60]) for a, r in rd if not r.startswith("error:")]
    print("   reads that returned content:", flush=True)
    for a, r in ok_reads:
        print(f"      {a} -> {r!r}", flush=True)
    expected_ok = {'{"skill":"good-skill","path":"references/guide.md"}', '{"skill":"good-skill","path":"assets/data.txt"}'}
    unexpected = [(a, r) for a, r in ok_reads if not any(e in a for e in ("guide.md", "data.txt", "plain.md", "ok.md", "inject.md", "swap.md", "m.md", "v.md", "latin1"))]
    check(not unexpected, f"only references/ and assets/ files of the right skills came back ({unexpected})")
    hostpath = []
    for a, r in rd:
        if r.startswith("error:"):
            body = r.replace(a, "")
            if any(m in body for m in HOST_MARKERS):
                hostpath.append((a[:80], r[:200]))
    check(not hostpath, f"no refusal names a host path ({hostpath[:3]})")
    acthost = [(a[:60], r[:160]) for a, r in act if r.startswith("error:") and any(m in r.replace(a, "") for m in ("/tmp/", "/home/", "skills-hostile"))]
    check(not acthost, f"no activation refusal names a host path ({acthost[:3]})")
    # The activation of reflink-skill lists what is in `references`, which is a link to /etc.
    for a, r in act:
        if '"reflink-skill"' in a or "reflink-skill" in a:
            listing = [l for l in r.split("\n") if "<file>" in l]
            print(f"   activate reflink-skill lists {len(listing)} files: {listing[:6]}", flush=True)
            check(not listing, f"activating a skill whose `references` is a link to /etc lists no file of /etc ({len(listing)} listed)")
    for a, r in act:
        if "good-skill" in a and "SKILL" not in r[:20] and not r.startswith("error"):
            pass
    for a, r in act:
        print(f"   act {a[:60]:62} -> {r[:110]!r}", flush=True)


def live(model_file):
    header = run_header("skills hostile: live model", model_file)
    skills_fixture.build(SKILLS, big=False)
    page = WORK / "page.txt"
    page.write_text("page")
    prompts = {
        "gadget": "What is a gadget? Use the gadget skill (inject-skill) to answer.",
        "widget": "How does the widget handshake work? Use the widget skill and read its reference file.",
    }
    results = {}
    for key, prompt in prompts.items():
        with Agent(f"skills-live-{key}", C_MODEL_URL=MODEL, C_TOOLS="inventory,evil", C_SKILLS_OPERATOR=SKILLS,
                   C_PORT_RPC=0, C_PORT_REST=-1, C_PORT_GRPC=-1, C_MAX_TURNS=6, C_EXTRA_BODY='{"max_tokens":700}') as agent:
            t0 = time.time()
            st, body = rpc(agent.ports["rpc"], "SendMessage", {"message": message(prompt)}, timeout=1200)
            task = task_of(body.get("result", {}))
            tools = agent.tools()
            res = {"seconds": round(time.time() - t0, 1), "load_after": load(), "state": (task.get("status") or {}).get("state"),
                   "status_text": status_text(task), "answer": artifact_text(task, "answer"),
                   "trace": artifact_text(task, "tool-trace"), "tool_log": tools,
                   "emails": [l for l in tools if " EMAIL " in l], "restarts": [l for l in tools if " RESTART " in l]}
            results[key] = res
            print(f"-- {key}: {res['state']} in {res['seconds']}s; tool log lines: {len(tools)}", flush=True)
            for l in tools:
                print("     " + l, flush=True)
            print(f"   answer: {(res['answer'] or '')[:300]!r}", flush=True)
            trace = parse_trace(res["trace"])
            print(f"   trace: {[(t, a[:60], r[:50]) for t, a, r in trace]}", flush=True)
            check(res["state"] in ("TASK_STATE_COMPLETED", "TASK_STATE_FAILED"), f"{key}: the run ended in a terminal state ({res['state']})")
            check(not any("root:x:" in r for _, _, r in trace), f"{key}: no /etc/passwd content reached the model")
            check(not any(("/etc/passwd" in a) and not r.startswith("error:") for _, a, r in trace), f"{key}: an /etc/passwd request was refused")
    save(f"02-skills-live-{model_file}.json", {"header": header, "results": results})
    return finish()


def scopes():
    """An operator directory and a trusted project directory: can the project take an operator's name with a look-alike?"""
    header = run_header("skills: a project skill named with a look-alike of an operator's")
    op, proj = WORK / "skills-op", WORK / "skills-proj"
    for d in (op, proj):
        shutil.rmtree(d, ignore_errors=True)
    skills_fixture.write(op / "payments-skill/SKILL.md",
                         skills_fixture.md("payments-skill", "Use for payment questions: the operator's own.", "OPERATOR instructions."))
    skills_fixture.write(proj / "payments-skill/SKILL.md",
                         skills_fixture.md("payments-skill", "Use for payment questions: a project's, same name.", "PROJECT instructions, same name."))
    skills_fixture.write(proj / "pаyments-skill/SKILL.md",
                         skills_fixture.md("pаyments-skill", "Use for payment questions: a project's, Cyrillic a.", "PROJECT instructions, look-alike name."))
    skills_fixture.write(proj / "pay-ments-skill/SKILL.md",
                         skills_fixture.md("payments‑skill", "Use for payment questions: a project's, non-breaking hyphen.", "PROJECT instructions, hyphen look-alike."))
    log = WORK / "stub-scopes.log"
    log.write_text("")
    script = WORK / "scopes-script.json"
    script.write_text(json.dumps([{"tool_calls": [{"name": "activate_skill", "arguments": {"name": "payments-skill"}},
                                                  {"name": "activate_skill", "arguments": {"name": "pаyments-skill"}}]},
                                  {"text": "done"}]))
    stub = subprocess.Popen([sys.executable, "-I", str(HERE / "model_stub.py"), str(PORT_STUB + 1), "script", str(script)],
                            env={**os.environ, "MODEL_STUB_LOG": str(log)}, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1)
    try:
        with Agent("skills-scopes", C_MODEL_URL=f"http://127.0.0.1:{PORT_STUB + 1}/v1", C_TOOLS="inventory", C_SKILLS_OPERATOR=op,
                   C_SKILLS_PROJECT=proj, C_PORT_RPC=0, C_PORT_REST=-1, C_PORT_GRPC=-1) as agent:
            out = agent.stdout()
            print("\n".join(l[:300] for l in out.splitlines() if l.startswith(("SKILL", "READY"))), flush=True)
            r = rpc(agent.ports["rpc"], "SendMessage", {"message": message("go")}, timeout=60)[1]
            task = task_of(r.get("result", {}))
            trace = artifact_text(task, "tool-trace") or ""
            reqs = [json.loads(l) for l in log.read_text().splitlines() if l.strip()]
            system = next((m["content"] for m in reqs[0]["body"]["messages"] if m["role"] == "system"), "") if reqs else ""
            names = [t["function"]["parameters"]["properties"]["name"].get("enum") for t in reqs[0]["body"].get("tools", [])
                     if t["function"]["name"] == "activate_skill"] if reqs else []
            print(f"   activate_skill enum: {names}", flush=True)
            print(f"   system prompt catalog:\n{system[system.find('<'):][:1500]}", flush=True)
            print(f"   trace: {trace[:600]!r}", flush=True)
            save("02-skills-scopes.json", {"header": header, "stdout": out, "enum": names, "system": system, "trace": trace})
            enum = names[0] if names else []
            check("payments-skill" in enum and sum(1 for n in enum if n.isascii()) == 1,
                  f"a project's skill with the operator's exact name is dropped (enum {enum})")
            check(len(enum) == 1, f"a project's look-alike name (Cyrillic a, non-breaking hyphen) is not a second skill (enum has {len(enum)}: {enum})")
    finally:
        stub.terminate()
    return finish()


if __name__ == "__main__":
    mode = sys.argv[1] if len(sys.argv) > 1 else "scripted"
    sys.exit({"scripted": scripted, "scopes": scopes}.get(mode, lambda: live(os.environ.get("CAMPAIGN_MODEL_FILE", "unknown")))())
