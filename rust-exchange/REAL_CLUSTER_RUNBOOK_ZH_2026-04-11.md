# 真集群命令手册

这份手册把以下 4 件事合在一起：

- 真集群 `kubectl apply` 验收
- 真实域名 / TLS / 外部 LB 绑定
- 告警与 on-call 联动实测
- 对象存储备份与恢复演练

本文默认：

- 仓库根目录为 `rust-exchange/`
- 终端为 `PowerShell`
- 集群中已经安装：
  - `kubectl`
  - NGINX Ingress Controller
  - `cert-manager`
  - `external-dns`
  - `Prometheus Operator`（若要使用 `ServiceMonitor`）

## 1. 预设变量

先把下面这些变量替换成真实值。

```powershell
$env:KUBE_NAMESPACE       = "exchange"
$env:EXCHANGE_IMAGE       = "ghcr.io/<your-org>/rust-exchange:<tag>"
$env:EXCHANGE_HOST        = "exchange.example.com"
$env:EXCHANGE_TLS_SECRET  = "exchange-tls"

$env:BACKUP_S3_BUCKET     = "s3://<bucket>/exchange"
$env:RESTORE_S3_URI       = "s3://<bucket>/exchange/wal/<backup-file>.tgz"
$env:AWS_REGION           = "ap-southeast-1"
$env:AWS_ACCESS_KEY_ID    = "<aws-access-key-id>"
$env:AWS_SECRET_ACCESS_KEY= "<aws-secret-access-key>"

$env:ONCALL_WEBHOOK_URL   = "https://hooks.slack.com/services/xxx/yyy/zzz"
$env:ONCALL_PRIMARY_NAME  = "alice"
$env:ONCALL_PRIMARY_CONTACT = "alice@example.com"
```

## 2. 准备本地文件

在仓库内创建一份真实密钥文件和角色映射文件。

```powershell
New-Item -ItemType Directory -Path .\secrets -Force | Out-Null
New-Item -ItemType Directory -Path .\config -Force | Out-Null

[System.IO.File]::WriteAllText(
  (Resolve-Path .\secrets).Path + "\internal_auth.secret",
  "replace-with-a-random-32-byte-plus-secret",
  [System.Text.UTF8Encoding]::new($false)
)

[System.IO.File]::WriteAllText(
  (Resolve-Path .\config).Path + "\role_mapping.json",
  '{"ops-admin":"admin","ops-reader":"admin"}',
  [System.Text.UTF8Encoding]::new($false)
)
```

## 3. 生成可 apply 的主清单

把模板中的占位值替换成真实值，生成一份临时 YAML。

```powershell
$rendered = Join-Path $env:TEMP "exchange.rendered.yaml"

$raw = Get-Content .\deploy\k8s\exchange.yaml -Raw
$raw = $raw.Replace("ghcr.io/OWNER/rust-exchange:latest", $env:EXCHANGE_IMAGE)
$raw = $raw.Replace("exchange.example.com", $env:EXCHANGE_HOST)
$raw = $raw.Replace("exchange-tls", $env:EXCHANGE_TLS_SECRET)
$raw = $raw.Replace("s3://replace-me/exchange/wal/REPLACE_ME.tgz", $env:RESTORE_S3_URI)
$raw = $raw.Replace("s3://replace-me/exchange", $env:BACKUP_S3_BUCKET)

[System.IO.File]::WriteAllText($rendered, $raw, [System.Text.UTF8Encoding]::new($false))
$rendered
```

## 4. 创建或更新 Secret

这里不直接把真实值写回仓库，而是用 `kubectl create secret ... --dry-run=client -o yaml | kubectl apply -f -`。

```powershell
kubectl create namespace $env:KUBE_NAMESPACE --dry-run=client -o yaml | kubectl apply -f -

kubectl -n $env:KUBE_NAMESPACE create secret generic exchange-secrets `
  --from-file=internal_auth.secret=.\secrets\internal_auth.secret `
  --from-file=role_mapping.json=.\config\role_mapping.json `
  --from-literal=ONCALL_WEBHOOK_URL=$env:ONCALL_WEBHOOK_URL `
  --from-literal=ONCALL_PRIMARY_NAME=$env:ONCALL_PRIMARY_NAME `
  --from-literal=ONCALL_PRIMARY_CONTACT=$env:ONCALL_PRIMARY_CONTACT `
  --from-literal=BACKUP_S3_BUCKET=$env:BACKUP_S3_BUCKET `
  --from-literal=RESTORE_S3_URI=$env:RESTORE_S3_URI `
  --from-literal=AWS_REGION=$env:AWS_REGION `
  --from-literal=AWS_ACCESS_KEY_ID=$env:AWS_ACCESS_KEY_ID `
  --from-literal=AWS_SECRET_ACCESS_KEY=$env:AWS_SECRET_ACCESS_KEY `
  --dry-run=client -o yaml | kubectl apply -f -
```

如果你的 TLS 证书不是由 `cert-manager` 自动签发，而是已有现成 PEM：

```powershell
kubectl -n $env:KUBE_NAMESPACE create secret tls $env:EXCHANGE_TLS_SECRET `
  --cert=.\secrets\tls.crt `
  --key=.\secrets\tls.key `
  --dry-run=client -o yaml | kubectl apply -f -
```

## 5. apply 主清单

```powershell
kubectl apply -f $rendered
```

## 6. 等待 rollout 完成

```powershell
kubectl -n $env:KUBE_NAMESPACE rollout status deploy/exchange --timeout=180s

kubectl -n $env:KUBE_NAMESPACE get pods -o wide
kubectl -n $env:KUBE_NAMESPACE get svc exchange
kubectl -n $env:KUBE_NAMESPACE get ingress exchange
kubectl -n $env:KUBE_NAMESPACE get endpoints exchange
```

如果 rollout 失败，第一时间看：

```powershell
kubectl -n $env:KUBE_NAMESPACE describe pod -l app=exchange
kubectl -n $env:KUBE_NAMESPACE logs deploy/exchange --tail=200
kubectl -n $env:KUBE_NAMESPACE get events --sort-by=.lastTimestamp
```

## 7. 集群内健康探针验收

先做端口转发，避免外部 DNS / LB 还没完成时误判应用有问题。

```powershell
kubectl -n $env:KUBE_NAMESPACE port-forward svc/exchange 3030:3030
```

另开一个终端，检查公开接口：

```powershell
curl http://127.0.0.1:3030/health
curl http://127.0.0.1:3030/ready
curl http://127.0.0.1:3030/metrics/prometheus
```

## 8. 管理接口验收

用现有 PowerShell helper 直接打带签名的请求。

```powershell
cd .\rust-exchange
. .\scripts\backend_resilience_lib.ps1

$secret = (Get-Content .\secrets\internal_auth.secret -Raw).Trim()
$client = New-HttpClient -TimeoutSeconds 15

Invoke-ApiJsonRequest -Client $client -BaseUrl "http://127.0.0.1:3030" -Method "GET" -Path "/openapi.json" -Secret $secret -Subject "ops-admin" -Role "admin"
Invoke-ApiJsonRequest -Client $client -BaseUrl "http://127.0.0.1:3030" -Method "GET" -Path "/admin/oncall/status" -Secret $secret -Subject "ops-admin" -Role "admin"
Invoke-ApiJsonRequest -Client $client -BaseUrl "http://127.0.0.1:3030" -Method "GET" -Path "/admin/sentinel/posture" -Secret $secret -Subject "ops-admin" -Role "admin"
Invoke-ApiJsonRequest -Client $client -BaseUrl "http://127.0.0.1:3030" -Method "GET" -Path "/admin/capacity/alerts" -Secret $secret -Subject "ops-admin" -Role "admin"

$client.Dispose()
```

通过标准：

- `/openapi.json` 返回 `200`
- `/admin/oncall/status` 中存在 `webhook(ONCALL_WEBHOOK_URL)` 且 `healthy = true`
- `/admin/sentinel/posture` 可正常返回 posture JSON

## 9. 真实交易链路验收

继续用 helper 跑一笔完整交易。

```powershell
cd .\rust-exchange
. .\scripts\backend_resilience_lib.ps1

$secret = (Get-Content .\secrets\internal_auth.secret -Raw).Trim()
$client = New-HttpClient -TimeoutSeconds 30
$baseUrl = "http://127.0.0.1:3030"

Seed-ExchangeUsers `
  -Client $client `
  -BaseUrl $baseUrl `
  -Secret $secret `
  -Markets @("btc-usdt") `
  -BuyerUsers @("bench-buyer") `
  -SellerUsers @("bench-seller") `
  -AdminSubject "ops-admin" `
  -CashAmount 10000000 `
  -PositionAmount 50

$makerBody = New-OrderBody -MarketId "btc-usdt" -Side "sell" -Price 50000 -Amount 2 -ClientOrderId "cluster-maker-1"
$takerBody = New-OrderBody -MarketId "btc-usdt" -Side "buy" -Price 50000 -Amount 2 -ClientOrderId "cluster-taker-1"

$maker = Invoke-ApiJsonRequest -Client $client -BaseUrl $baseUrl -Method "POST" -Path "/submit-order" -Secret $secret -Subject "bench-seller" -Role "user" -Body $makerBody
$taker = Invoke-ApiJsonRequest -Client $client -BaseUrl $baseUrl -Method "POST" -Path "/submit-order" -Secret $secret -Subject "bench-buyer" -Role "user" -Body $takerBody
$trades = Invoke-ApiJsonRequest -Client $client -BaseUrl $baseUrl -Method "GET" -Path "/trades?market_id=btc-usdt&limit=10" -Secret $secret -Subject "bench-buyer" -Role "user"

$maker
$taker
$trades

$client.Dispose()
```

通过标准：

- maker `status_code = 200`
- taker `status_code = 200`
- taker `fills > 0`
- `/trades?market_id=btc-usdt` 返回有该市场的成交记录

## 10. 真实域名 / TLS / 外部 LB 联调

确认 Ingress 和 LB：

```powershell
kubectl -n $env:KUBE_NAMESPACE describe ingress exchange
kubectl -n ingress-nginx get svc
```

确认证书：

```powershell
kubectl -n $env:KUBE_NAMESPACE get certificate
kubectl -n $env:KUBE_NAMESPACE describe certificate $env:EXCHANGE_TLS_SECRET
kubectl -n $env:KUBE_NAMESPACE get secret $env:EXCHANGE_TLS_SECRET
```

确认 DNS：

```powershell
nslookup $env:EXCHANGE_HOST
```

最终从外部地址验收：

```powershell
curl https://$env:EXCHANGE_HOST/health
curl https://$env:EXCHANGE_HOST/ready
```

如果这里不通，分层排查顺序固定为：

1. `kubectl get ingress`
2. `kubectl describe ingress`
3. `kubectl get svc -n ingress-nginx`
4. `kubectl get certificate`
5. `nslookup <host>`
6. 云 LB 安全组 / 防火墙 `80/443`

## 11. 告警与 on-call 联动实测

先验证配置已生效：

```powershell
cd .\rust-exchange
. .\scripts\backend_resilience_lib.ps1

$secret = (Get-Content .\secrets\internal_auth.secret -Raw).Trim()
$client = New-HttpClient -TimeoutSeconds 15

Invoke-ApiJsonRequest -Client $client -BaseUrl "http://127.0.0.1:3030" -Method "GET" -Path "/admin/oncall/status" -Secret $secret -Subject "ops-admin" -Role "admin"

$client.Dispose()
```

看点：

- `alert_channels` 里必须有 `webhook(ONCALL_WEBHOOK_URL)`
- `healthy = true`
- `dead_man_switch` 状态正常

然后做一次最小联调：

1. 保持 `port-forward` 或外部域名可访问
2. 调 `/admin/sentinel/posture`
3. 调 `/admin/oncall/status`
4. 检查 webhook 接收端是否收到了通知或至少处于健康状态
5. 截图或保存响应 JSON，作为首次上线留档

如果你们要做更强的联调，可以选一个低风险方式：

- 临时将 `ONCALL_WEBHOOK_URL` 指向内部测试 webhook 接收器
- 记录收到的 JSON payload
- 恢复正式 webhook 地址

## 12. 对象存储备份验收

检查备份任务：

```powershell
kubectl -n $env:KUBE_NAMESPACE get cronjob exchange-wal-backup
kubectl -n $env:KUBE_NAMESPACE create job --from=cronjob/exchange-wal-backup exchange-wal-backup-manual-1
kubectl -n $env:KUBE_NAMESPACE logs job/exchange-wal-backup-manual-1
```

通过标准：

- 能成功打包 `/app/data`
- 能成功上传到 `BACKUP_S3_BUCKET`

## 13. 对象存储恢复演练

执行恢复 Job：

```powershell
kubectl -n $env:KUBE_NAMESPACE apply -f .\deploy\k8s\exchange-restore-job.yaml
kubectl -n $env:KUBE_NAMESPACE logs job/exchange-wal-restore
```

通过标准：

- 能从 `RESTORE_S3_URI` 下载归档
- 能成功解压
- 日志里能列出恢复出的 WAL 文件

本地也可以做一次离线恢复验证：

```powershell
.\scripts\run_wal_restore_drill.ps1 -BackupArchive .\exchange-wal-20260411T120000Z.tgz -CleanRestoreDir
```

## 14. 上线后首日盯盘项

上线后第一层运维闭环至少要盯：

```powershell
kubectl -n $env:KUBE_NAMESPACE logs deploy/exchange --tail=200
```

以及 Prometheus 指标：

- `exchange_submit_order_ip_rate_limited_total`
- `exchange_submit_order_user_rate_limited_total`
- `exchange_submit_order_engine_rate_limited_total`
- `exchange_http_requests_total`
- `wal_errors_total`

## 15. 本轮已被本机验证的部分

参考：

- [`DEPLOYMENT_ACCEPTANCE_ZH_2026-04-11.md`](DEPLOYMENT_ACCEPTANCE_ZH_2026-04-11.md)
- [`deployment_acceptance_report.json`](artifacts/deployment-acceptance/20260411-143121/reports/deployment_acceptance_report.json)

已确认通过：

- 文件型 secret 启动
- role mapping 文件启动
- `/health`
- `/ready`
- `/metrics/prometheus`
- admin `GET /openapi.json`
- 一笔真实成交链路
