using System.Text.Json;
using Windows.Globalization;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed record AppSettings(string Language = "system")
{
    private static readonly string SettingsDirectory = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "QuickLook.Next");
    private static readonly string SettingsPath = Path.Combine(SettingsDirectory, "settings.json");

    public static AppSettings Current { get; private set; } = Load();

    public static void ApplyLanguage()
    {
        try
        {
            ApplicationLanguages.PrimaryLanguageOverride = Current.Language switch
            {
                "en-US" => "en-US",
                "zh-CN" => "zh-CN",
                _ => "",
            };
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "language preference failed: " + ex.Message);
        }
    }

    public static bool SaveLanguage(string language)
    {
        if (language is not ("system" or "en-US" or "zh-CN"))
            return false;

        try
        {
            Directory.CreateDirectory(SettingsDirectory);
            Current = Current with { Language = language };
            File.WriteAllText(SettingsPath, JsonSerializer.Serialize(Current, new JsonSerializerOptions { WriteIndented = true }));
            return true;
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "settings save failed: " + ex);
            return false;
        }
    }

    private static AppSettings Load()
    {
        try
        {
            if (!File.Exists(SettingsPath))
                return new AppSettings();
            return JsonSerializer.Deserialize<AppSettings>(File.ReadAllText(SettingsPath)) ?? new AppSettings();
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "settings load failed: " + ex.Message);
            return new AppSettings();
        }
    }
}
