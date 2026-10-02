# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
import sys, json
for l in open(sys.argv[1]):
    d = json.loads(l)
    print("%-29s %-34s %-9s %s" % (d["server"], d["check"], d["verdict"], d["outcome"]))
