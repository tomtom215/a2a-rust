# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""The fourth pairing, push:

  push       `HttpPushSender`'s worst-case schedule — computed from the same
             constants and `PushRetryPolicy` default the code uses — against
             `HandlerLimits::push_delivery_timeout`. At the shipped defaults
             they contradict (98 s inside 5 s), and the recorded reason is
             *code*, not prose: the sender must report the schedule
             (`max_delivery_duration`) and the deliverer must compare it to
             the bound (`TIMEOUT_TRUNCATED`). Either going missing is the
             contradiction reintroduced, and this pairing takes no allowlist
             entry, because "documented" is what that pair had before and it
             was not enough.

This pairing is hand-modelled precisely because the enclosing `timeout` and
the enclosed retry loop are two functions apart, which is the first blind
spot the package docstring records: any other such pair is invisible until
someone adds it here.
"""

from __future__ import annotations

import re

from . import LIMITS_RS, SENDER_RS
from .entities import Bound, Finding, Site, SourceFile
from .scrub import matching_brace


class PushMixin:
    """The push pairing of `model.Model`; expects `self.files`, `self.by_crate`,
    `self.sites` and the `resolve` methods."""

    files: list[SourceFile]
    by_crate: dict[str, list[SourceFile]]
    sites: list[Site]

    def push_pair(self) -> tuple[list[Finding], list[str]]:
        notes: list[str] = []
        by_rel = {f.rel: f for f in self.files}
        sender, limits = by_rel.get(SENDER_RS), by_rel.get(LIMITS_RS)
        if sender is None or limits is None:
            return [Finding("push", SENDER_RS, "-", "HttpPushSender", "push_delivery_timeout", [], "sender.rs or limits.rs not found; the push pairing cannot be modelled", [])], notes

        def const(name: str) -> Bound:
            return self.resolve_const(name, sender, 0)

        req, dns = const("DEFAULT_PUSH_REQUEST_TIMEOUT"), const("DEFAULT_DNS_LOOKUP_TIMEOUT")
        m = re.search(r"\bimpl\s+Default\s+for\s+PushRetryPolicy\s*\{", sender.code)
        attempts, backoff = None, None
        if m:
            body = sender.code[m.end() : matching_brace(sender.code, m.end() - 1)]
            a = re.search(r"\bmax_attempts\s*:\s*(\d+)", body)
            b = re.search(r"\bbackoff\s*:\s*vec!\[([^\]]*)\]", body)
            attempts = int(a.group(1)) if a else None
            if b:
                backoff = [self.resolve(x, sender, None, 0).secs for x in b.group(1).split(",") if x.strip()]
        outer = self.resolve_knob("push_delivery_timeout", "HandlerLimits::push_delivery_timeout", limits, 0)
        if req.secs is None or dns.secs is None or attempts is None or backoff is None or None in backoff or outer.secs is None:
            return [Finding("push", SENDER_RS, "-", "HttpPushSender", "push_delivery_timeout", [], "could not read the sender's constants, PushRetryPolicy's default, or HandlerLimits' default — the model no longer matches the code; update it", [f"request={req.show()} dns={dns.show()} attempts={attempts} backoff={backoff} outer={outer.show()}"])], notes
        sched = dns.secs + req.secs * attempts
        waits = [backoff[i] if i < len(backoff) else backoff[-1] for i in range(attempts - 1)] if backoff else []
        sched += sum(waits)
        notes.append(
            f"push     HttpPushSender::send worst case = dns {dns.secs:g}s + {attempts} x request {req.secs:g}s + backoff {'+'.join(f'{w:g}' for w in waits) or '0'}s = {sched:g}s;"
            f" enclosed by HandlerLimits::push_delivery_timeout default {outer.secs:g}s"
        )
        if sched <= outer.secs:
            notes.append("push     fits: no justification needed")
            return [], notes
        # The contradiction stands. The recorded reason must be code.
        problems: list[str] = []
        m = re.search(r"\bimpl\s+PushSender\s+for\s+HttpPushSender\s*\{", sender.code)
        reports = bool(m) and re.search(r"\bfn\s+max_delivery_duration\s*\(", sender.code[m.end() : matching_brace(sender.code, m.end() - 1)]) is not None
        if not reports:
            problems.append(f"{SENDER_RS}: `impl PushSender for HttpPushSender` does not override `max_delivery_duration`, so the schedule is not reported")
        compared = None
        for f in self.by_crate[sender.crate]:
            if not any(s.rel == f.rel and s.kind == "timeout" and s.bound.name == "push_delivery_timeout" for s in self.sites):
                continue
            c = re.search(r"max_delivery_duration\(\)[^;]{0,200}\bpush_delivery_timeout\b", f.code)
            if c:
                compared = f"{f.rel}:{f.line_of(c.start())}"
        if compared is None:
            problems.append("no file that bounds `send` with `push_delivery_timeout` compares `max_delivery_duration()` against it, so a truncated schedule is reported as a slow webhook")
        detail = [
            f"inner {SENDER_RS}  HttpPushSender schedule = {sched:g}s (dns {dns.secs:g}s + {attempts} x {req.secs:g}s + backoff {sum(waits):g}s)",
            f"outer {LIMITS_RS}  HandlerLimits::push_delivery_timeout default = {outer.secs:g}s",
        ] + problems
        if problems:
            return [Finding("push", SENDER_RS, "HttpPushSender::send", "HttpPushSender schedule", "push_delivery_timeout", [], f"push_delivery_timeout / HttpPushSender contradiction ({sched:g}s inside {outer.secs:g}s) with the truncation no longer reported", detail)], notes
        notes.append(f"push     justified in code: the sender reports max_delivery_duration and {compared} compares it to push_delivery_timeout (TIMEOUT_TRUNCATED)")
        return [], notes
