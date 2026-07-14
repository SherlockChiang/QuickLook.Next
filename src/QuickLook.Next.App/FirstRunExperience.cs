namespace QuickLook.Next.App;

internal static class FirstRunExperience
{
    private static readonly string MarkerPath = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "QuickLookNext",
        "welcome-shown-v1");

    public static bool ShouldShow => !File.Exists(MarkerPath);

    public static void MarkShown()
    {
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(MarkerPath)!);
            File.WriteAllText(MarkerPath, DateTimeOffset.UtcNow.ToString("O"));
        }
        catch
        {
            // A failed marker only causes the welcome page to appear again.
        }
    }
}
