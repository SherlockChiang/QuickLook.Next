namespace QuickLook.Next.App;

internal static class PreviewContentCompletion
{
    // Theme/preferences can replace rendering within the same preview generation.
    // Observe every task but only commit the latest task of a still-current preview.
    public static async Task<bool> WaitAsync(
        Func<Task> currentRender, Func<bool> isCurrent, CancellationToken token)
    {
        while (isCurrent())
        {
            Task render = currentRender();
            try { await render.WaitAsync(token); }
            catch (Exception) when (!isCurrent() || !ReferenceEquals(render, currentRender()))
            {
                continue;
            }
            if (isCurrent() && ReferenceEquals(render, currentRender()))
                return true;
        }
        return false;
    }
}
