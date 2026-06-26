using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using System.Net.Http;
using System.Security.Cryptography;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace AdvancedBench
{
    class Program
    {
        static readonly string BaseUri = "http://localhost:3030";
        static readonly string Secret = "dev-secret-change-me";
        static readonly HttpClient Client;
        static int _seq;

        // Use only spot markets for stress tests (perp/margin require special config)
        static readonly string[] Markets = { "btc-usdt", "eth-usdt" };

        static async Task FundUsers(string userPrefix, int count, long amount)
        {
            Console.WriteLine($"\n  Funding {count} users ({userPrefix}-0..{userPrefix}-{count-1}) with {amount:N0} each...");
            int ok = 0, fail = 0;
            for (int i = 0; i < count; i++)
            {
                var userId = $"{userPrefix}-{i}";
                var opId = $"fund-{userId}-{Guid.NewGuid():N}";
                var bodyJson = $"{{\"user_id\":\"{userId}\",\"amount\":{amount},\"op_id\":\"{opId}\"}}";
                var bodyBytes = Encoding.UTF8.GetBytes(bodyJson);

                var timestamp = DateTimeOffset.UtcNow.ToUnixTimeSeconds().ToString();
                var bodyHash = ComputeHash(SHA256.Create(), bodyBytes);
                var payload = $"POST\n/deposit\n\nadmin\nadmin\n\n{timestamp}\nfund-{opId}";
                var signature = ComputeHmac(payload, Secret);

                var content = new ByteArrayContent(bodyBytes);
                content.Headers.ContentType = new System.Net.Http.Headers.MediaTypeHeaderValue("application/json");
                content.Headers.TryAddWithoutValidation("x-internal-auth-subject", "admin");
                content.Headers.TryAddWithoutValidation("x-internal-auth-role", "admin");
                content.Headers.TryAddWithoutValidation("x-internal-auth-session-id", "");
                content.Headers.TryAddWithoutValidation("x-internal-auth-timestamp", timestamp);
                content.Headers.TryAddWithoutValidation("x-internal-auth-signature", signature);
                content.Headers.TryAddWithoutValidation("x-internal-auth-body-sha256", bodyHash);
                content.Headers.TryAddWithoutValidation("x-request-id", $"fund-{opId}");

                var req = new HttpRequestMessage(HttpMethod.Post, $"{BaseUri}/deposit") { Content = content };
                try
                {
                    var resp = await Client.SendAsync(req);
                    if (resp.IsSuccessStatusCode) ok++;
                    else fail++;
                    resp.Dispose();
                }
                catch { fail++; }
                finally { req.Dispose(); }

                // Rate limit: admin deposit is 10/sec, so space requests ~120ms apart
                await Task.Delay(120);
            }
            Console.WriteLine($"  Funded: {ok}/{count} | Failed: {fail}");
            if (fail > 0) throw new Exception($"Failed to fund {fail} users");
        }

        static Program()
        {
            var handler = new HttpClientHandler { UseCookies = false, MaxConnectionsPerServer = 200 };
            Client = new HttpClient(handler) { Timeout = TimeSpan.FromSeconds(30) };
            // Pre-warm connections
            Client.GetAsync($"{BaseUri}/health").Result.Dispose();
        }

        static HttpRequestMessage BuildOrderRequest(string userId, string marketId, string side, long price, long amount, int? outcome = null)
        {
            var rid = $"adv-{Interlocked.Increment(ref _seq)}-{Guid.NewGuid():N}";
            var cid = $"co-{Guid.NewGuid():N}";
            var bodyJson = $"{{\"market_id\":\"{marketId}\",\"side\":\"{side}\",\"price\":{price},\"amount\":{amount},\"outcome\":{outcome ?? 0},\"time_in_force\":\"GTC\",\"user_id\":\"{userId}\",\"client_order_id\":\"{cid}\"}}";
            var bodyBytes = Encoding.UTF8.GetBytes(bodyJson);

            var timestamp = DateTimeOffset.UtcNow.ToUnixTimeSeconds().ToString();
            var bodyHash = ComputeHash(SHA256.Create(), bodyBytes);
            var payload = $"POST\n/intent\n\n{userId}\nuser\n\n{timestamp}\n{rid}";
            var signature = ComputeHmac(payload, Secret);

            var content = new ByteArrayContent(bodyBytes);
            content.Headers.ContentType = new System.Net.Http.Headers.MediaTypeHeaderValue("application/json");
            content.Headers.TryAddWithoutValidation("x-internal-auth-subject", userId);
            content.Headers.TryAddWithoutValidation("x-internal-auth-role", "user");
            content.Headers.TryAddWithoutValidation("x-internal-auth-session-id", "");
            content.Headers.TryAddWithoutValidation("x-internal-auth-timestamp", timestamp);
            content.Headers.TryAddWithoutValidation("x-internal-auth-signature", signature);
            content.Headers.TryAddWithoutValidation("x-internal-auth-body-sha256", bodyHash);
            content.Headers.TryAddWithoutValidation("x-request-id", rid);

            return new HttpRequestMessage(HttpMethod.Post, $"{BaseUri}/intent") { Content = content };
        }

        static HttpRequestMessage BuildCancelRequest(string userId, string marketId, string orderId, int? outcome = null)
        {
            var rid = $"adv-cancel-{Interlocked.Increment(ref _seq)}-{Guid.NewGuid():N}";
            var bodyJson = $"{{\"market_id\":\"{marketId}\",\"order_id\":\"{orderId}\",\"outcome\":{outcome ?? 0}}}";
            var bodyBytes = Encoding.UTF8.GetBytes(bodyJson);

            var timestamp = DateTimeOffset.UtcNow.ToUnixTimeSeconds().ToString();
            var bodyHash = ComputeHash(SHA256.Create(), bodyBytes);
            var payload = $"POST\n/cancel-order\n\n{userId}\nuser\n\n{timestamp}\n{rid}";
            var signature = ComputeHmac(payload, Secret);

            var content = new ByteArrayContent(bodyBytes);
            content.Headers.ContentType = new System.Net.Http.Headers.MediaTypeHeaderValue("application/json");
            content.Headers.TryAddWithoutValidation("x-internal-auth-subject", userId);
            content.Headers.TryAddWithoutValidation("x-internal-auth-role", "user");
            content.Headers.TryAddWithoutValidation("x-internal-auth-session-id", "");
            content.Headers.TryAddWithoutValidation("x-internal-auth-timestamp", timestamp);
            content.Headers.TryAddWithoutValidation("x-internal-auth-signature", signature);
            content.Headers.TryAddWithoutValidation("x-internal-auth-body-sha256", bodyHash);
            content.Headers.TryAddWithoutValidation("x-request-id", rid);

            return new HttpRequestMessage(HttpMethod.Post, $"{BaseUri}/cancel-order") { Content = content };
        }

        static HttpRequestMessage BuildReplaceRequest(string userId, string marketId, string orderId, long? newPrice = null, long? newAmount = null, int? outcome = null)
        {
            var rid = $"adv-replace-{Interlocked.Increment(ref _seq)}-{Guid.NewGuid():N}";
            var pricePart = newPrice.HasValue ? $",\"new_price\":{newPrice.Value}" : "";
            var amountPart = newAmount.HasValue ? $",\"new_amount\":{newAmount.Value}" : "";
            var bodyJson = $"{{\"market_id\":\"{marketId}\",\"order_id\":\"{orderId}\",\"outcome\":{outcome ?? 0}{pricePart}{amountPart}}}";
            var bodyBytes = Encoding.UTF8.GetBytes(bodyJson);

            var timestamp = DateTimeOffset.UtcNow.ToUnixTimeSeconds().ToString();
            var bodyHash = ComputeHash(SHA256.Create(), bodyBytes);
            var payload = $"POST\n/replace-order\n\n{userId}\nuser\n\n{timestamp}\n{rid}";
            var signature = ComputeHmac(payload, Secret);

            var content = new ByteArrayContent(bodyBytes);
            content.Headers.ContentType = new System.Net.Http.Headers.MediaTypeHeaderValue("application/json");
            content.Headers.TryAddWithoutValidation("x-internal-auth-subject", userId);
            content.Headers.TryAddWithoutValidation("x-internal-auth-role", "user");
            content.Headers.TryAddWithoutValidation("x-internal-auth-session-id", "");
            content.Headers.TryAddWithoutValidation("x-internal-auth-timestamp", timestamp);
            content.Headers.TryAddWithoutValidation("x-internal-auth-signature", signature);
            content.Headers.TryAddWithoutValidation("x-internal-auth-body-sha256", bodyHash);
            content.Headers.TryAddWithoutValidation("x-request-id", rid);

            return new HttpRequestMessage(HttpMethod.Post, $"{BaseUri}/replace-order") { Content = content };
        }

        static string ComputeHash(HashAlgorithm algo, byte[] data)
        {
            var h = algo.ComputeHash(data);
            algo.Dispose();
            return BitConverter.ToString(h).Replace("-", "").ToLowerInvariant();
        }

        static string ComputeHmac(string msg, string secret)
        {
            using var hmac = new HMACSHA256(Encoding.UTF8.GetBytes(secret));
            var h = hmac.ComputeHash(Encoding.UTF8.GetBytes(msg));
            return BitConverter.ToString(h).Replace("-", "").ToLowerInvariant();
        }

        static void PrintResults(List<long> latenciesTicks, int ok, int fail, int total, string testName)
        {
            latenciesTicks.Sort();
            if (latenciesTicks.Count == 0)
            {
                Console.WriteLine($"\n  [{testName}] Orders: 0/{total} (0%) | Failed: {fail}");
                return;
            }

            var tickFactor = 1_000_000.0 / Stopwatch.Frequency;
            var toUs = (long t) => (long)(t * tickFactor);

            var p50 = toUs(latenciesTicks[latenciesTicks.Count / 2]);
            var p95 = toUs(latenciesTicks[Math.Min((int)(latenciesTicks.Count * 0.95), latenciesTicks.Count - 1)]);
            var p99 = toUs(latenciesTicks[Math.Min((int)(latenciesTicks.Count * 0.99), latenciesTicks.Count - 1)]);
            var avg = latenciesTicks.Average() * tickFactor;
            var min = toUs(latenciesTicks.First());
            var max = toUs(latenciesTicks.Last());

            Console.WriteLine($"\n  [{testName}] Orders: {ok}/{total} ({(double)ok / total * 100:F0}%) | Failed: {fail}");
            Console.WriteLine($"  [{testName}] Latency: P50={p50}μs | P95={p95}μs | P99={p99}μs | Avg={avg:F0}μs | Min={min}μs | Max={max}μs");

            // Distribution histogram
            Console.WriteLine($"\n  [{testName}] Distribution:");
            var buckets = new Dictionary<string, int>();
            foreach (var t in latenciesTicks)
            {
                var us = toUs(t);
                var bucketKey = us switch
                {
                    < 500 => "   0-  500μs",
                    < 1000 => " 500- 1000μs",
                    < 1500 => "1000- 1500μs",
                    < 2000 => "1500- 2000μs",
                    < 2500 => "2000- 2500μs",
                    < 3000 => "2500- 3000μs",
                    < 5000 => "3000- 5000μs",
                    < 10000 => "5000-10000μs",
                    < 20000 => "10000-20000μs",
                    < 50000 => "20000-50000μs",
                    _ => "50000+ μs"
                };
                buckets[bucketKey] = buckets.GetValueOrDefault(bucketKey, 0) + 1;
            }
            var maxCount = buckets.Values.Max();
            var barWidth = Console.WindowWidth > 80 ? 40 : 20;
            foreach (var kvp in buckets.OrderBy(k => k.Key))
            {
                var barLen = (int)((double)kvp.Value / maxCount * barWidth);
                Console.WriteLine($"       {kvp.Key}: {new string('█', barLen)} {kvp.Value}");
            }
        }

        static async Task<(List<long> latencies, int ok, int fail)> RunRequestsAsync(Func<Task<(long ticks, bool ok)>> requestFactory, int count)
        {
            var latencies = new List<long>();
            int ok = 0, fail = 0;
            for (int i = 0; i < count; i++)
            {
                var (ticks, success) = await requestFactory();
                if (success) { ok++; latencies.Add(ticks); }
                else fail++;
            }
            return (latencies, ok, fail);
        }

        static async Task<(List<long> latencies, int ok, int fail)> RunConcurrentAsync(Func<int, Task<(long ticks, bool ok)>> requestFactory, int count, int concurrency)
        {
            var latencies = new ConcurrentBag<long>();
            int ok = 0, fail = 0;

            var semaphore = new SemaphoreSlim(concurrency);
            var tasks = new Task[count];

            for (int i = 0; i < count; i++)
            {
                int idx = i;
                tasks[i] = Task.Run(async () =>
                {
                    await semaphore.WaitAsync();
                    try
                    {
                        var (ticks, success) = await requestFactory(idx);
                        if (success) { Interlocked.Increment(ref ok); latencies.Add(ticks); }
                        else Interlocked.Increment(ref fail);
                    }
                    finally { semaphore.Release(); }
                });
            }

            await Task.WhenAll(tasks);
            return (latencies.ToList(), ok, fail);
        }

        static async Task StressTest(int count, int concurrency, string testName)
        {
            Console.WriteLine($"\n{'=',-60}");
            Console.WriteLine($"  {testName}");
            Console.WriteLine($"  Count={count}, Concurrency={concurrency}, Markets={Markets.Length}");
            Console.WriteLine($"{'=',-60}");

            var latencies = new ConcurrentBag<long>();
            int ok = 0, fail = 0;
            var orderIds = new ConcurrentBag<string>(); // Track created orders for potential cancel testing

            // Timing breakdown collectors (microseconds)
            var queueWaitUs = new ConcurrentBag<long>();
            var matchExecUs = new ConcurrentBag<long>();
            var validationUs = new ConcurrentBag<long>();
            var riskUs = new ConcurrentBag<long>();
            var matchingUs = new ConcurrentBag<long>();
            var walUs = new ConcurrentBag<long>();
            var postMatchUs = new ConcurrentBag<long>();
            var authUs = new ConcurrentBag<long>();
            var ipRlUs = new ConcurrentBag<long>();
            var userRlUs = new ConcurrentBag<long>();
            var sentinelUs = new ConcurrentBag<long>();
            var lookupUs = new ConcurrentBag<long>();
            var sequencerUs = new ConcurrentBag<long>();
            var totalE2eUs = new ConcurrentBag<long>();
            var preMatchUs = new ConcurrentBag<long>();

            var semaphore = new SemaphoreSlim(concurrency);
            var tasks = new Task[count];
            var barrier = new Barrier(concurrency > count ? count : concurrency); // Sync start

            for (int i = 0; i < count; i++)
            {
                int idx = i;
                tasks[i] = Task.Run(async () =>
                {
                    await semaphore.WaitAsync();
                    try
                    {
                        var userId = $"bm-latency-{idx % 50}";
                        var market = Markets[idx % Markets.Length]; // Distribute across markets for partition spread
                        var outcome = idx % 3; // Distribute across outcomes for further partition spread
                        // All BUY orders to avoid position requirements
                        var side = "buy";
                        // Spread prices around a low level to minimize partial fills
                        var price = 100 + (idx % 50);
                        var amount = 1;

                        var req = BuildOrderRequest(userId, market, side, price, amount, outcome);
                        var sw = Stopwatch.StartNew();
                        try
                        {
                            var resp = await Client.SendAsync(req, HttpCompletionOption.ResponseHeadersRead);
                            sw.Stop();
                            var body = await resp.Content.ReadAsStringAsync();
                            if (resp.IsSuccessStatusCode)
                            {
                                Interlocked.Increment(ref ok);
                                latencies.Add(sw.ElapsedTicks);
                                // Extract timing breakdown from response
                                try
                                {
                                    var json = System.Text.Json.JsonDocument.Parse(body);
                                    if (json.RootElement.TryGetProperty("order_id", out var oid))
                                        orderIds.Add(oid.GetString()!);
                                    
                                    // Debug: print first response
                                    if (idx == 0)
                                        Console.WriteLine($"    [DEBUG] Response: {body}");
                                    
                                    void TryExtract(string prop, ConcurrentBag<long> bag)
                                    {
                                        if (json.RootElement.TryGetProperty(prop, out var v) && v.TryGetInt64(out var val))
                                            bag.Add(val);
                                    }
                                    TryExtract("queue_wait_us", queueWaitUs);
                                    TryExtract("match_execution_us", matchExecUs);
                                    TryExtract("validation_us", validationUs);
                                    TryExtract("risk_us", riskUs);
                                    TryExtract("matching_us", matchingUs);
                                    TryExtract("wal_us", walUs);
                                    TryExtract("post_match_us", postMatchUs);
                                    TryExtract("auth_us", authUs);
                                    TryExtract("ip_rl_us", ipRlUs);
                                    TryExtract("user_rl_us", userRlUs);
                                    TryExtract("sentinel_us", sentinelUs);
                                    TryExtract("lookup_us", lookupUs);
                                    TryExtract("sequencer_us", sequencerUs);
                                    TryExtract("total_e2e_us", totalE2eUs);
                                    TryExtract("pre_match_us", preMatchUs);
                                }
                                catch { }
                            }
                            else
                            {
                                Interlocked.Increment(ref fail);
                                if (fail <= 10)
                                    Console.WriteLine($"    [{idx}] FAIL {(int)resp.StatusCode}: {body[..Math.Min(100, body.Length)]}");
                            }
                            resp.Dispose();
                        }
                        catch (Exception ex)
                        {
                            sw.Stop();
                            Interlocked.Increment(ref fail);
                            if (fail <= 10) Console.WriteLine($"    [{idx}] ERR: {ex.Message}");
                        }
                        finally { req.Dispose(); }
                    }
                    finally { semaphore.Release(); }
                });
            }

            await Task.WhenAll(tasks);

            PrintResults(latencies.ToList(), ok, fail, count, testName);

            // Print timing breakdown
            Console.WriteLine($"  [DEBUG] queueWaitUs.Count={queueWaitUs.Count}, matchExecUs.Count={matchExecUs.Count}");
            if (queueWaitUs.Count > 0)
            {
                Console.WriteLine($"\n  {'─',-60}");
                Console.WriteLine($"  Server-Side Timing Breakdown (μs):");
                Console.WriteLine($"  {'─',-60}");
                PrintBreakdown("Queue Wait", queueWaitUs.ToList());
                PrintBreakdown("Match Exec", matchExecUs.ToList());
                PrintBreakdown("Validation", validationUs.ToList());
                PrintBreakdown("Risk", riskUs.ToList());
                PrintBreakdown("Matching", matchingUs.ToList());
                PrintBreakdown("WAL", walUs.ToList());
                PrintBreakdown("Post-Match", postMatchUs.ToList());
                
                Console.WriteLine($"\n  {'─',-60}");
                Console.WriteLine($"  Pre-Match Breakdown (μs):");
                Console.WriteLine($"  {'─',-60}");
                PrintBreakdown("Auth", authUs.ToList());
                PrintBreakdown("IP Rate Limit", ipRlUs.ToList());
                PrintBreakdown("User Rate Limit", userRlUs.ToList());
                PrintBreakdown("Sentinel", sentinelUs.ToList());
                PrintBreakdown("Lookup/Validate", lookupUs.ToList());
                PrintBreakdown("Sequencer", sequencerUs.ToList());
                PrintBreakdown("Total Pre-Match", preMatchUs.ToList());
                PrintBreakdown("Total E2E", totalE2eUs.ToList());
            }
        }

        static void PrintBreakdown(string label, List<long> valuesUs)
        {
            if (valuesUs.Count == 0) return;
            valuesUs.Sort();
            var p50 = valuesUs[valuesUs.Count / 2];
            var p95 = valuesUs[(int)(valuesUs.Count * 0.95)];
            var p99 = valuesUs[(int)(valuesUs.Count * 0.99)];
            var avg = (long)valuesUs.Average();
            Console.WriteLine($"    {label,-14} P50={p50,7}μs  P95={p95,7}μs  P99={p99,7}μs  Avg={avg,7}μs");
        }

        static async Task BatchOrderTest(int totalOrders, int batchSize, string testName)
        {
            Console.WriteLine($"\n{'=',-60}");
            Console.WriteLine($"  {testName}");
            Console.WriteLine($"  TotalOrders={totalOrders}, BatchSize={batchSize}, Markets={Markets.Length}");
            Console.WriteLine($"{'=',-60}");

            var batchLatencies = new ConcurrentBag<long>();
            var perOrderLatencies = new ConcurrentBag<long>();
            var batchTimingE2e = new ConcurrentBag<long>();
            var batchTimingAvgOrder = new ConcurrentBag<long>();
            int totalOk = 0, totalFail = 0, totalOrdersSubmitted = 0;
            int batchCount = (totalOrders + batchSize - 1) / batchSize;
            var concurrency = 1; // Sequential to avoid rate limits

            var semaphore = new SemaphoreSlim(concurrency);
            var tasks = new Task[batchCount];

            for (int b = 0; b < batchCount; b++)
            {
                int batchIdx = b;
                tasks[b] = Task.Run(async () =>
                {
                    await semaphore.WaitAsync();
                    try
                    {
                        var ordersInBatch = Math.Min(batchSize, totalOrders - batchIdx * batchSize);
                        var orders = new List<object>();
                        var userIds = new List<string>();

                        for (int i = 0; i < ordersInBatch; i++)
                        {
                            var globalIdx = batchIdx * batchSize + i;
                            var userId = $"bm-batch-{globalIdx % 50}";
                            userIds.Add(userId);
                            var market = Markets[globalIdx % Markets.Length];
                            var outcome = globalIdx % 3;
                            var price = 100 + (globalIdx % 50);
                            var amount = 1;
                            var uniqueSuffix = Guid.NewGuid().ToString("N")[..8];

                            orders.Add(new
                            {
                                request_id = $"req-{batchIdx}-{i}-{uniqueSuffix}",
                                client_order_id = $"co-{batchIdx}-{i}-{uniqueSuffix}",
                                market_id = market,
                                side = "buy",
                                order_type = (string?)null,
                                time_in_force = (string?)null,
                                price = (int?)price,
                                amount = amount,
                                outcome = outcome,
                                post_only = (bool?)false,
                                reduce_only = (bool?)false,
                                leverage = (int?)null,
                                expires_at = (string?)null,
                                stp_mode = (string?)null,
                                trigger_price = (int?)null,
                                trigger_type = (string?)null,
                                session_id = (string?)null
                            });
                        }

                        var batchJson = System.Text.Json.JsonSerializer.Serialize(new { orders });
                        var bodyBytes = Encoding.UTF8.GetBytes(batchJson);

                        var sw = Stopwatch.StartNew();
                        
                        // Generate unique auth params per request to avoid 409 conflicts
                        var timestamp = DateTimeOffset.UtcNow.ToUnixTimeSeconds().ToString();
                        var nonce = $"batch-{batchIdx}-{Guid.NewGuid():N}-{DateTimeOffset.UtcNow.ToUnixTimeMilliseconds()}";
                        var bodyHash = ComputeHash(SHA256.Create(), bodyBytes);
                        var payload = $"POST\n/batch-orders\n\nadmin\nadmin\n\n{timestamp}\n{nonce}";
                        var signature = ComputeHmac(payload, Secret);

                        var content = new ByteArrayContent(bodyBytes);
                        content.Headers.ContentType = new System.Net.Http.Headers.MediaTypeHeaderValue("application/json");
                        content.Headers.TryAddWithoutValidation("x-internal-auth-subject", "admin");
                        content.Headers.TryAddWithoutValidation("x-internal-auth-role", "admin");
                        content.Headers.TryAddWithoutValidation("x-internal-auth-session-id", "");
                        content.Headers.TryAddWithoutValidation("x-internal-auth-timestamp", timestamp);
                        content.Headers.TryAddWithoutValidation("x-internal-auth-signature", signature);
                        content.Headers.TryAddWithoutValidation("x-internal-auth-body-sha256", bodyHash);
                        content.Headers.TryAddWithoutValidation("x-request-id", nonce);

                        var req = new HttpRequestMessage(HttpMethod.Post, $"{BaseUri}/batch-orders") { Content = content };

                        try
                        {
                            var resp = await Client.SendAsync(req, HttpCompletionOption.ResponseContentRead);
                            sw.Stop();
                            var body = await resp.Content.ReadAsStringAsync();

                            if (resp.IsSuccessStatusCode)
                            {
                                batchLatencies.Add(sw.ElapsedTicks);
                                Interlocked.Add(ref totalOk, ordersInBatch);
                                Interlocked.Add(ref totalOrdersSubmitted, ordersInBatch);

                                try
                                {
                                    var json = System.Text.Json.JsonDocument.Parse(body);
                                    
                                    // Extract batch-level timing summary
                                    if (json.RootElement.TryGetProperty("timing_summary", out var ts))
                                    {
                                        if (ts.TryGetProperty("batch_total_e2e_us", out var be2e) && be2e.TryGetInt64(out var be2eVal))
                                            batchTimingE2e.Add(be2eVal);
                                        if (ts.TryGetProperty("avg_order_e2e_us", out var ao) && ao.TryGetInt64(out var aoVal))
                                            batchTimingAvgOrder.Add(aoVal);
                                    }

                                    // Extract per-order latencies from results
                                    if (json.RootElement.TryGetProperty("results", out var results))
                                    {
                                        foreach (var item in results.EnumerateArray())
                                        {
                                            if (item.TryGetProperty("status", out var st) && st.GetString() == "ok")
                                            {
                                                if (item.TryGetProperty("total_e2e_us", out var oe2e) && oe2e.TryGetInt64(out var oe2eVal))
                                                    perOrderLatencies.Add(oe2eVal);
                                            }
                                        }
                                    }
                                }
                                catch { }
                            }
                            else
                            {
                                Interlocked.Increment(ref totalFail);
                                if (totalFail <= 5)
                                    Console.WriteLine($"    [Batch {batchIdx}] FAIL {(int)resp.StatusCode}: {body[..Math.Min(100, body.Length)]}");
                            }
                            resp.Dispose();
                        }
                        catch (Exception ex)
                        {
                            sw.Stop();
                            Interlocked.Increment(ref totalFail);
                            if (totalFail <= 5) Console.WriteLine($"    [Batch {batchIdx}] ERR: {ex.Message}");
                        }
                        finally { req.Dispose(); }
                    }
                    finally { semaphore.Release(); }
                    // Pace batches to avoid rate limiting (sequential-ish)
                    await Task.Delay(300);
                });
            }

            await Task.WhenAll(tasks);

            var tickFactor = 1_000_000.0 / Stopwatch.Frequency;
            var clientLatenciesMs = batchLatencies.Select(t => (long)(t * tickFactor / 1000)).OrderBy(x => x).ToList();
            var orderLatenciesUs = perOrderLatencies.OrderBy(x => x).ToList();
            var batchE2eUs = batchTimingE2e.OrderBy(x => x).ToList();
            var batchAvgOrderUs = batchTimingAvgOrder.OrderBy(x => x).ToList();

            Console.WriteLine($"\n  [{testName}] Batches: {batchCount}/{batchCount} | Orders: {totalOk}/{totalOrdersSubmitted} | Failed batches: {totalFail}");

            // Client-perceived latency (per batch HTTP request)
            if (clientLatenciesMs.Count > 0)
            {
                var cP50 = clientLatenciesMs[clientLatenciesMs.Count / 2];
                var cP95 = clientLatenciesMs[(int)(clientLatenciesMs.Count * 0.95)];
                var cP99 = clientLatenciesMs[Math.Min((int)(clientLatenciesMs.Count * 0.99), clientLatenciesMs.Count - 1)];
                var cAvg = (long)clientLatenciesMs.Average();
                Console.WriteLine($"  [{testName}] Batch HTTP Latency: P50={cP50}ms | P95={cP95}ms | P99={cP99}ms | Avg={cAvg}ms");
            }

            // Per-order latency (individual orders within batches)
            if (orderLatenciesUs.Count > 0)
            {
                var oP50 = orderLatenciesUs[orderLatenciesUs.Count / 2];
                var oP95 = orderLatenciesUs[(int)(orderLatenciesUs.Count * 0.95)];
                var oP99 = orderLatenciesUs[Math.Min((int)(orderLatenciesUs.Count * 0.99), orderLatenciesUs.Count - 1)];
                var oAvg = (long)orderLatenciesUs.Average();
                Console.WriteLine($"  [{testName}] Per-Order E2E (μs): P50={oP50}μs | P95={oP95}μs | P99={oP99}μs | Avg={oAvg}μs");
                
                // Throughput calculation
                var ordersPerSecond = (double)orderLatenciesUs.Count / (orderLatenciesUs.Sum() / 1_000_000.0);
                Console.WriteLine($"  [{testName}] Effective Throughput: {ordersPerSecond:F0} orders/sec (engine-side)");
            }

            // Batch-level timing
            if (batchE2eUs.Count > 0)
            {
                var bP50 = batchE2eUs[batchE2eUs.Count / 2];
                var bP95 = batchE2eUs[(int)(batchE2eUs.Count * 0.95)];
                var bP99 = batchE2eUs[Math.Min((int)(batchE2eUs.Count * 0.99), batchE2eUs.Count - 1)];
                Console.WriteLine($"\n  {'─',-60}");
                Console.WriteLine($"  Batch-Level Timing (μs):");
                Console.WriteLine($"  {'─',-60}");
                Console.WriteLine($"    Batch Total E2E  P50={bP50,7}μs  P95={bP95,7}μs  P99={bP99,7}μs");
            }

            if (batchAvgOrderUs.Count > 0)
            {
                var baP50 = batchAvgOrderUs[batchAvgOrderUs.Count / 2];
                var baP95 = batchAvgOrderUs[(int)(batchAvgOrderUs.Count * 0.95)];
                var baP99 = batchAvgOrderUs[Math.Min((int)(batchAvgOrderUs.Count * 0.99), batchAvgOrderUs.Count - 1)];
                Console.WriteLine($"    Avg Order E2E    P50={baP50,7}μs  P95={baP95,7}μs  P99={baP99,7}μs");
            }

            // Comparison with single-order stress test
            Console.WriteLine($"\n  {'═',-60}");
            Console.WriteLine($"  vs Single-Order C=64 (from Phase 2):");
            Console.WriteLine($"    Single: P95=52ms (client), P95=4,331μs (server E2E)");
            if (clientLatenciesMs.Count > 0 && orderLatenciesUs.Count > 0)
            {
                var batchP95Ms = clientLatenciesMs[(int)(clientLatenciesMs.Count * 0.95)];
                var orderP95Us = orderLatenciesUs[(int)(orderLatenciesUs.Count * 0.95)];
                var throughputPerBatch = batchCount > 0 ? (double)totalOk / (batchLatencies.Sum() / Stopwatch.Frequency) : 0;
                Console.WriteLine($"    Batch:  P95={batchP95Ms}ms (per batch HTTP), P95={orderP95Us}μs (per order E2E)");
                Console.WriteLine($"    Batch throughput: {throughputPerBatch:F0} orders/sec");
            }
        }

        static async Task CancelReplacePressureTest(int orderCount, double cancelRatio, int concurrency)
        {
            var testName = "Cancel/Replace Pressure Test";
            Console.WriteLine($"\n{'=',-60}");
            Console.WriteLine($"  {testName}");
            Console.WriteLine($"  Orders={orderCount}, CancelRatio={cancelRatio:P0}, Concurrency={concurrency}");
            Console.WriteLine($"{'=',-60}");

            // Phase 1: Submit orders
            Console.WriteLine("\n  Phase 1: Submitting orders...");
            var submittedOrders = new ConcurrentBag<(string orderId, string userId, string market, int outcome)>();
            var submitLatencies = new ConcurrentBag<long>();
            int submitOk = 0, submitFail = 0;

            var semaphore = new SemaphoreSlim(concurrency);
            var submitTasks = new Task[orderCount];

            for (int i = 0; i < orderCount; i++)
            {
                int idx = i;
                submitTasks[i] = Task.Run(async () =>
                {
                    await semaphore.WaitAsync();
                    try
                    {
                        var userId = $"bm-cr-{idx % 50}";
                        var market = "btc-usdt";
                        var outcome = idx % 3;
                        var side = "buy"; // All buy orders - users have no positions to sell
                        var price = 50000 - 1000 - idx; // Below market to ensure fills

                        var req = BuildOrderRequest(userId, market, side, price, 1, outcome);
                        var sw = Stopwatch.StartNew();
                        try
                        {
                            var resp = await Client.SendAsync(req, HttpCompletionOption.ResponseHeadersRead);
                            sw.Stop();
                            var body = await resp.Content.ReadAsStringAsync();
                            if (resp.IsSuccessStatusCode)
                            {
                                Interlocked.Increment(ref submitOk);
                                submitLatencies.Add(sw.ElapsedTicks);
                                try
                                {
                                    var json = System.Text.Json.JsonDocument.Parse(body);
                                    if (json.RootElement.TryGetProperty("order_id", out var oid))
                                        submittedOrders.Add((oid.GetString()!, userId, market, outcome));
                                }
                                catch { }
                            }
                            else
                            {
                                Interlocked.Increment(ref submitFail);
                                if (submitFail <= 5)
                                    Console.WriteLine($"    [Submit {idx}] FAIL {(int)resp.StatusCode}: {body}");
                            }
                            resp.Dispose();
                        }
                        catch (Exception ex)
                        {
                            sw.Stop();
                            Interlocked.Increment(ref submitFail);
                            if (submitFail <= 5)
                                Console.WriteLine($"    [Submit {idx}] ERR: {ex.Message}");
                        }
                        finally { req.Dispose(); }
                    }
                    finally { semaphore.Release(); }
                });
            }

            await Task.WhenAll(submitTasks);
            PrintResults(submitLatencies.ToList(), submitOk, submitFail, orderCount, "Submit Phase");

            // Wait for rate limit window to reset before cancel/replace phase
            // IP limit is 60/window, submit phase consumes most of the budget
            Console.WriteLine("\n  Waiting for rate limit window to reset...");
            await Task.Delay(1500);

            // Phase 2: Cancel/Replace
            var ordersToModify = submittedOrders.Take((int)(submittedOrders.Count * cancelRatio)).ToList();
            if (ordersToModify.Count == 0)
            {
                Console.WriteLine("\n  No orders to cancel/replace (all may have matched or failed)");
                return;
            }

            Console.WriteLine($"\n  Phase 2: Cancelling/Replacing {ordersToModify.Count} orders...");
            var cancelReplaceLatencies = new ConcurrentBag<long>();
            int crOk = 0, crFail = 0;

            // Process sequentially to respect rate limits and measure per-operation latency accurately
            for (int i = 0; i < ordersToModify.Count; i++)
            {
                int idx = i;
                var order = ordersToModify[i];
                var sw = Stopwatch.StartNew();
                try
                {
                    HttpRequestMessage req;
                    if (idx % 2 == 0)
                    {
                        // Cancel
                        req = BuildCancelRequest(order.userId, order.market, order.orderId, order.outcome);
                    }
                    else
                    {
                        // Replace (adjust price)
                        var newPrice = 50000 + (idx % 200);
                        req = BuildReplaceRequest(order.userId, order.market, order.orderId, newPrice, null, order.outcome);
                    }

                    try
                    {
                        var resp = await Client.SendAsync(req, HttpCompletionOption.ResponseHeadersRead);
                        sw.Stop();
                        var body = await resp.Content.ReadAsStringAsync();
                        if (resp.IsSuccessStatusCode)
                        {
                            Interlocked.Increment(ref crOk);
                            cancelReplaceLatencies.Add(sw.ElapsedTicks);
                        }
                        else
                        {
                            Interlocked.Increment(ref crFail);
                            if (crFail <= 5)
                                Console.WriteLine($"    [{(idx % 2 == 0 ? "Cancel" : "Replace")} {idx}] FAIL {(int)resp.StatusCode}: {body}");
                        }
                        resp.Dispose();
                    }
                    catch (Exception ex)
                    {
                        sw.Stop();
                        Interlocked.Increment(ref crFail);
                        if (crFail <= 5)
                            Console.WriteLine($"    [{(idx % 2 == 0 ? "Cancel" : "Replace")} {idx}] ERR: {ex.Message}");
                    }
                    finally { req.Dispose(); }
                }
                finally
                {
                    // Small delay between requests to stay within rate limits
                    await Task.Delay(50);
                }
            }

            PrintResults(cancelReplaceLatencies.ToList(), crOk, crFail, ordersToModify.Count, "Cancel/Replace Phase");
        }

        static async Task RecoveryRestartTest()
        {
            var testName = "Recovery/Restart Test";
            Console.WriteLine($"\n{'=',-60}");
            Console.WriteLine($"  {testName}");
            Console.WriteLine($"{'=',-60}");

            // Step 1: Submit baseline orders
            Console.WriteLine("\n  Step 1: Submitting baseline orders...");
            int baselineCount = 50;
            var baselineLatencies = new List<long>();
            int baselineOk = 0, baselineFail = 0;

            for (int i = 0; i < baselineCount; i++)
            {
                var userId = $"bm-recovery-{i % 20}";
                var market = "btc-usdt";
                var outcome = i % 3;
                var side = "buy"; // All buy - no positions to sell
                var price = 50000 - 1000 - i; // Below market
                var req = BuildOrderRequest(userId, market, side, price, 1, outcome);

                var sw = Stopwatch.StartNew();
                try
                {
                    var resp = await Client.SendAsync(req, HttpCompletionOption.ResponseHeadersRead);
                    sw.Stop();
                    var body = await resp.Content.ReadAsStringAsync();
                    if (resp.IsSuccessStatusCode) { baselineOk++; baselineLatencies.Add(sw.ElapsedTicks); }
                    else baselineFail++;
                    resp.Dispose();
                }
                catch { sw.Stop(); baselineFail++; }
                finally { req.Dispose(); }
            }

            PrintResults(baselineLatencies, baselineOk, baselineFail, baselineCount, "Pre-Restart");

            // Step 2: Signal server restart
            Console.WriteLine("\n  Step 2: Please restart the server now.");
            Console.WriteLine("  Press ENTER when the server is back online...");
            Console.ReadLine();

            // Step 3: Verify health
            Console.WriteLine("\n  Step 3: Verifying server health...");
            try
            {
                var healthResp = await Client.GetAsync($"{BaseUri}/health");
                var healthBody = await healthResp.Content.ReadAsStringAsync();
                Console.WriteLine($"  Health: {healthBody}");
                healthResp.Dispose();
            }
            catch (Exception ex)
            {
                Console.WriteLine($"  Health check FAILED: {ex.Message}");
                return;
            }

            // Step 4: Post-restart latency test
            Console.WriteLine("\n  Step 4: Post-restart latency test...");
            var postRestartLatencies = new List<long>();
            int postOk = 0, postFail = 0;
            var postCount = 50;

            // Wait for warmup
            for (int i = 0; i < 5; i++)
            {
                var userId = $"bm-recovery-warmup-{i}";
                var req = BuildOrderRequest(userId, "btc-usdt", "buy", 50000, 1, 0);
                try { var resp = await Client.SendAsync(req); await resp.Content.ReadAsStringAsync(); resp.Dispose(); } catch { }
                req.Dispose();
            }

            for (int i = 0; i < postCount; i++)
            {
                var userId = $"bm-recovery-{i % 20}";
                var market = "btc-usdt";
                var outcome = i % 3;
                var side = "buy";
                var price = 50000 - 1000 - i;
                var req = BuildOrderRequest(userId, market, side, price, 1, outcome);

                var sw = Stopwatch.StartNew();
                try
                {
                    var resp = await Client.SendAsync(req, HttpCompletionOption.ResponseHeadersRead);
                    sw.Stop();
                    var body = await resp.Content.ReadAsStringAsync();
                    if (resp.IsSuccessStatusCode) { postOk++; postRestartLatencies.Add(sw.ElapsedTicks); }
                    else postFail++;
                    resp.Dispose();
                }
                catch { sw.Stop(); postFail++; }
                finally { req.Dispose(); }
            }

            PrintResults(postRestartLatencies, postOk, postFail, postCount, "Post-Restart");

            // Compare
            if (baselineLatencies.Count > 0 && postRestartLatencies.Count > 0)
            {
                var tickFactor = 1_000_000.0 / Stopwatch.Frequency;
                var preP50 = (long)(baselineLatencies[baselineLatencies.Count / 2] * tickFactor);
                var postP50 = (long)(postRestartLatencies[postRestartLatencies.Count / 2] * tickFactor);
                var preP99 = (long)(baselineLatencies[Math.Min((int)(baselineLatencies.Count * 0.99), baselineLatencies.Count - 1)] * tickFactor);
                var postP99 = (long)(postRestartLatencies[Math.Min((int)(postRestartLatencies.Count * 0.99), postRestartLatencies.Count - 1)] * tickFactor);

                Console.WriteLine($"\n  Comparison:");
                Console.WriteLine($"    P50: {preP50}μs (pre) -> {postP50}μs (post) | Δ{(postP50 - preP50):+0;-0}μs");
                Console.WriteLine($"    P99: {preP99}μs (pre) -> {postP99}μs (post) | Δ{(postP99 - preP99):+0;-0}μs");
                Console.WriteLine($"    Success Rate: {(double)baselineOk / baselineCount * 100:F0}% (pre) -> {(double)postOk / postCount * 100:F0}% (post)");

                if (postP99 < 15000)
                    Console.WriteLine($"  ✅ Recovery test PASSED (P99 < 15ms)");
                else
                    Console.WriteLine($"  ⚠️  Recovery test WARNING (P99={postP99}μs > 15ms threshold)");
            }
        }

        static async Task Main(string[] args)
        {
            var testType = args.Length > 0 ? args[0] : "all";
            var fundUsers = args.Contains("--fund");

            switch (testType.ToLowerInvariant())
            {
                case "stress":
                case "high-concurrency":
                    if (fundUsers) await FundUsers("bm-latency", 50, 10000000);
                    var stressCount = args.Where(a => !a.StartsWith("--")).Skip(1).FirstOrDefault() is string sc ? int.Parse(sc) : 200;
                    var stressConcurrency = args.Where(a => !a.StartsWith("--")).Skip(2).FirstOrDefault() is string sn ? int.Parse(sn) : 32;
                    await StressTest(stressCount, stressConcurrency, $"High-Concurrency Stress (C={stressConcurrency})");
                    break;

                case "batch":
                    if (fundUsers) await FundUsers("bm-batch", 50, 10000000);
                    var batchTotal = args.Where(a => !a.StartsWith("--")).Skip(1).FirstOrDefault() is string bt ? int.Parse(bt) : 200;
                    var batchSz = args.Where(a => !a.StartsWith("--")).Skip(2).FirstOrDefault() is string bs ? int.Parse(bs) : 10;
                    await BatchOrderTest(batchTotal, batchSz, $"Batch Order Test (B={batchSz})");
                    break;

                case "cancel":
                case "cancel-replace":
                    if (fundUsers) await FundUsers("bm-cr", 50, 10000000);
                    var orderCount = args.Where(a => !a.StartsWith("--")).Skip(1).FirstOrDefault() is string oc ? int.Parse(oc) : 100;
                    var cancelRatio = args.Where(a => !a.StartsWith("--")).Skip(2).FirstOrDefault() is string cr ? double.Parse(cr) : 0.5;
                    var crConcurrency = args.Where(a => !a.StartsWith("--")).Skip(3).FirstOrDefault() is string cc ? int.Parse(cc) : 16;
                    await CancelReplacePressureTest(orderCount, cancelRatio, crConcurrency);
                    break;

                case "recovery":
                case "restart":
                    if (fundUsers) await FundUsers("bm-recovery", 20, 10000000);
                    await RecoveryRestartTest();
                    break;

                case "all":
                default:
                    Console.WriteLine("╔══════════════════════════════════════════════════════════╗");
                    Console.WriteLine("║         Advanced Benchmark Suite — All Tests             ║");
                    Console.WriteLine("╚══════════════════════════════════════════════════════════╝");

                    // Fund test users
                    if (fundUsers)
                    {
                        await FundUsers("bm-latency", 50, 10000000);
                        await FundUsers("bm-cr", 50, 10000000);
                        await FundUsers("bm-recovery", 20, 10000000);
                    }

                    // Test 1: High-concurrency stress test
                    await StressTest(200, 32, "High-Concurrency Stress (C=32)");
                    await Task.Delay(2000); // Cool-down
                    await StressTest(400, 64, "High-Concurrency Stress (C=64)");

                    // Test 2: Cancel/Replace pressure
                    await Task.Delay(2000);
                    await CancelReplacePressureTest(orderCount: 100, cancelRatio: 0.5, concurrency: 16);

                    // Test 3: Recovery/Restart
                    await Task.Delay(2000);
                    await RecoveryRestartTest();
                    break;
            }
        }
    }
}
