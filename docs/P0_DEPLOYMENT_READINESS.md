# P0 Deployment-Readiness Suite

Practical operator guide for running the P0 acceptance checklist defined in
`docs/PROJECT_STATUS_FOR_AI.md` §17 + §19 阶段 1.

P0 is **the gate before staging**. It does not validate liquidation, funding,
or read-side completeness — those are P1/P2. P0 only proves the v1 core can
build, pass its own tests, recover from WAL, and be operated.

---

## What's in scope

| # | Check                                  | Script / command                                  |
|---|----------------------------------------|---------------------------------------------------|
| 1 | Workspace builds                       | `cargo build --release --bin api`                 |
| 2 | Workspace tests pass                   | `cargo test --workspace`                          |
| 3 | E2E order → match → ledger             | `scripts/e2e_trading_test.ps1`                    |
| 4 | WAL recovery after dirty restart       | `scripts/test_wal_recovery.ps1`                   |
| 5 | Restart-after-business-error integrity | `scripts/test_restart_after_errors.ps1`           |
| 6 | WAL backup + restore drill             | `scripts/wal_backup.ps1` → `run_wal_restore_drill.ps1` |

A clean run produces artifacts under
`rust-exchange/artifacts/p0_run_<timestamp>/`.

---

## Prerequisites

- **MinGW gcc on PATH** — `.cargo/config.toml` pins
  `target = "x86_64-pc-windows-gnu"` with `linker = "gcc.exe"`. Plain MSVC
  toolchain alone is not enough; without `gcc.exe` the link step fails.
- **PowerShell 5.1+** for the `*.ps1` scripts.
- **Windows native bsdtar** at `%SystemRoot%\System32\tar.exe` (default on
  Windows 10/11). The scripts force this path because msys2 GNU tar misparses
  drive-letter paths like `D:\…` as `host:path`.
- **`data/internal_auth.secret`** must exist and match the secret embedded in
  the PowerShell scripts: `dev-secret-change-me-to-32-chars-min!` (36 chars).
  This is a dev-only default; production must use
  `INTERNAL_AUTH_SHARED_SECRET_FILE`.

---

## Run order

Each script either runs offline (build/test) or manages its own server
lifecycle (start / clear WAL / stop). Run them sequentially from the
`rust-exchange/` directory. **Do not run them in parallel** — they share the
`data/` directory and a single TCP port.

```powershell
# 1. Build (≈10–15 min cold, ≈1–2 min incremental)
cargo build --release --bin api 2>&1 | Tee-Object artifacts\p0_run_xxx\01_build.log

# 2. Tests (≈5–10 min)
cargo test --workspace 2>&1 | Tee-Object artifacts\p0_run_xxx\02_test.log

# 3. E2E — defaults now match config/exchange.toml (port 3030, correct secret)
.\scripts\e2e_trading_test.ps1 *>&1 | Tee-Object artifacts\p0_run_xxx\03_e2e.log

# 4. WAL recovery — manages its own server start/stop
.\scripts\test_wal_recovery.ps1 *>&1 | Tee-Object artifacts\p0_run_xxx\04_wal_recovery.log

# 5. Restart after business errors — manages its own server start/stop
.\scripts\test_restart_after_errors.ps1 *>&1 | Tee-Object artifacts\p0_run_xxx\05_restart.log

# 6. WAL backup + restore drill (round-trip)
.\scripts\wal_backup.ps1
$archive = (Get-ChildItem artifacts\wal-backups\wal-*.tar.gz |
            Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName
.\scripts\run_wal_restore_drill.ps1 -BackupArchive $archive -CleanRestoreDir `
    *>&1 | Tee-Object artifacts\p0_run_xxx\06_restore_drill.log
```

---

## Pass criteria

| Step | Pass means                                                                 |
|------|----------------------------------------------------------------------------|
| 1    | Exit 0, `target/x86_64-pc-windows-gnu/release/api.exe` exists & is fresh   |
| 2    | `test result: ok` for every crate, **0 failed**                            |
| 3    | All probe orders accepted, no 5xx, settlement metrics increment            |
| 4    | Server starts on cleared WAL, health 200, lifecycle distribution sane      |
| 5    | After error→restart, all post-restart orders return 200                    |
| 6    | `restore_drill_report.json` `restored_count == file_count` from manifest   |

---

## Known sharp edges (operator gotchas)

These are issues the P0 suite itself surfaces if you run it cold. They are
**fixed in the scripts now**, but worth knowing because they reflect real
operator-onboarding pain.

1. **Drive-letter tar handling.** `wal_backup.ps1` and
   `run_wal_restore_drill.ps1` invoke `%SystemRoot%\System32\tar.exe`
   explicitly. Do not rely on whatever `tar` happens to be on PATH — msys2 GNU
   tar will fail with "Cannot connect to D: resolve failed" on Windows paths.
2. **Silent restore-drill success bug (fixed).** The original
   `run_wal_restore_drill.ps1` did not check `$LASTEXITCODE` after `tar`,
   so a failed extraction printed "Restore drill complete" with an empty
   directory. Now it `throw`s on non-zero exit.
3. **Port mismatch (fixed).** `e2e_trading_test.ps1` defaulted to
   `http://127.0.0.1:8080`; `config/exchange.toml` defaults to `3030`. Now
   aligned. Override with `-BaseUri` if you bind elsewhere.
4. **Secret mismatch (fixed).** The dev secret in `e2e_trading_test.ps1`
   (`dev-secret-change-me`, 20 chars) did not match the actual secret in
   `data/internal_auth.secret` (`dev-secret-change-me-to-32-chars-min!`).
   Now aligned.
5. **Build target is gnu, not MSVC.** `cargo build --release` produces a
   binary at `target/x86_64-pc-windows-gnu/release/api.exe`, **not**
   `target/release/api.exe`. Anything that hard-codes the latter will not
   find the binary on this repo.
6. **No backup creation script existed.** Prior to this suite, the restore
   drill required a `.tar.gz` archive nobody had a script to produce. The new
   `wal_backup.ps1` closes that gap and pairs cleanly with the drill.

---

## Adding a new P0 check

P0 is intentionally narrow. Before adding a step, ask:

1. Does it test a property the v1 core must have to be deployable at all?
   (Not "would be nice", not "we want eventually".)
2. Does it produce an artifact a release manager can archive as evidence?
3. Can it be run unattended (no interactive prompts, no cloud creds)?

If all three are yes, add the script under `rust-exchange/scripts/`,
add the step to the run-order table above, and add a pass-criterion row.
Otherwise it belongs in P1/P2.

---

## What P0 does NOT cover

These belong to P1 or P2 and have separate scripts/checklists:

- 24h / 72h soak runs (P2)
- HA failover / multi-instance drill (P2)
- Insurance fund / bankruptcy price / liquidation auction (P2)
- Automatic funding rate generation (P2)
- Closed-beta whitelist + per-user fund caps (P1)
- Online ledger invariant monitor (P1 — currently fires only on recovery)
- Admin audit export / query tool (P1)

See `docs/PROJECT_STATUS_FOR_AI.md` §17 for the full priority ladder.
