using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class AppLanguagePolicyTests
{
    [Theory]
    [InlineData(AppLanguagePolicy.SystemLanguage, "")]
    [InlineData(AppLanguagePolicy.EnglishUnitedStates, "en-US")]
    [InlineData(AppLanguagePolicy.ChineseSimplified, "zh-CN")]
    [InlineData(AppLanguagePolicy.ChineseTraditional, "zh-TW")]
    public void Supported_modes_map_to_the_expected_primary_language_override(
        string languageMode,
        string expectedOverride)
    {
        Assert.True(AppLanguagePolicy.IsSupported(languageMode));
        Assert.Equal(expectedOverride, AppLanguagePolicy.ToPrimaryLanguageOverride(languageMode));
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("zh-tw")]
    [InlineData("fr-FR")]
    [InlineData(" zh-TW ")]
    public void Invalid_modes_are_rejected(string? languageMode)
    {
        Assert.False(AppLanguagePolicy.IsSupported(languageMode));
        Assert.Throws<ArgumentOutOfRangeException>(
            () => AppLanguagePolicy.ToPrimaryLanguageOverride(languageMode));
    }
}
