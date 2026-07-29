using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal static class UpdateChecker
{
    private static readonly Uri FeedUri = new("https://github.com/SherlockChiang/QuickLook.Next/releases/latest/download/update.json");
    private const int MaxResponseBytes = 16 * 1024;

    public static async Task<UpdateMetadata> CheckAsync(CancellationToken cancellationToken)
    {
        using var client = new HttpClient { Timeout = TimeSpan.FromSeconds(5) };
        client.DefaultRequestHeaders.UserAgent.ParseAdd("QuickLook-Next-Update-Check");
        using var request = new HttpRequestMessage(HttpMethod.Get, FeedUri);
        request.Headers.CacheControl = new System.Net.Http.Headers.CacheControlHeaderValue { NoCache = true };
        using HttpResponseMessage response = await client.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, cancellationToken);
        response.EnsureSuccessStatusCode();
        if (response.Content.Headers.ContentLength is > MaxResponseBytes)
            throw new InvalidDataException("Update metadata is too large.");
        await using Stream stream = await response.Content.ReadAsStreamAsync(cancellationToken);
        using var buffer = new MemoryStream(MaxResponseBytes);
        byte[] chunk = new byte[4096];
        while (true)
        {
            int read = await stream.ReadAsync(chunk, cancellationToken);
            if (read == 0) break;
            if (buffer.Length + read > MaxResponseBytes)
                throw new InvalidDataException("Update metadata is too large.");
            buffer.Write(chunk, 0, read);
        }
        return UpdateMetadata.Parse(buffer.GetBuffer().AsMemory(0, checked((int)buffer.Length)));
    }
}
