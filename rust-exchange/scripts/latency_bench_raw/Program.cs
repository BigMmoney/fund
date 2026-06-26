using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using System.Net.Sockets;
using System.Security.Cryptography;
using System.Text;
using System.Threading;

namespace LatencyBenchRaw
{
    class Program
    {
        static readonly string Host = "127.0.0.1";
        static readonly int Port = 3030;
        static readonly string Secret = "dev-secret-change-me";
        static int _seq;

        static void SendRaw(string userId, string side, int price, int amount, List<long> latencies, ref int ok, ref int fail)
        {
            var rid = $"bench-{Interlocked.Increment(ref _seq)}-{Guid.NewGuid():N}";
            var cid = $"co-{Guid.NewGuid():N}";
            var bodyJson = $"{{\"market_id\":\"btc-usdt\",\"side\":\"{side}\",\"price\":{price},\"amount\":{amount},\"outcome\":0,\"time_in_force\":\"GTC\",\"user_id\":\"{userId}\",\"client_order_id\":\"{cid}\"}}";
            var bodyBytes = Encoding.UTF8.GetBytes(bodyJson);

            var timestamp = DateTimeOffset.UtcNow.ToUnixTimeSeconds().ToString();
            using var sha = SHA256.Create();
            var bodyHash = BitConverter.ToString(sha.ComputeHash(bodyBytes)).Replace("-", "").ToLowerInvariant();
            using var hmac = new HMACSHA256(Encoding.UTF8.GetBytes(Secret));
            var payload = $"POST\n/intent\n\n{userId}\nuser\n\n{timestamp}\n{rid}";
            var signature = BitConverter.ToString(hmac.ComputeHash(Encoding.UTF8.GetBytes(payload))).Replace("-", "").ToLowerInvariant();

            var requestHead = $"POST /intent HTTP/1.1\r\nHost: localhost:3030\r\nContent-Type: application/json\r\nContent-Length: {bodyBytes.Length}\r\nx-internal-auth-subject: {userId}\r\nx-internal-auth-role: user\r\nx-internal-auth-session-id: \r\nx-internal-auth-timestamp: {timestamp}\r\nx-internal-auth-signature: {signature}\r\nx-internal-auth-body-sha256: {bodyHash}\r\nx-request-id: {rid}\r\nConnection: close\r\n\r\n";
            var headBytes = Encoding.ASCII.GetBytes(requestHead);

            using var tcp = new TcpClient();
            tcp.Connect(Host, Port);
            using var stream = tcp.GetStream();

            var sw = Stopwatch.StartNew();
            stream.Write(headBytes, 0, headBytes.Length);
            stream.Write(bodyBytes, 0, bodyBytes.Length);
            stream.Flush();

            // Read response headers
            var buf = new byte[8192];
            var totalRead = 0;
            while (totalRead < buf.Length)
            {
                var read = stream.Read(buf, totalRead, buf.Length - totalRead);
                if (read == 0) break;
                totalRead += read;
                // Look for \r\n\r\n
                for (int i = 3; i < totalRead; i++)
                {
                    if (buf[i - 3] == '\r' && buf[i - 2] == '\n' && buf[i - 1] == '\r' && buf[i] == '\n')
                    {
                        sw.Stop();
                        var headerStr = Encoding.ASCII.GetString(buf, 0, i - 3);
                        var firstLine = headerStr.Split(new[] { "\r\n" }, StringSplitOptions.None)[0];
                        var parts = firstLine.Split(' ');
                        if (parts.Length >= 2 && int.TryParse(parts[1], out var code) && code >= 200 && code < 300)
                        {
                            latencies.Add(sw.ElapsedTicks);
                            ok++;
                        }
                        else
                        {
                            fail++;
                        }
                        goto done;
                    }
                }
            }
            sw.Stop();
            fail++;
            done:;
        }

        static void Main(string[] args)
        {
            var count = args.Length > 0 ? int.Parse(args[0]) : 100;
            Console.WriteLine($"\n=== Raw TCP Latency Benchmark (N={count}) ===");

            var latencies = new List<long>();
            int ok = 0, fail = 0;

            // Warmup
            for (int i = 0; i < 3; i++)
            {
                SendRaw("warmup-user", "buy", 50000, 1, new List<long>(), ref ok, ref fail);
            }
            ok = 0; fail = 0; latencies.Clear();

            GC.Collect(); GC.WaitForPendingFinalizers(); GC.Collect();
            Thread.SpinWait(100000);

            for (int i = 0; i < count; i++)
            {
                var userId = $"bm-latency-{i % 50}";
                var side = "buy";
                var price = 50000 - (i % 100);

                try
                {
                    SendRaw(userId, side, price, 1, latencies, ref ok, ref fail);
                }
                catch (Exception ex)
                {
                    fail++;
                    if (fail <= 5) Console.WriteLine($"  [{i}] ERR: {ex.Message}");
                }
            }

            // Print results
            latencies.Sort();
            var freq = Stopwatch.Frequency;
            double ToUs(long ticks) => (double)ticks * 1_000_000 / freq;

            Console.WriteLine($"\n  Orders: {ok}/{count} ({ok * 100 / count}%) | Failed: {fail}");
            if (latencies.Count > 0)
            {
                Console.WriteLine($"  Latency: P50={(long)ToUs(latencies[latencies.Count / 2])}μs | P95={(long)ToUs(latencies[(int)(latencies.Count * 0.95)])}μs | P99={(long)ToUs(latencies[(int)(latencies.Count * 0.99)])}μs | Avg={(long)latencies.Average(l => ToUs(l))}μs | Min={(long)ToUs(latencies.First())}μs | Max={(long)ToUs(latencies.Last())}μs");

                // Histogram
                Console.WriteLine("\n  Distribution:");
                int bucketSize = 500;
                int maxBucket = (int)(ToUs(latencies.Last()) / bucketSize) + 1;
                var buckets = new int[maxBucket + 1];
                foreach (var l in latencies)
                {
                    var bucket = (int)(ToUs(l) / bucketSize);
                    if (bucket < buckets.Length) buckets[bucket]++;
                }
                int maxCount = buckets.Max();
                for (int i = 0; i < buckets.Length; i++)
                {
                    if (buckets[i] > 0 || i < 20)
                    {
                        var bars = new string('█', (buckets[i] * 30) / Math.Max(maxCount, 1));
                        Console.WriteLine($"  {i * bucketSize,6}-{(i + 1) * bucketSize,5}μs: {bars} {buckets[i]}");
                    }
                }
            }
        }
    }
}
