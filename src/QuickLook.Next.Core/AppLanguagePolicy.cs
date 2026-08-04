namespace QuickLook.Next.Core;

public static class AppLanguagePolicy
{
    public const string SystemLanguage = "system";
    public const string EnglishUnitedStates = "en-US";
    public const string ChineseSimplified = "zh-CN";
    public const string ChineseTraditional = "zh-TW";

    public static bool IsSupported(string? languageMode)
        => languageMode is SystemLanguage
            or EnglishUnitedStates
            or ChineseSimplified
            or ChineseTraditional;

    public static string ToPrimaryLanguageOverride(string? languageMode)
        => languageMode switch
        {
            SystemLanguage => string.Empty,
            EnglishUnitedStates => EnglishUnitedStates,
            ChineseSimplified => ChineseSimplified,
            ChineseTraditional => ChineseTraditional,
            _ => throw new ArgumentOutOfRangeException(
                nameof(languageMode),
                languageMode,
                "Unsupported application language mode."),
        };
}
