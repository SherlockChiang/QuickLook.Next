using System.Text.Json;
using Windows.Globalization;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed record AppSettings(int SchemaVersion = 1, string Language = "system", string Animation = "system")
{
    public const int CurrentSchemaVersion = 1;
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

        return Save(Current with { Language = language });
    }

    public static bool SaveAnimation(string animation)
    {
        if (animation is not ("system" or "always" or "still"))
            return false;
        return Save(Current with { Animation = animation });
    }

    private static bool Save(AppSettings updated)
    {
        try
        {
            Directory.CreateDirectory(SettingsDirectory);
            updated = updated with { SchemaVersion = CurrentSchemaVersion };
            string temporaryPath = SettingsPath + $".{Environment.ProcessId}.{Guid.NewGuid():N}.tmp";
            try
            {
                File.WriteAllText(temporaryPath, JsonSerializer.Serialize(updated, new JsonSerializerOptions { WriteIndented = true }));
                File.Move(temporaryPath, SettingsPath, overwrite: true);
            }
            finally
            {
                try { File.Delete(temporaryPath); } catch { }
            }
            Current = updated;
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
            AppSettings? settings = JsonSerializer.Deserialize<AppSettings>(File.ReadAllText(SettingsPath));
            if (settings is null
                || settings.SchemaVersion is < 1 or > CurrentSchemaVersion
                || settings.Language is not ("system" or "en-US" or "zh-CN")
                || settings.Animation is not ("system" or "always" or "still"))
            {
                PreserveInvalidSettings();
                return new AppSettings();
            }
            return settings;
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "settings load failed: " + ex.Message);
            PreserveInvalidSettings();
            return new AppSettings();
        }
    }

    private static void PreserveInvalidSettings()
    {
        try
        {
            if (File.Exists(SettingsPath))
                File.Move(SettingsPath, SettingsPath + ".invalid", overwrite: true);
        }
        catch
        {
        }
    }
}
