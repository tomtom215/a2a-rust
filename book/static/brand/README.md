<!-- SPDX-License-Identifier: Apache-2.0 -->

# a2a-rust brand kit

Visual identity for **a2a-rust**. These assets are versioned here and published at
`https://a2a-rust.com/brand/`.

## Palette

| Role | Hex |
|------|-----|
| Amber (dark bg) | `#F0A93B` |
| Amber (light bg) | `#D97706` |
| Ink (tile / dark card) | `#19150F` |
| Cream | `#F4EEE0` |

## Type

- **Display / wordmark:** Space Grotesk (Bold)
- **Code / mono / URLs:** JetBrains Mono

The amber **2** in `a2a-rust` (agent-**2**-agent) is the brand's hero device.

## Social / link-preview cards — 1200×630

| File | Use |
|------|-----|
| `og-card-editorial-dark.png` | **Primary.** Wired as the site's `og-image.png`; README hero in dark mode. |
| `og-card-editorial-light.png` | Light variant; README hero in light mode. |
| `og-card-code.png` | Code-snippet card for social posts / docs. |

## Icon & favicon

| File | Use |
|------|-----|
| `avatar.png` (1024) | Profile / logo mark — upload as the GitHub org/repo avatar. |
| `favicon.svg` | Vector favicon (text outlined to paths — no font dependency). |
| `favicon.ico` | 16/32/48 multi-size. |
| `favicon-16.png`, `favicon-32.png` | Raster favicons. |
| `apple-touch-icon.png` (180, opaque) | iOS home-screen icon. |
| `icon-192.png`, `icon-512.png` | PWA / Android. |

## Sources

`sources/` holds the files the assets render from:

- `editorial.html` — light/dark card (append `#light` or `#dark`); render with headless Chrome.
- `code-card.html` — code card; render with headless Chrome.
- `avatar.svg` — avatar with live text; `avatar-outlined.svg` is the same with text converted to paths (the favicon master).

Reproducing requires the **Space Grotesk** and **JetBrains Mono** fonts installed.
