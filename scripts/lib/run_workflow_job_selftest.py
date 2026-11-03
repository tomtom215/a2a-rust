# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Self-test of scripts/run_workflow_job.py.

Each case writes a tiny workflow, runs the runner as a subprocess (the real
command line, not its functions), and checks the exit code, the lines it prints,
and, where it matters, whether a step ran at all (a marker file). The cases that
matter most are the ones where the wrong answer is a false PASS: a failed step,
a step the runner could not evaluate, an action it cannot run, a refused command
that must not have started anything.
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
import textwrap
from pathlib import Path


def main(runner: Path) -> int:
    tmp = Path(tempfile.mkdtemp(prefix="wfjob-selftest-"))
    results: list[tuple[str, bool, str]] = []

    def case(name: str, jobs: str, args: list[str] | None = None, *, code: int,
             has: tuple[str, ...] = (), lacks: tuple[str, ...] = (),
             ran: tuple[str, ...] = (), not_ran: tuple[str, ...] = (), top: str = "") -> None:
        mark = tmp / f"mark-{len(results)}"
        mark.mkdir()
        body = f"name: t\non: push\n{top}\njobs:\n" + textwrap.indent(textwrap.dedent(jobs).replace("@MARK@", str(mark)), "  ")
        wf = tmp / f"wf-{len(results)}.yml"
        wf.write_text(body, encoding="utf-8")
        p = subprocess.run([sys.executable, "-I", str(runner), *(args or []), str(wf), "j"],
                           capture_output=True, text=True)
        out = p.stdout + p.stderr
        problems = []
        if p.returncode != code:
            problems.append(f"exit {p.returncode}, wanted {code}")
        problems += [f"output lacks {s!r}" for s in has if s not in out]
        problems += [f"output has {s!r}" for s in lacks if s in out]
        problems += [f"{f} was not created" for f in ran if not (mark / f).exists()]
        problems += [f"{f} was created" for f in not_ran if (mark / f).exists()]
        pidf = mark / "pid"
        if pidf.exists():
            pid = int(pidf.read_text())
            try:
                # A killed process whose parent is gone can stay in the table as a
                # zombie until init reaps it; that is dead, so read its state.
                state = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[0]
            except FileNotFoundError:
                state = "gone"
            if state not in ("Z", "gone"):
                problems.append(f"process {pid} started by a step was still running after the job (state {state})")
        results.append((name, not problems, "; ".join(problems) + (f"\n{out}" if problems else "")))

    case("passing steps", """
        j:
          runs-on: ubuntu-latest
          steps:
            - run: touch @MARK@/a
            - run: touch @MARK@/b
        """, code=0, has=("PASS",), ran=("a", "b"))
    case("a failing step fails the job, shows its own exit code, and stops later steps", """
        j:
          runs-on: ubuntu-latest
          steps:
            - name: boom
              run: exit 7
            - run: touch @MARK@/later
        """, code=1, has=("FAIL", "exit   7"), not_ran=("later",))
    case("if: always() runs after a failure", """
        j:
          runs-on: ubuntu-latest
          steps:
            - run: exit 1
            - if: always()
              run: touch @MARK@/cleanup
        """, code=1, has=("FAIL",), ran=("cleanup",))
    case("if: failure() does not run when everything passed", """
        j:
          runs-on: ubuntu-latest
          steps:
            - run: "true"
            - if: failure()
              run: touch @MARK@/only-on-failure
        """, code=0, has=("PASS",), not_ran=("only-on-failure",))
    case("continue-on-error: the job passes, the exit code is still printed", """
        j:
          runs-on: ubuntu-latest
          steps:
            - name: allowed
              continue-on-error: true
              run: exit 5
            - run: touch @MARK@/after
        """, code=0, has=("PASS", "exit   5", "failed-allowed", "failed-under-continue-on-error=1"), ran=("after",))
    case("an action it cannot run makes the job INCOMPLETE, never PASS", """
        j:
          runs-on: ubuntu-latest
          steps:
            - uses: some/action@v1
            - run: touch @MARK@/a
        """, code=3, has=("INCOMPLETE", "some/action: cannot run an action here"), lacks=("PASS",), ran=("a",))
    case("runner set-up and plumbing actions are skipped without a verdict", """
        j:
          runs-on: ubuntu-latest
          steps:
            - uses: actions/checkout@abc
            - uses: actions/upload-artifact@abc
            - run: "true"
        """, code=0, has=("PASS",))
    case("sudo is refused before anything runs", """
        j:
          runs-on: ubuntu-latest
          steps:
            - run: touch @MARK@/first
            - run: sudo true
        """, code=2, has=("runs `sudo`",), not_ran=("first",))
    case("git push is refused before anything runs", """
        j:
          runs-on: ubuntu-latest
          steps:
            - run: touch @MARK@/first
            - run: git push origin HEAD
        """, code=2, has=("runs `git push`",), not_ran=("first",))
    case("a step if: it cannot evaluate is not run and the job is INCOMPLETE", """
        j:
          runs-on: ubuntu-latest
          steps:
            - if: steps.diff.outputs.skip == 'false'
              run: touch @MARK@/guarded
        """, code=3, has=("INCOMPLETE", "cannot be evaluated"), not_ran=("guarded",))
    case("a ${{ secrets }} reference is not run and the job is INCOMPLETE", """
        j:
          runs-on: ubuntu-latest
          steps:
            - run: touch @MARK@/leak; echo ${{ secrets.TOKEN }}
        """, code=3, has=("INCOMPLETE", "secrets.TOKEN"), not_ran=("leak",))
    jobs_matrix = """
        j:
          runs-on: ubuntu-latest
          strategy:
            matrix:
              v: [a, b]
          steps:
            - run: test "${{ matrix.v }}" = b && touch @MARK@/b
        """
    case("a matrix axis with several values and no --matrix is refused", jobs_matrix, code=2, has=("pass --matrix v=",))
    case("--matrix substitutes into the step", jobs_matrix, ["--matrix", "v=b"], code=0, has=("PASS",), ran=("b",))
    case("--matrix with a wrong value runs and fails", jobs_matrix, ["--matrix", "v=a"], code=1, has=("FAIL",))
    case("step env beats job env beats workflow env", """
        j:
          runs-on: ubuntu-latest
          env:
            X: job
            Z: job
          steps:
            - env:
                X: step
              run: test "$X" = step && test "$Z" = job && test "$W" = workflow && touch @MARK@/ok
        """, code=0, ran=("ok",), top="env:\n  W: workflow\n  Z: workflow")
    case("GITHUB_ENV carries a value to the next step", """
        j:
          runs-on: ubuntu-latest
          steps:
            - run: echo "Y=1" >> "$GITHUB_ENV"
            - run: test "$Y" = 1 && touch @MARK@/ok
        """, code=0, ran=("ok",))
    case("the default shell has no pipefail", """
        j:
          runs-on: ubuntu-latest
          steps:
            - run: "false | true"
        """, code=0, has=("PASS",))
    case("shell: bash has pipefail", """
        j:
          runs-on: ubuntu-latest
          steps:
            - shell: bash
              run: "false | true"
        """, code=1, has=("FAIL",))
    case("working-directory is honoured", """
        j:
          runs-on: ubuntu-latest
          steps:
            - working-directory: scripts
              run: test "$(basename "$PWD")" = scripts && touch @MARK@/ok
        """, code=0, ran=("ok",))
    jobs_expr = """
        j:
          runs-on: ubuntu-latest
          env:
            BASE: ${{ github.event.pull_request.base.sha }}
          steps:
            - run: test "$BASE" = abc123 && touch @MARK@/ok
        """
    case("an expression with no answer makes the job INCOMPLETE and the step is not run", jobs_expr, code=3,
         has=("INCOMPLETE", "github.event.pull_request.base.sha"), not_ran=("ok",))
    case("--expr supplies the expression's value", jobs_expr,
         ["--expr", "github.event.pull_request.base.sha=abc123"], code=0, has=("PASS",), ran=("ok",))
    jobs_or = """
        j:
          runs-on: ubuntu-latest
          steps:
            - env:
                B: ${{ github.event.pull_request.base.sha || github.event.before }}
              run: test "$B" = fromsecond && touch @MARK@/ok
        """
    case("a || expression takes the first operand with an answer", jobs_or,
         ["--expr", "github.event.before=fromsecond"], code=0, has=("PASS",), ran=("ok",))
    case("a || expression prefers the first operand", jobs_or,
         ["--expr", "github.event.pull_request.base.sha=first", "--expr", "github.event.before=fromsecond"],
         code=1, has=("FAIL",), not_ran=("ok",))
    case("a || expression with no answered operand is INCOMPLETE", jobs_or, code=3,
         has=("INCOMPLETE", "github.event.before"), not_ran=("ok",))
    case("a server a step leaves running is stopped when the job ends", """
        j:
          runs-on: ubuntu-latest
          steps:
            - run: |
                sleep 300 &
                echo $! > @MARK@/pid
            - run: kill -0 "$(cat @MARK@/pid)" && touch @MARK@/alive-for-next-step
        """, code=0, has=("PASS",), ran=("alive-for-next-step",))
    case("a server that ignores SIGTERM is killed when the job ends", """
        j:
          runs-on: ubuntu-latest
          steps:
            - run: |
                bash -c 'trap "" TERM; exec sleep 300' &
                echo $! > @MARK@/pid
        """, code=0, has=("PASS",))
    case("a timed-out step's children die at the timeout, not at the end of the job", """
        j:
          runs-on: ubuntu-latest
          steps:
            - timeout-minutes: 0.02
              run: |
                (sleep 2; touch @MARK@/late) &
                sleep 20
            - if: always()
              run: sleep 3
        """, code=1, has=("FAIL", "exit 124"), not_ran=("late",))
    steps_outcome = """
        j:
          runs-on: ubuntu-latest
          steps:
            - id: first
              run: {first}
            - if: {cond}
              run: touch @MARK@/second
        """
    def outcome_case(name, first, cond, *, code, ran=False, has=()):
        case(name, steps_outcome.replace("{first}", first).replace("{cond}", cond), code=code,
             has=has, ran=("second",) if ran else (), not_ran=() if ran else ("second",))
    outcome_case("steps.<id>.outcome == 'success' runs after a passing step", '"true"',
                 "steps.first.outcome == 'success'", code=0, ran=True, has=("PASS",))
    outcome_case("steps.<id>.outcome == 'failure' does not run after a passing step", '"true"',
                 "steps.first.outcome == 'failure'", code=0, has=("PASS", "skipped"))
    outcome_case("!cancelled() && steps.<id>.outcome == 'success' runs", '"true"',
                 "${{ !cancelled() && steps.first.outcome == 'success' }}", code=0, ran=True, has=("PASS",))
    outcome_case("a condition naming a step id that does not exist is INCOMPLETE", '"true"',
                 "steps.nope.outcome == 'success'", code=3, has=("INCOMPLETE", "cannot be evaluated"))
    outcome_case("a failed step's outcome is 'failure' (always() lets the next step read it)", "exit 1",
                 "always() && steps.first.outcome == 'failure'", code=1, ran=True, has=("FAIL",))
    case("continue-on-error: outcome is 'failure' and conclusion is 'success'", """
        j:
          runs-on: ubuntu-latest
          steps:
            - id: a
              continue-on-error: true
              run: exit 1
            - if: steps.a.outcome == 'failure' && steps.a.conclusion == 'success'
              run: touch @MARK@/both
        """, code=0, has=("PASS",), ran=("both",))
    case("a condition with no status function does not run after a failure (implicit success())", """
        j:
          runs-on: ubuntu-latest
          steps:
            - run: exit 1
            - if: github.event_name == 'pull_request'
              run: touch @MARK@/ran
        """, code=1, has=("FAIL",), not_ran=("ran",))
    case("a set-up action with an id counts as succeeded", """
        j:
          runs-on: ubuntu-latest
          steps:
            - id: co
              uses: actions/checkout@abc
            - if: steps.co.outcome == 'success'
              run: touch @MARK@/after-checkout
        """, code=0, has=("PASS",), ran=("after-checkout",))
    case("an action it could not run has no outcome, so a step reading it is not run", """
        j:
          runs-on: ubuntu-latest
          steps:
            - id: x
              uses: some/action@v1
            - if: steps.x.outcome == 'success'
              run: touch @MARK@/ran
        """, code=3, has=("INCOMPLETE",), not_ran=("ran",))
    jobs_services = """
        j:
          runs-on: ubuntu-latest
          services:
            db:
              image: postgres
          steps:
            - run: touch @MARK@/a
        """
    case("services not started: INCOMPLETE and nothing ran", jobs_services, code=3, has=("INCOMPLETE", "services"), not_ran=("a",))
    case("--services-ready runs the steps", jobs_services, ["--services-ready"], code=0, has=("PASS",), ran=("a",))
    case("a non-Linux runner is INCOMPLETE and nothing ran", """
        j:
          runs-on: macos-latest
          steps:
            - run: touch @MARK@/a
        """, code=3, has=("INCOMPLETE", "not a Linux runner"), not_ran=("a",))
    jobs_if = """
        j:
          runs-on: ubuntu-latest
          if: github.event_name == 'push'
          steps:
            - run: touch @MARK@/a
        """
    case("a job whose if: is false for the event is SKIPPED", jobs_if, code=0, has=("SKIPPED",), not_ran=("a",))
    case("--event push runs it", jobs_if, ["--event", "push"], code=0, has=("PASS",), ran=("a",))
    jobs_or_if = """
        j:
          runs-on: ubuntu-latest
          if: github.event_name == 'schedule' || github.event_name == 'workflow_dispatch'
          steps:
            - run: touch @MARK@/a
        """
    case("an || of event names is false for pull_request", jobs_or_if, code=0, has=("SKIPPED",), not_ran=("a",))
    case("an || of event names is true for its second alternative", jobs_or_if, ["--event", "workflow_dispatch"],
         code=0, has=("PASS",), ran=("a",))
    jobs_and_if = """
        j:
          runs-on: ubuntu-latest
          if: github.event_name == 'push' && github.event_name != 'push'
          steps:
            - run: touch @MARK@/a
        """
    case("an && of contradictory event names is false", jobs_and_if, ["--event", "push"], code=0, has=("SKIPPED",), not_ran=("a",))
    case("an if: with a function it cannot evaluate is INCOMPLETE", """
        j:
          runs-on: ubuntu-latest
          if: github.event_name == 'push' && hashFiles('x') != ''
          steps:
            - run: touch @MARK@/a
        """, ["--event", "push"], code=3, has=("INCOMPLETE", "cannot be evaluated"), not_ran=("a",))
    case("steps run in file order", """
        j:
          runs-on: ubuntu-latest
          steps:
            - run: printf 1 >> @MARK@/o
            - run: printf 2 >> @MARK@/o
            - run: printf 3 >> @MARK@/o && test "$(cat @MARK@/o)" = 123
        """, code=0, has=("PASS",))
    case("a step that times out fails with 124", """
        j:
          runs-on: ubuntu-latest
          steps:
            - timeout-minutes: 0.02
              run: sleep 20
        """, code=1, has=("FAIL", "exit 124"))
    case("a missing command fails with 127", """
        j:
          runs-on: ubuntu-latest
          steps:
            - run: a_command_that_does_not_exist_xyz
        """, code=1, has=("FAIL", "exit 127"))
    case("an unknown job name is refused", """
        other:
          runs-on: ubuntu-latest
          steps:
            - run: "true"
        """, code=2, has=("no job 'j'",))

    bad = [r for r in results if not r[1]]
    for name, ok, why in results:
        print(f"{'ok  ' if ok else 'FAIL'} {name}" + ("" if ok else f"\n     {why}"))
    print(f"\n{len(results) - len(bad)} of {len(results)} self-test cases pass")
    return 1 if bad else 0
