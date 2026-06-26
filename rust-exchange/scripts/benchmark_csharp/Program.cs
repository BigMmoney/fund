using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using System.Net.Http;
using System.Security.Cryptography;
using System.Text;
using System.Threading.Tasks;

namespace BenchmarkV5
{
    class Program
    {
        static readonly string BaseUri = "http://localhost:3030";
        static readonly string Secret = "dev-secret-change-me";
        static readonly string Instrument = "btc-usdt";
        static readonly HttpClient Client;

        static Program()
        {
            var handler = new HttpClientHandler { UseCookies = false };
            Client = new HttpClient(handler) { Timeout = TimeSpan.FromSeconds(10) };
        }

        static string ComputeHmac(string message, string secret)
        {
            using var hmac = new HMACSHA256(Encoding.UTF8.GetBytes(secret));
            var hash = hmac.ComputeHash(Encoding.UTF8.GetBytes(message));
            return BitConverter.ToString(hash).Replace("-", "").ToLowerInvariant();
        }

        static string ComputeBodyHash(byte[] bodyBytes)
        {
            using var sha = SHA256.Create();
            var hash = sha.ComputeHash(bodyBytes);
            return BitConverter.ToString(hash).Replace("-", "").ToLowerInvariant();
        }

        static HttpRequestMessage BuildRequest(string path, string subject, string role, string requestId, byte[] bodyBytes)
        {
            var timestamp = DateTimeOffset.UtcNow.ToUnixTimeSeconds().ToString();
            var bodyHash = ComputeBodyHash(bodyBytes);
            var payload = $"POST\n{path}\n\n{subject}\n{role}\n\n{timestamp}\n{requestId}";
            var signature = ComputeHmac(payload, Secret);

            var content = new ByteArrayContent(bodyBytes);
            content.Headers.ContentType = new System.Net.Http.Headers.MediaTypeHeaderValue("application/json");
            content.Headers.TryAddWithoutValidation("x-internal-auth-subject", subject);
            content.Headers.TryAddWithoutValidation("x-internal-auth-role", role);
            content.Headers.TryAddWithoutValidation("x-internal-auth-session-id", "");
            content.Headers.TryAddWithoutValidation("x-internal-auth-timestamp", timestamp);
            content.Headers.TryAddWithoutValidation("x-internal-auth-signature", signature);
            content.Headers.TryAddWithoutValidation("x-internal-auth-body-sha256", bodyHash);
            content.Headers.TryAddWithoutValidation("x-request-id", requestId);

            var request = new HttpRequestMessage(HttpMethod.Post, $"{BaseUri}{path}") { Content = content };
            return request;
        }

        static (bool ok, long ms, string body) SendOrder(string userId, int index)
        {
            var side = index % 2 == 0 ? "buy" : "sell";
            var price = side == "buy" ? 50000 + (index % 5) * 100 : 50000 - (index % 5) * 100;
            var requestId = $"cs-{Guid.NewGuid():N}";
            var clientOrderId = $"co-{Guid.NewGuid():N}";
            var bodyJson = $"{{\"market_id\":\"{Instrument}\",\"side\":\"{side}\",\"price\":{price},\"amount\":1,\"outcome\":0,\"time_in_force\":\"GTC\",\"user_id\":\"{userId}\",\"client_order_id\":\"{clientOrderId}\"}}";
            var bodyBytes = Encoding.UTF8.GetBytes(bodyJson);

            var request = BuildRequest("/intent", userId, "user", requestId, bodyBytes);

            var sw = Stopwatch.StartNew();
            try
            {
                var response = Client.SendAsync(request).GetAwaiter().GetResult();
                sw.Stop();
                var respBody = response.Content.ReadAsStringAsync().GetAwaiter().GetResult();
                return (response.IsSuccessStatusCode, sw.ElapsedMilliseconds, respBody);
            }
            catch (Exception ex)
            {
                sw.Stop();
                return (false, sw.ElapsedMilliseconds, ex.Message);
            }
            finally
            {
                request.Dispose();
            }
        }

        static async Task FundAccount(string userId, int cashAmount, int posAmount)
        {
            // Cash deposit
            var opId1 = Guid.NewGuid().ToString("N");
            var cashJson = $"{{\"user_id\":\"{userId}\",\"amount\":{cashAmount},\"op_id\":\"{opId1}\"}}";
            var cashBytes = Encoding.UTF8.GetBytes(cashJson);
            var req1 = BuildRequest("/deposit", "admin", "admin", opId1, cashBytes);
            try { var resp1 = await Client.SendAsync(req1); /* ignore errors */ req1.Dispose(); await Task.Delay(50); } catch { }

            // Position deposit (for sell orders)
            var opId2 = Guid.NewGuid().ToString("N");
            var posJson = $"{{\"user_id\":\"{userId}\",\"market_id\":\"{Instrument}\",\"outcome\":0,\"amount\":{posAmount},\"op_id\":\"{opId2}\"}}";
            var posBytes = Encoding.UTF8.GetBytes(posJson);
            var req2 = BuildRequest("/position-deposit", "admin", "admin", opId2, posBytes);
            try { var resp2 = await Client.SendAsync(req2); /* ignore errors */ req2.Dispose(); await Task.Delay(50); } catch { }
        }

        static async Task Main(string[] args)
        {
            var mode = args.Length > 0 ? args[0] : "quick";
            var concurrency = args.Length > 1 ? int.Parse(args[1]) : 10;

            if (mode == "quick")
            {
                Console.WriteLine("\n=== Quick Test (C# HttpClient) ===");
                
                // Fund accounts sequentially to avoid rate limiting (429)
                Console.WriteLine("  Funding 20 accounts...");
                for (int i = 0; i < 20; i++)
                {
                    await FundAccount($"bm-csharp-{i}", 100000, 1000);
                }
                Console.WriteLine("  Done.");

                // Send orders
                var latencies = new List<long>();
                int success = 0, failed = 0;

                for (int i = 0; i < concurrency; i++)
                {
                    var userId = $"bm-csharp-{i % 20}";
                    var (ok, ms, body) = SendOrder(userId, i);
                    latencies.Add(ms);
                    if (ok) success++; else { failed++; Console.WriteLine($"    FAIL [{i}]: {ms}ms - {body.Substring(0, Math.Min(80, body.Length))}"); }
                }

                latencies.Sort();
                var p50 = latencies[latencies.Count / 2];
                var p95 = latencies[Math.Min((int)(latencies.Count * 0.95), latencies.Count - 1)];
                var p99 = latencies[Math.Min((int)(latencies.Count * 0.99), latencies.Count - 1)];
                var avg = latencies.Average();

                Console.WriteLine($"\n  Orders: {success}/{concurrency} ({(double)success/concurrency*100:F0}%) | Failed: {failed}");
                Console.WriteLine($"  Latency: P50={p50}ms | P95={p95}ms | P99={p99}ms | Avg={avg:F1}ms | Min={latencies.First()}ms | Max={latencies.Last()}ms");
            }
            else if (mode == "sweep")
            {
                Console.WriteLine("\n=== Concurrency Sweep (C# HttpClient) ===");
                
                // Fund accounts sequentially to avoid rate limiting (429)
                Console.WriteLine("  Funding 32 accounts...");
                for (int i = 0; i < 32; i++)
                {
                    await FundAccount($"bm-cs-{i}", 100000, 1000);
                }
                Console.WriteLine("  Done.");

                int[] levels = { 1, 2, 4, 8, 16, 32 };
                int ordersPerLevel = 50;

                Console.WriteLine($"\n  {(string.Format("{0,-4} {1,-6} {2,-6} {3,-6} {4,-7} {5,-7} {6,-7} {7,-6}", "C", "Sent", "OK", "Fail", "P50", "P95", "P99", "ops/s"))}");
                Console.WriteLine($"  {"---", -4} {"------", -6} {"------", -6} {"------", -6} {"-------", -7} {"-------", -7} {"-------", -7} {"------", -6}");

                foreach (var c in levels)
                {
                    var allLatencies = new ConcurrentBag<long>();
                    int sent = 0, ok = 0, fail = 0;

                    var sw = Stopwatch.StartNew();
                    var tasks = new List<Task>();
                    for (int i = 0; i < ordersPerLevel; i++)
                    {
                        int idx = i;
                        tasks.Add(Task.Run(() =>
                        {
                            var userId = $"bm-cs-{idx % 32}";
                            var (success, ms, _) = SendOrder(userId, idx);
                            allLatencies.Add(ms);
                            if (success) Interlocked.Increment(ref ok); else Interlocked.Increment(ref fail);
                            Interlocked.Increment(ref sent);
                        }));
                    }
                    Task.WaitAll(tasks.ToArray());
                    sw.Stop();

                    var sorted = allLatencies.OrderBy(x => x).ToList();
                    var p50 = sorted[sorted.Count / 2];
                    var p95 = sorted[Math.Min((int)(sorted.Count * 0.95), sorted.Count - 1)];
                    var p99 = sorted[Math.Min((int)(sorted.Count * 0.99), sorted.Count - 1)];
                    var opsSec = sorted.Count / sw.Elapsed.TotalSeconds;

                    Console.WriteLine($"  {c, -4} {sent, -6} {ok, -6} {fail, -6} {p50, -7} {p95, -7} {p99, -7} {opsSec, -6:F0}");
                }
            }
        }
    }
}
