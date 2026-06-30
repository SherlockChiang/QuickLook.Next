using QuickLook.Next.Contracts;

namespace QuickLook.Next.Plugin.Info;

/// <summary>Universal fallback viewer: reports basic file facts. Handles any file (lowest priority).</summary>
public sealed class InfoProvider : IPreviewProvider
{
    public bool CanHandle(FileProbe probe) => true;

    public Task<PreviewResult> OpenAsync(string path, FileProbe probe, IPreviewContext context)
    {
        context.ReportStatus("InfoProvider: file metadata");
        // Metadata comes from the native probe — no re-stat here.
        var modified = DateTimeOffset.FromUnixTimeSeconds(probe.ModifiedUnix).LocalDateTime;
        string summary = $"{probe.Size:N0} bytes · {modified:g}";
        // Use the native Kind as the label (video/office/audio/binary…) so the status reads naturally.
        return Task.FromResult(new PreviewResult(probe.Kind, $"{Path.GetFileName(path)} — {summary}")
        {
            PreferredWidth = 640,
            PreferredHeight = 400,
        });
    }
}
