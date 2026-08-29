using Xunit;

namespace QuickLook.Next.App;

public sealed class DuplicateOpenCloseGuardTests
{
    private const int SettleWindowMs = 750;

    [Fact]
    public void SamePathWithinTheSettleWindowIsIgnored()
    {
        var guard = new DuplicateOpenCloseGuard(SettleWindowMs);
        guard.NoteReveal(@"C:\a.png", tick: 1_000);

        Assert.True(guard.ShouldIgnoreToggleClose(@"C:\a.png", tick: 1_000 + SettleWindowMs - 1));
    }

    [Fact]
    public void SamePathAfterTheSettleWindowStillToggles()
    {
        var guard = new DuplicateOpenCloseGuard(SettleWindowMs);
        guard.NoteReveal(@"C:\a.png", tick: 1_000);

        Assert.False(guard.ShouldIgnoreToggleClose(@"C:\a.png", tick: 1_000 + SettleWindowMs));
    }

    [Fact]
    public void DifferentPathIsNeverIgnored()
    {
        var guard = new DuplicateOpenCloseGuard(SettleWindowMs);
        guard.NoteReveal(@"C:\a.png", tick: 1_000);

        Assert.False(guard.ShouldIgnoreToggleClose(@"C:\b.png", tick: 1_010));
        Assert.False(guard.ShouldIgnoreToggleClose(null, tick: 1_010));
        Assert.False(guard.ShouldIgnoreToggleClose("", tick: 1_010));
    }

    [Fact]
    public void PathComparisonIgnoresCase()
    {
        var guard = new DuplicateOpenCloseGuard(SettleWindowMs);
        guard.NoteReveal(@"C:\A.PNG", tick: 1_000);

        Assert.True(guard.ShouldIgnoreToggleClose(@"C:\a.png", tick: 1_005));
    }

    [Fact]
    public void ClearRemovesTheLatchImmediately()
    {
        var guard = new DuplicateOpenCloseGuard(SettleWindowMs);
        guard.NoteReveal(@"C:\a.png", tick: 1_000);
        guard.Clear();

        Assert.False(guard.ShouldIgnoreToggleClose(@"C:\a.png", tick: 1_001));
    }

    [Fact]
    public void ARevealForAnotherPathReplacesTheLatch()
    {
        var guard = new DuplicateOpenCloseGuard(SettleWindowMs);
        guard.NoteReveal(@"C:\a.png", tick: 1_000);
        guard.NoteReveal(@"C:\b.png", tick: 1_050);

        Assert.False(guard.ShouldIgnoreToggleClose(@"C:\a.png", tick: 1_060));
        Assert.True(guard.ShouldIgnoreToggleClose(@"C:\b.png", tick: 1_060));
    }
}
