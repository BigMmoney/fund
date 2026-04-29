# Kubernetes Layout

This directory is organized for repeatable deployment with Kustomize.

## Structure

- `base/`
  - core app deployment, service, PVC, service account, probes, and Prometheus scrape annotations
- `observability/`
  - optional Prometheus Operator resources such as `ServiceMonitor`
- `overlays/docker-desktop/`
  - local cluster overlay with a development secret and `NodePort` service
- `benchmarks/base/`
  - staircase benchmark PVC and Job definitions
- `benchmarks/overlays/docker-desktop/`
  - local cluster overlay for the benchmark runner image

## Typical Local Workflow

1. Build and import local images:

```powershell
.\scripts\prepare_k8s_local_images.ps1
```

2. Deploy the exchange app:

```powershell
kubectl apply -k .\deploy\k8s\overlays\docker-desktop
kubectl rollout status deploy/exchange -n exchange
```

3. Optionally apply observability resources if the cluster has Prometheus Operator CRDs:

```powershell
kubectl apply -k .\deploy\k8s\observability
```

4. Run staircase benchmarks as a Job:

```powershell
kubectl apply -k .\deploy\k8s\benchmarks\overlays\docker-desktop
kubectl wait --for=condition=complete job/exchange-staircase-benchmark -n exchange --timeout=7200s
kubectl logs job/exchange-staircase-benchmark -n exchange
```

The benchmark Job always writes `/artifacts/summary.json` inside the `exchange-benchmark-artifacts` PVC and also prints the same JSON to stdout.
