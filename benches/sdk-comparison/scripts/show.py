# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
import sys, json
with open(sys.argv[1]) as f:
    for l in f:
        d = json.loads(l)
        print("%-29s %-34s %-9s %s" % (d["server"], d["check"], d["verdict"], d["outcome"]))
