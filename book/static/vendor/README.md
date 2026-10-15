<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Vendored third-party assets

These files are checked into the repository so the documentation site can be
served without any cross-origin requests. Corporate egress proxies and
Pi-hole-style DNS filters frequently block `cdn.jsdelivr.net`,
`fonts.googleapis.com`, and similar CDNs — fetching them at runtime broke
the benchmark dashboard for enterprise visitors.

The benchmark dashboard (`benches/dashboard/template.html`, rendered to
`book/src/reference/benchmark-dashboard.html`) loads Chart.js from here and its
fonts from `book/static/fonts/`. Until 2026-10-15 it still loaded both from
jsDelivr and Google Fonts, and this file was copied into the site but used by
nothing. `docs.yml` copies `book/static/` into the built site and checks this
file is there.

## Contents

| File                | Upstream                                               | Version | SHA-256 | License |
|---------------------|--------------------------------------------------------|---------|---------|---------|
| `chart.umd.min.js`  | <https://github.com/chartjs/Chart.js>                  | 4.5.1   | `48444a82d4edcb5bec0f1965faacdde18d9c17db3063d042abada2f705c9f54a` | MIT     |

## Updating

Name the exact version, so the file is reproducible and its hash can be
recorded:

```bash
curl -fsSL https://cdn.jsdelivr.net/npm/chart.js@4.5.1/dist/chart.umd.min.js \
  -o book/static/vendor/chart.umd.min.js
sha256sum book/static/vendor/chart.umd.min.js
```

Then update the version and hash above. The dashboard references the file by
path only, so a new version needs no change to the template unless Chart.js
changed its API.
