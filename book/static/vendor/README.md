<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Vendored third-party assets

These files are checked into the repository so the documentation site can be
served without any cross-origin requests. Corporate egress proxies and
Pi-hole-style DNS filters frequently block `cdn.jsdelivr.net`,
`fonts.googleapis.com`, and similar CDNs — fetching them at runtime broke
the benchmark dashboard for enterprise visitors. Vendoring sidesteps the
problem entirely.

## Contents

| File                | Upstream                                               | Version | License |
|---------------------|--------------------------------------------------------|---------|---------|
| `chart.umd.min.js`  | <https://github.com/chartjs/Chart.js>                  | 4.5.1   | MIT     |

## Updating

```bash
curl -fsSL https://cdn.jsdelivr.net/npm/chart.js@4/dist/chart.umd.min.js \
  -o book/static/vendor/chart.umd.min.js
```

When bumping to a new major, also update the version column above and the
build banner in `book/src/reference/benchmark-dashboard.html`.
