#!/usr/bin/env bash
# ============================================================
#  Rust Exchange — WSL2 Docker 测试脚本
#  用法:  bash test_wsl2.sh [build|run|test|stop|clean|all]
# ============================================================
set -euo pipefail

IMAGE_NAME="rust-exchange"
CONTAINER_NAME="rust-exchange-test"
PORT=3030
BASE_URL="http://localhost:${PORT}"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
ok()   { echo -e "${GREEN}✓ $*${NC}"; }
fail() { echo -e "${RED}✗ $*${NC}"; }
info() { echo -e "${YELLOW}▸ $*${NC}"; }

# ── Build ─────────────────────────────────────────────────
cmd_build() {
    info "Building Docker image: ${IMAGE_NAME}..."
    docker build -t "${IMAGE_NAME}:latest" .
    ok "Image built successfully"
    docker images "${IMAGE_NAME}" --format "  Size: {{.Size}}  Created: {{.CreatedSince}}"
}

# ── Run ───────────────────────────────────────────────────
cmd_run() {
    # Stop existing container if running
    docker rm -f "${CONTAINER_NAME}" 2>/dev/null || true

    info "Starting container: ${CONTAINER_NAME} on port ${PORT}..."
    docker run -d \
        --name "${CONTAINER_NAME}" \
        -p "${PORT}:3030" \
        -e RUST_LOG=info \
        -e API_BIND_HOST=0.0.0.0 \
        -e API_BIND_PORT=3030 \
        "${IMAGE_NAME}:latest"

    info "Waiting for server to be ready..."
    for i in $(seq 1 30); do
        if curl -sf "${BASE_URL}/health" > /dev/null 2>&1; then
            ok "Server is ready (took ~${i}s)"
            return 0
        fi
        sleep 1
    done
    fail "Server failed to start within 30s"
    docker logs "${CONTAINER_NAME}" --tail 20
    return 1
}

# ── Test ──────────────────────────────────────────────────
cmd_test() {
    local passed=0 failed=0 total=0

    run_test() {
        local name="$1" method="$2" url="$3" expected_code="${4:-200}"
        total=$((total + 1))
        local code
        code=$(curl -sf -o /dev/null -w '%{http_code}' -X "${method}" "${url}" 2>/dev/null || echo "000")
        if [ "$code" = "$expected_code" ]; then
            ok "[${code}] ${name}"
            passed=$((passed + 1))
        else
            fail "[${code}] ${name} (expected ${expected_code})"
            failed=$((failed + 1))
        fi
    }

    echo ""
    info "═══════════════════════════════════════════"
    info "  Rust Exchange — API 端到端测试"
    info "═══════════════════════════════════════════"
    echo ""

    # ── System endpoints ──
    info "系统端点 (System)"
    run_test "GET /health"                  GET  "${BASE_URL}/health"
    run_test "GET /ready"                   GET  "${BASE_URL}/ready"
    run_test "GET /health/partitions"       GET  "${BASE_URL}/health/partitions"
    run_test "GET /metrics"                 GET  "${BASE_URL}/metrics"
    run_test "GET /metrics/prometheus"      GET  "${BASE_URL}/metrics/prometheus"
    run_test "GET /openapi.json"            GET  "${BASE_URL}/openapi.json"
    run_test "GET /swagger-ui"              GET  "${BASE_URL}/swagger-ui"

    # ── Market endpoints (no auth required for reads) ──
    echo ""
    info "市场端点 (Markets)"
    run_test "GET /markets"                 GET  "${BASE_URL}/markets"
    run_test "GET /trades"                  GET  "${BASE_URL}/trades"
    run_test "GET /stats"                   GET  "${BASE_URL}/stats"
    run_test "GET /matching-status"         GET  "${BASE_URL}/matching-status"

    # ── Prometheus format validation ──
    echo ""
    info "Prometheus 指标格式验证"
    local prom_output
    prom_output=$(curl -sf "${BASE_URL}/metrics/prometheus" 2>/dev/null || echo "")
    total=$((total + 1))
    if echo "$prom_output" | grep -q "exchange_orders_received_total"; then
        ok "Prometheus output contains expected counters"
        passed=$((passed + 1))
    else
        fail "Prometheus output missing expected counters"
        failed=$((failed + 1))
    fi

    # ── OpenAPI spec validation ──
    info "OpenAPI 规范验证"
    local spec
    spec=$(curl -sf "${BASE_URL}/openapi.json" 2>/dev/null || echo "{}")
    total=$((total + 1))
    if echo "$spec" | python3 -c "import sys,json; d=json.load(sys.stdin); assert d['openapi']=='3.0.3'; assert len(d['paths'])>30" 2>/dev/null; then
        ok "OpenAPI 3.0.3 spec valid with 30+ paths"
        passed=$((passed + 1))
    else
        fail "OpenAPI spec validation failed"
        failed=$((failed + 1))
    fi

    # ── Health response structure ──
    info "Health 响应结构验证"
    local health
    health=$(curl -sf "${BASE_URL}/health" 2>/dev/null || echo "{}")
    total=$((total + 1))
    if echo "$health" | python3 -c "import sys,json; d=json.load(sys.stdin); assert d['status']=='ok'; assert 'uptime_secs' in d" 2>/dev/null; then
        ok "Health response has status=ok and uptime_secs"
        passed=$((passed + 1))
    else
        fail "Health response structure invalid"
        failed=$((failed + 1))
    fi

    # ── Summary ──
    echo ""
    info "═══════════════════════════════════════════"
    if [ "$failed" -eq 0 ]; then
        ok "全部通过: ${passed}/${total} tests passed"
    else
        fail "失败: ${failed}/${total} tests failed"
    fi
    info "═══════════════════════════════════════════"
    echo ""

    return "$failed"
}

# ── Stop ──────────────────────────────────────────────────
cmd_stop() {
    info "Stopping container: ${CONTAINER_NAME}..."
    docker stop "${CONTAINER_NAME}" 2>/dev/null && docker rm "${CONTAINER_NAME}" 2>/dev/null
    ok "Container stopped and removed"
}

# ── Clean ─────────────────────────────────────────────────
cmd_clean() {
    cmd_stop 2>/dev/null || true
    info "Removing image: ${IMAGE_NAME}..."
    docker rmi "${IMAGE_NAME}:latest" 2>/dev/null || true
    ok "Cleaned up"
}

# ── All (build → run → test → stop) ──────────────────────
cmd_all() {
    cmd_build
    cmd_run
    cmd_test
    local result=$?
    cmd_stop
    return $result
}

# ── Logs ──────────────────────────────────────────────────
cmd_logs() {
    docker logs "${CONTAINER_NAME}" --tail "${1:-50}" -f
}

# ── docker-compose shortcut ──────────────────────────────
cmd_compose() {
    info "Starting via docker-compose..."
    docker compose up -d --build
    info "Waiting for server..."
    sleep 5
    cmd_test
}

# ── Main ──────────────────────────────────────────────────
case "${1:-all}" in
    build)   cmd_build ;;
    run)     cmd_run ;;
    test)    cmd_test ;;
    stop)    cmd_stop ;;
    clean)   cmd_clean ;;
    logs)    cmd_logs "${2:-50}" ;;
    compose) cmd_compose ;;
    all)     cmd_all ;;
    *)
        echo "用法: $0 {build|run|test|stop|clean|logs|compose|all}"
        echo ""
        echo "  build    构建 Docker 镜像"
        echo "  run      启动容器"
        echo "  test     运行 API 端到端测试"
        echo "  stop     停止并删除容器"
        echo "  clean    停止容器 + 删除镜像"
        echo "  logs     查看容器日志 (可选: logs 100)"
        echo "  compose  使用 docker-compose 启动并测试"
        echo "  all      build → run → test → stop (默认)"
        exit 1
        ;;
esac
