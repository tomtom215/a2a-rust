<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Tool-calling study: raw runs

The output of `run.py` (direct arm) and `sdk_arm.py` (SDK arm), as
`cell.py` assembled it, 2026-10-20 to 2026-10-22. One file per cell:
`{arm}_{model}_{thinking}.json`, 30 seeds of each of two prompts
(`billing`, `checkout`), 60 runs a cell. All 16 cells are complete. The method is in
`docs/tool-calling-study.md`; nothing here is scored yet.

These files were kept in the container's scratch space while the study
ran and were committed as it went so that a restart could not lose them.
`study.sh` resumes a study from such files: finished cells are skipped,
and `cell.py` runs only the seeds a partial cell is missing.

## State of each cell

Counts read from the files themselves (`len(runs)`) at the commit that
last changed this README; recount before relying on them.

| Cell | Runs | Harness (SHA-256, first 12) | Notes |
|---|---|---|---|
| `direct_qwen3.5-0.8b-q4_0_{off,on}` | 60, 60 | `run.py` `a503a03efca7` | made by `run.py` at `1a832495` |
| `direct_qwen3.5-0.8b-q4_k_m_{off,on}` | 60, 60 | `run.py` `a503a03efca7` | as above |
| `direct_qwen3-1.7b-q4_k_m_{off,on}` | 60, 60 | `run.py` `536670664c79` | `on` resumed by `cell.py` from 10 runs |
| `direct_qwen3.5-2b-q4_k_m_{off,on}` | 60, 60 | `run.py` `536670664c79` | `on` resumed from 9 runs |
| `sdk_qwen3.5-0.8b-q4_0_{off,on}` | 60, 60 | `sdk_arm.py` `0f3c76d439bd` | |
| `sdk_qwen3.5-0.8b-q4_k_m_{off,on}` | 60, 60 | `sdk_arm.py` `0f3c76d439bd` | |
| `sdk_qwen3-1.7b-q4_k_m_{off,on}` | 60, 60 | `sdk_arm.py` `0f3c76d439bd` | |
| `sdk_qwen3.5-2b-q4_k_m_{off,on}` | 60, 60 | `sdk_arm.py` `0f3c76d439bd` | `off` resumed from 14 |

## What a reader must know

- **Two `run.py` versions.** The four 0.8B direct cells were made by
  `run.py` as of `1a832495` (SHA-256 `a503a03e…`), the others by the version
  of `18f9e8b4` (`53667066…`). That commit named the existing 1800 s turn
  timeout as a constant and recorded it in the header
  (`turn_timeout_secs`); the request sent and its timeout are the same
  (`git show 18f9e8b4 -- scripts/tool_calling_study/run.py`).
- **One SDK binary.** Every SDK cell records the same binary,
  `01dce94e4fadfd5fd59e32c2ed0935b93d63370b6cf730b795f3bdc5816f2fc7`, a
  release build of `examples/tool-loop-agent`. Which commit it was built
  from is inferred, not verified: the file is dated 2026-10-21 06:12, a
  minute before `18f9e8b4`, which added the `TOOL_LOOP_MODEL_TIMEOUT_SECS`
  it reads. A rebuild does not settle it, since release builds here are not
  byte-reproducible. The example's tools are lookups, declared read-only
  since ADR 0026, so the loop's later idempotency replay does not change
  what that binary did.
- **Cells assembled by `cell.py`** carry an `assembly` object: the
  `cell.py` hash, how many runs the file held when it resumed
  (`resumed_with`), and, for the SDK arm, whether each seed it re-ran (a
  pair is run together) matched the stored run (`replays`). 14 of 14
  matched, all in `sdk_qwen3.5-2b-q4_k_m_off`.
- **Runs removed by hand.** On 2026-10-22 a restart script started its
  drivers before the servers had loaded. The SDK 1.7B cells it filled (60
  failed runs each, in seconds) were deleted, and the SDK 2B thinking-off
  cell was cut back to the 14 runs it held before; `cell.py` now checks
  the server's `/health` before each chunk.
- **Errors kept as data.** Runs recorded against a healthy server are kept
  whatever happened in them: `server_error` in the SDK arm, `http_error` in
  a direct run's turns. Before scoring, read them. Counted at completion:
  one `http_error` (500) in `direct_qwen3.5-0.8b-q4_0_on`, seed 2006;
  `server_error` in `sdk_qwen3.5-0.8b-q4_0_off` (seed 1020) and
  `sdk_qwen3.5-0.8b-q4_0_on` (seeds 1000, 1016, 2022); none elsewhere.
