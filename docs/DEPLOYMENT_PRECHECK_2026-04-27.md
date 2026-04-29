# Deployment Precheck 2026-04-27

## Scope

Based on the current `rust-exchange` workspace and the project status guidance in `docs/PROJECT_STATUS_FOR_AI.md`, this precheck focused on:

- workspace test readiness
- WAL/recovery readiness
- deployment/configuration completeness
- local smoke-level acceptance signals

This report does **not** claim real production-cluster acceptance. However, local Docker and Kubernetes validation on Docker Desktop has now been completed to a much deeper level than the original pass.

## Commands Run

Executed on `2026-04-27`:

- `cargo test --workspace`
- `powershell -ExecutionPolicy Bypass -File .\scripts\run_backend_recovery_checks.ps1`
- `powershell -ExecutionPolicy Bypass -File .\scripts\run_backend_resilience_benchmarks.ps1 -Smoke`
- `powershell -ExecutionPolicy Bypass -File .\scripts\run_backend_resilience_benchmarks.ps1 -Smoke -HttpBenchClient powershell`
- `docker compose config`
- `docker build -t rust-exchange-precheck:20260427 .`
- `docker build --pull=false -t rust-exchange-precheck:20260427 .`
- `kubectl apply --dry-run=client -f .\deploy\k8s\exchange.yaml`
- `kubectl apply --dry-run=client -f .\deploy\k8s\exchange-restore-job.yaml`
- `kubectl cluster-info`
- `kubectl get nodes -o wide`

Static inspection covered:

- `rust-exchange/config/exchange.toml`
- `rust-exchange/Dockerfile`
- `rust-exchange/docker-compose.yml`
- `rust-exchange/deploy/k8s/exchange.yaml`
- `rust-exchange/deploy/k8s/exchange-restore-job.yaml`
- `rust-exchange/scripts/run_wal_restore_drill.ps1`
- `rust-exchange/deploy/grafana/exchange-dashboard.json`

## Result Summary

### Passed

- `cargo test --workspace` passed on the current branch.
- Recovery-focused checks passed via `scripts/run_backend_recovery_checks.ps1`.
- `docker compose config` passed on the current machine.
- Docker Desktop Kubernetes was enabled successfully.
- Local Kubernetes context was created successfully:
  - `current-context = docker-desktop`
  - local control plane reachable
  - node `desktop-control-plane` is `Ready`
- `kubectl cluster-info` passed.
- `kubectl get nodes -o wide` passed.
- `kubectl apply --dry-run=client -f .\deploy\k8s\exchange-restore-job.yaml` passed.
- Docker image build passed after pre-pulling the base images:
  - `docker build --pull=false -t rust-exchange-precheck:20260427 .`
- Required deployment artifacts exist:
  - Dockerfile
  - docker-compose
  - k8s deployment manifest
  - restore job manifest
  - WAL restore drill script
  - Grafana dashboard
- Prior local deployment acceptance evidence exists in:
  - `rust-exchange/DEPLOYMENT_ACCEPTANCE_ZH_2026-04-11.md`
  - `rust-exchange/artifacts/deployment-acceptance/20260411-143121/reports/deployment_acceptance_report.json`

### Blocked Or Failed

- Smoke resilience benchmark did not complete successfully on the current branch.
  - Default Go client path failed with `go benchmark returned empty stdout`.
  - PowerShell client path reached the API, but the script later crashed with `Cannot index into a null array`.
- `kubectl apply --dry-run=client -f .\deploy\k8s\exchange.yaml` is almost fully green, but fails on one resource:
  - `ServiceMonitor` with apiVersion `monitoring.coreos.com/v1`
  - local Docker Desktop cluster does not have the Prometheus Operator CRD installed
  - all other resources in the manifest dry-run successfully

## Important Findings

### 1. Current branch is test-green, but smoke acceptance is not green

The codebase is in a much better place than an early prototype:

- full workspace tests passed
- matching recovery/fault matrix tests passed
- latency smoke tests passed

But a deploy precheck should still be treated as **not fully passed**, because the local smoke acceptance harness is not currently producing a clean green run.

### 1.5 Docker packaging is now proven locally

The compose file now resolves correctly on this machine, and the image build completes successfully once the base images are available locally.

That confirms:

- the Compose definition is structurally usable
- the Dockerfile is buildable on this branch

### 2. Backend resilience smoke path is brittle

Two independent harness issues appeared:

- Go benchmark client path returned empty stdout.
- PowerShell smoke path later assumed `error_categories` was present and crashed when it was null.

This means the benchmark/acceptance tooling itself is not yet reliable enough to be used as a release gate in current form.

### 3. Local smoke logs show likely error-mapping noise during request failures

The latest smoke run log:

- `rust-exchange/artifacts/backend-resilience/20260427-014911/logs/api.stdout.log`

shows repeated entries like:

- underlying `429 rate limit exceeded`
- underlying `400 insufficient funds`
- but logged as `status: 500` internal error at the warp trace layer

This may be only a logging/rejection-wrapping problem, or it may indicate the API is surfacing business/rate-limit rejections incorrectly during benchmark paths. That should be verified before beta deployment sign-off.

### 4. Smoke run used fallback role trust

The same smoke log shows:

- `No role mapping file at data/role_mapping.json. Client-provided roles will be trusted (not recommended for production).`

This is acceptable for a local benchmark harness, but it is not acceptable as a production-like deployment posture. Secret-file and role-mapping-file startup must remain part of the release gate.

### 5. Kubernetes tooling and local cluster are now working

The missing piece is no longer the `kubectl` binary or local context.

The remaining blocker is now narrower:

- `ServiceMonitor` depends on Prometheus Operator CRDs
- those CRDs are not present in the local Docker Desktop cluster

## Readiness Judgment

### P0 status

Partially complete.

What is green now:

- code compiles and tests
- recovery-oriented Rust tests
- deployment manifests and restore assets exist
- prior local acceptance artifact exists

What is still missing before calling this branch deployment-ready for `v1 beta`:

- a clean current-branch smoke acceptance run
- validation of the `ServiceMonitor` resource in a cluster that has Prometheus Operator CRDs
- explicit verification that secret-file and role-mapping-file startup path is enforced in the release checklist
- confirmation whether `429/400` paths are being logged or surfaced incorrectly as `500`

## Recommended Next Actions

1. Fix `scripts/run_backend_resilience_benchmarks.ps1` so both Go and PowerShell smoke paths complete reliably.
2. Verify API error mapping for rate-limit and insufficient-funds scenarios, using the latest smoke log as the repro hint.
3. Re-run smoke acceptance and save a fresh JSON report on the current branch.
4. Validate `ServiceMonitor` in a cluster with Prometheus Operator installed, or gate that manifest by environment.
5. Re-run rollout rehearsal on the now-working `docker-desktop` context if you want a full local apply test.
6. Re-check startup in production-like mode with:
   - `INTERNAL_AUTH_SHARED_SECRET_FILE`
   - `SERVER_ROLE_MAPPING_FILE`
   - WAL data volume mounted

## Bottom Line

The trading core and recovery tests are in good shape, but the deployment gate is **not fully closed yet**.

The next best move is not adding features. It is:

- stabilize the smoke acceptance harness
- verify error mapping
- decide how to handle the `ServiceMonitor` CRD dependency

Only after that should this branch be considered ready to advance toward `closed beta`.
