# 部署验收与恢复演练

## 当前结论

- 本机已完成仿生产部署验收
- 真实集群 `kubectl apply` 验收尚未执行
- 对象存储备份模板、恢复 Job 模板、本地恢复演练脚本已经补齐
- Ingress / TLS / ExternalDNS 占位已补齐，真实外部 LB 绑定仍需在集群侧完成

## 已完成的文件

- 主清单: [`deploy/k8s/exchange.yaml`](deploy/k8s/exchange.yaml)
- 恢复 Job: [`deploy/k8s/exchange-restore-job.yaml`](deploy/k8s/exchange-restore-job.yaml)
- 本地恢复演练脚本: [`scripts/run_wal_restore_drill.ps1`](scripts/run_wal_restore_drill.ps1)

## 真集群最小 apply 验收

1. 替换真实参数
   `ghcr.io/OWNER/rust-exchange:latest`
   `exchange.example.com`
   `exchange-tls`
   `BACKUP_S3_BUCKET`
   `RESTORE_S3_URI`

2. 注入真实 Secret
   `internal_auth.secret`
   `AWS_ACCESS_KEY_ID`
   `AWS_SECRET_ACCESS_KEY`
   `ONCALL_WEBHOOK_URL`

3. apply 主清单

```bash
kubectl apply -f deploy/k8s/exchange.yaml
```

4. 检查 rollout

```bash
kubectl get pods -n exchange
kubectl describe pod -n exchange -l app=exchange
kubectl get ingress -n exchange
```

5. 检查服务面

- `GET /health`
- `GET /ready`
- `GET /metrics/prometheus`
- admin 鉴权 `GET /openapi.json`

6. 做一笔真实交易

- `POST /deposit`
- `POST /position-deposit`
- `POST /submit-order` 卖单
- `POST /submit-order` 买单

7. 检查关键指标

- `exchange_submit_order_ip_rate_limited_total`
- `exchange_http_requests_total`
- `wal_errors_total`

## 对象存储恢复演练

### 集群内恢复

```bash
kubectl apply -f deploy/k8s/exchange-restore-job.yaml
kubectl logs -n exchange job/exchange-wal-restore
```

要求：

- 能从 `RESTORE_S3_URI` 拉下归档
- 能成功解压
- 能看到恢复出的 WAL 文件列表

### 本地恢复演练

```powershell
.\scripts\run_wal_restore_drill.ps1 -BackupArchive .\exchange-wal-20260411T120000Z.tgz -CleanRestoreDir
```

产物：

- `restore_drill_report.json`

## Ingress / TLS / 外部 LB

主清单里已经具备：

- `Ingress`
- `cert-manager.io/cluster-issuer`
- `external-dns.alpha.kubernetes.io/hostname`

还需要在真实环境完成：

- DNS 记录指向 ingress controller / external LB
- `exchange.example.com` 替换成真实域名
- `exchange-tls` 由 cert-manager 或已有证书管理系统实际签发
- 云厂商 LB 安全组 / 防火墙放行 `80/443`

## 本次仿生产验收结果

参考报告：

- [`deployment_acceptance_report.json`](artifacts/deployment-acceptance/20260411-143121/reports/deployment_acceptance_report.json)

结果摘要：

- `health = 200`
- `ready = 200`
- `metrics/prometheus = 200`
- admin 鉴权 `openapi.json = 200`
- maker 下单 `200`
- taker 下单 `200`
- taker fills = `2`
