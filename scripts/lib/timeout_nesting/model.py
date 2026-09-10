# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""`Model`: the scrubbed tree, its sites and helpers, composed from the
resolution, discovery and pairing mixins so each stage lives in its own file
while sharing one set of collections."""

from __future__ import annotations

from .entities import Fn, Site, SourceFile
from .pairs import PairsMixin
from .push import PushMixin
from .resolve import ResolveMixin
from .sites import DiscoverMixin


class Model(ResolveMixin, DiscoverMixin, PairsMixin, PushMixin):
    def __init__(self, files: list[SourceFile]):
        self.files = files
        self.by_crate: dict[str, list[SourceFile]] = {}
        for f in files:
            self.by_crate.setdefault(f.crate, []).append(f)
        self.sites: list[Site] = []
        self.helpers: dict[str, tuple[SourceFile, Fn, int]] = {}  # name -> (file, fn, arg index)
