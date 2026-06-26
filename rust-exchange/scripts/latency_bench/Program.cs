using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using System.Net.Http;
using System.Security.Cryptography;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace LatencyBench
{
    class Program
    {
        static readonly string BaseUri = "http://localhost:3030";
        static readonly string Secret = "dev-secret-change-me";
        static readonly HttpClient Client;
        static int _seq;

        static Program()
        {
            var handler = new HttpClientHandler { UseCookies = false, MaxConnectionsPerServer = 100 };
            Client = new HttpClient(handler) { Timeout = TimeSpan.FromSeconds(10) };
            // Pre-warm the connection
            Client.GetAsync($"{BaseUri}/health").Result.Dispose();
        }

        static HttpRequestMessage BuildIntentRequest(string userId, string side, int price, int amount)
        {
            var rid = $"bench-{Interlocked.Increment(ref _seq)}-{Guid.NewGuid():N}";
            var cid = $"co-{Guid.NewGuid():N}";
            var bodyJson = $"{{\"market_id\":\"btc-usdt\",\"side\":\"{side}\",\"price\":{price},\"amount\":{amount},\"outcome\":0,\"time_in_force\":\"GTC\",\"user_id\":\"{userId}\",\"client_order_id\":\"{cid}\"}}";
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

        static byte[] SendRawSync(string userId, string side, int price, int amount, ref long elapsedUs)
        {
            var rid = $"bench-{Interlocked.Increment(ref _seq)}-{Guid.NewGuid():N}";
            var cid = $"co-{Guid.NewGuid():N}";
            var bodyJson = $"{{\"market_id\":\"btc-usdt\",\"side\":\"{side}\",\"price\":{price},\"amount\":{amount},\"outcome\":0,\"time_in_force\":\"GTC\",\"user_id\":\"{userId}\",\"client_order_id\":\"{cid}\"}}";
            var bodyBytes = Encoding.UTF8.GetBytes(bodyJson);

            var timestamp = DateTimeOffset.UtcNow.ToUnixTimeSeconds().ToString();
            using var sha = SHA256.Create();
            var bodyHash = ComputeHash(sha, bodyBytes);
            var payload = $"POST\n/intent\n\n{userId}\nuser\n\n{timestamp}\n{rid}";
            var signature = ComputeHmac(payload, Secret);

            var requestBody = $"POST /intent HTTP/1.1\r\nHost: localhost:3030\r\nContent-Type: application/json\r\nContent-Length: {bodyBytes.Length}\r\nx-internal-auth-subject: {userId}\r\nx-internal-auth-role: user\r\nx-internal-auth-session-id: \r\nx-internal-auth-timestamp: {timestamp}\r\nx-internal-auth-signature: {signature}\r\nx-internal-auth-body-sha256: {bodyHash}\r\nx-request-id: {rid}\r\nConnection: keep-alive\r\n\r\n";
            var requestHead = Encoding.ASCII.GetBytes(requestBody);

            var sw = Stopwatch.StartNew();
            using var tcp = new System.Net.Sockets.TcpClient();
            tcp.Connect("127.0.0.1", 3030);
            using var stream = tcp.GetStream();
            stream.Write(requestHead, 0, requestHead.Length);
            stream.Write(bodyBytes, 0, bodyBytes.Length);
            stream.Flush();

            // Read response
            var responseBuf = new byte[4096];
            var totalRead = 0;
            int read;
            while ((read = stream.Read(responseBuf, totalRead, responseBuf.Length - totalRead)) > 0)
            {
                totalRead += read;
                if (totalRead >= responseBuf.Length - 1) break;
                // Check if we got the full response (look for double CRLF + content)
                var respStr = Encoding.ASCII.GetString(responseBuf, 0, totalRead);
                var headerEnd = respStr.IndexOf("\r\n\r\n");
                if (headerEnd >= 0)
                {
                    var headerPart = respStr.Substring(0, headerEnd);
                    // Try to extract Content-Length
                    var clIdx = headerPart.LastIndexOf("Content-Length:", StringComparison.OrdinalIgnoreCase);
                    if (clIdx >= 0)
                    {
                        var clEnd = headerPart.IndexOf('\r', clIdx);
                        if (clEnd > 0 && int.TryParse(headerPart.Substring(clIdx + 15, clEnd - clIdx - 15).Trim(), out var contentLen))
                        {
                            var bodyStart = headerEnd + 4;
                            var bodyReceived = totalRead - bodyStart;
                            if (bodyReceived >= contentLen) break;
                        }
                        else break; // Can't parse, assume done
                    }
                    else break; // No Content-Length, assume done
                }
            }
            sw.Stop();
            elapsedUs = sw.ElapsedTicks * 1_000_000 / Stopwatch.Frequency;
            return responseBuf.Take(totalRead).ToArray();
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

        static async Task Main(string[] args)
        {
            var mode = args.Length > 0 ? args[0] : "sequential";
            var count = args.Length > 1 ? int.Parse(args[1]) : 100;
            var concurrency = args.Length > 2 ? int.Parse(args[2]) : 1;

            Console.WriteLine($"\n=== Latency Benchmark ({mode}, N={count}, C={concurrency}) ===");

            if (mode == "sequential")
            {
                var latencies = new List<long>();
                int ok = 0, fail = 0;

                // Pre-build all requests outside timing loop
                var requests = new HttpRequestMessage[count];
                for (int i = 0; i < count; i++)
                {
                    var userId = $"bm-latency-{i % 50}";
                    var side = "buy";
                    var price = 50000 - (i % 100);
                    requests[i] = BuildIntentRequest(userId, side, price, 1);
                }

                // Warmup
                for (int i = 0; i < 3; i++)
                {
                    var req = BuildIntentRequest("warmup-user", "buy", 50000, 1);
                    try { var resp = await Client.SendAsync(req); await resp.Content.ReadAsStringAsync(); } catch { }
                    req.Dispose();
                }

                GC.Collect(); GC.WaitForPendingFinalizers(); GC.Collect();
                Thread.SpinWait(100000); // Let GC settle

                for (int i = 0; i < count; i++)
                {
                    var req = requests[i];

                    // Measure ONLY network round-trip (headers received, not body)
                    var sw = Stopwatch.StartNew();
                    try
                    {
                        var resp = await Client.SendAsync(req, HttpCompletionOption.ResponseHeadersRead).ConfigureAwait(false);
                        sw.Stop();
                        var statusCode = (int)resp.StatusCode;
                        // Consume body outside timing
                        try { await resp.Content.ReadAsStringAsync().ConfigureAwait(false); } catch { }
                        if (resp.IsSuccessStatusCode) { ok++; latencies.Add(sw.ElapsedTicks); }
                        else { fail++; if (fail <= 5) Console.WriteLine($"  [{i}] FAIL {statusCode}"); }
                        resp.Dispose();
                    }
                    catch (Exception ex)
                    {
                        sw.Stop();
                        fail++;
                        if (fail <= 5) Console.WriteLine($"  [{i}] ERR: {ex.Message}");
                    }
                    finally { req.Dispose(); }
                }

                PrintResults(latencies, ok, fail, count);
            }
            else if (mode == "concurrent")
            {
                var latencies = new List<long>();
                var lockObj = new object();
                int ok = 0, fail = 0;

                var tasks = new Task[count];
                for (int i = 0; i < count; i++)
                {
                    int idx = i;
                    tasks[i] = Task.Run(async () =>
                    {
                        var userId = $"bm-latency-{idx % 50}";
                        var side = "buy";
                        var price = 50000 - (idx % 100);
                        var req = BuildIntentRequest(userId, side, price, 1);

                        var sw = Stopwatch.StartNew();
                        try
                        {
                            var resp = await Client.SendAsync(req);
                            sw.Stop();
                            var body = await resp.Content.ReadAsStringAsync();
                            if (resp.IsSuccessStatusCode)
                            {
                                lock (lockObj) { ok++; latencies.Add(sw.ElapsedTicks); }
                            }
                            else
                            {
                                lock (lockObj) { fail++; }
                            }
                        }
                        catch { sw.Stop(); lock (lockObj) { fail++; } }
                        finally { req.Dispose(); }
                    });
                }
                await Task.WhenAll(tasks);

                PrintResults(latencies, ok, fail, count);
            }
        }

        static void PrintResults(List<long> latenciesTicks, int ok, int fail, int total)
        {
            latenciesTicks.Sort();
            if (latenciesTicks.Count == 0)
            {
                Console.WriteLine($"\n  Orders: 0/{total} (0%) | Failed: {fail}");
                return;
            }

            var tickFactor = 1_000_000.0 / Stopwatch.Frequency; // ticks → microseconds
            var toUs = (long t) => (long)(t * tickFactor);

            var p50 = toUs(latenciesTicks[latenciesTicks.Count / 2]);
            var p95 = toUs(latenciesTicks[Math.Min((int)(latenciesTicks.Count * 0.95), latenciesTicks.Count - 1)]);
            var p99 = toUs(latenciesTicks[Math.Min((int)(latenciesTicks.Count * 0.99), latenciesTicks.Count - 1)]);
            var avg = latenciesTicks.Average() * tickFactor;
            var min = toUs(latenciesTicks.First());
            var max = toUs(latenciesTicks.Last());

            Console.WriteLine($"\n  Orders: {ok}/{total} ({(double)ok / total * 100:F0}%) | Failed: {fail}");
            Console.WriteLine($"  Latency: P50={p50}μs | P95={p95}μs | P99={p99}μs | Avg={avg:F0}μs | Min={min}μs | Max={max}μs");

            // Histogram with 500μs buckets
            Console.WriteLine("\n  Distribution:");
            var bucketSize = 500L; // 500μs buckets
            var maxBucket = (max / bucketSize + 1) * bucketSize;
            for (long b = 0; b <= maxBucket; b += bucketSize)
            {
                var upper = b + bucketSize;
                var count = latenciesTicks.Count(t => { var us = toUs(t); return us >= b && us < upper; });
                if (count == 0 && b > p99) break;
                var bar = new string('█', (int)((double)count / latenciesTicks.Count * 60));
                Console.WriteLine($"    {b,5}-{upper,5}μs: {bar} {count}");
            }
        }
    }
}
