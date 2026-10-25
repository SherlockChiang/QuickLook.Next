using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal static class UpdateChecker
{
    private static readonly Uri FeedUri = new("https://github.com/SherlockChiang/QuickLook.Next/releases/latest/download/update.json");
    private const int MaxResponseBytes = 16 * 1024;

    public static async Task<UpdateMetadata> CheckAsync(CancellationToken cancellationToken)
    {
        using var client = new HttpClient { Timeout = Timeout.InfiniteTimeSpan };
        return await CheckAsync(client, TimeSpan.FromSeconds(5), cancellationToken).ConfigureAwait(false);
    }

    internal static async Task<UpdateMetadata> CheckAsync(
        HttpClient client, TimeSpan timeout, CancellationToken cancellationToken)
    {
        using var deadline = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        deadline.CancelAfter(timeout);
        CancellationToken token = deadline.Token;
        using var request = new HttpRequestMessage(HttpMethod.Get, FeedUri);
        request.Headers.UserAgent.ParseAdd("QuickLook-Next-Update-Check");
        request.Headers.CacheControl = new System.Net.Http.Headers.CacheControlHeaderValue { NoCache = true };
        using HttpResponseMessage response = await client.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, token).ConfigureAwait(false);
        response.EnsureSuccessStatusCode();
        if (response.Content.Headers.ContentLength is > MaxResponseBytes)
            throw new InvalidDataException("Update metadata is too large.");
        await using Stream stream = await response.Content.ReadAsStreamAsync(token).ConfigureAwait(false);
        using var buffer = new MemoryStream(MaxResponseBytes);
        byte[] chunk = new byte[4096];
        while (true)
        {
            int read = await stream.ReadAsync(chunk, token).ConfigureAwait(false);
            if (read == 0) break;
            if (buffer.Length + read > MaxResponseBytes)
                throw new InvalidDataException("Update metadata is too large.");
            buffer.Write(chunk, 0, read);
        }
        token.ThrowIfCancellationRequested();
        UpdateMetadata metadata = UpdateMetadata.Parse(buffer.GetBuffer().AsMemory(0, checked((int)buffer.Length)));
        token.ThrowIfCancellationRequested();
        return metadata;
    }
}
