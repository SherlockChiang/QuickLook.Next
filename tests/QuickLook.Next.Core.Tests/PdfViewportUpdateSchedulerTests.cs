using QuickLook.Next.App;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class PdfViewportUpdateSchedulerTests
{
    [Fact]
    public void Repeated_events_share_one_update_using_the_latest_viewport()
    {
        var callbacks = new Queue<Action>();
        int viewport = 0;
        var renderedViewports = new List<int>();
        var scheduler = new PdfViewportUpdateScheduler(
            callback => { callbacks.Enqueue(callback); return true; },
            () => renderedViewports.Add(viewport));

        for (viewport = 0; viewport < 100; viewport++)
            scheduler.Queue();

        Assert.Single(callbacks);
        callbacks.Dequeue()();
        Assert.Equal([100], renderedViewports);

        viewport = 200;
        scheduler.Queue();
        Assert.Single(callbacks);
        callbacks.Dequeue()();
        Assert.Equal([100, 200], renderedViewports);
    }

    [Fact]
    public void Reset_discards_a_closed_previews_queued_callback()
    {
        var callbacks = new Queue<Action>();
        int updates = 0;
        var scheduler = new PdfViewportUpdateScheduler(
            callback => { callbacks.Enqueue(callback); return true; },
            () => updates++);

        scheduler.Queue();
        scheduler.Reset();
        callbacks.Dequeue()();

        Assert.Equal(0, updates);
        Assert.Empty(callbacks);
    }

    [Fact]
    public void Old_callback_cannot_clear_or_execute_a_reopened_previews_update()
    {
        var callbacks = new Queue<Action>();
        int updates = 0;
        var scheduler = new PdfViewportUpdateScheduler(
            callback => { callbacks.Enqueue(callback); return true; },
            () => updates++);

        scheduler.Queue();
        Action oldCallback = callbacks.Dequeue();
        scheduler.Reset();
        scheduler.Queue();
        Action newCallback = callbacks.Dequeue();

        newCallback();
        Assert.Equal(1, updates);
        oldCallback();
        scheduler.Queue();
        Assert.Single(callbacks);
        callbacks.Dequeue()();
        Assert.Equal(2, updates);
    }

    [Fact]
    public void Immediate_update_invalidates_queued_work_without_blocking_later_events()
    {
        var callbacks = new Queue<Action>();
        int updates = 0;
        var scheduler = new PdfViewportUpdateScheduler(
            callback => { callbacks.Enqueue(callback); return true; },
            () => updates++);

        scheduler.Queue();
        scheduler.Reset();
        updates++;
        callbacks.Dequeue()();
        Assert.Equal(1, updates);

        scheduler.Queue();
        callbacks.Dequeue()();
        Assert.Equal(2, updates);
    }

    [Fact]
    public void Rejected_enqueue_does_not_prevent_a_later_update()
    {
        var callbacks = new Queue<Action>();
        bool accept = false;
        int updates = 0;
        var scheduler = new PdfViewportUpdateScheduler(
            callback =>
            {
                if (!accept)
                    return false;
                callbacks.Enqueue(callback);
                return true;
            },
            () => updates++);

        scheduler.Queue();
        Assert.Empty(callbacks);
        accept = true;
        scheduler.Queue();
        callbacks.Dequeue()();
        Assert.Equal(1, updates);
    }

    [Fact]
    public void Update_can_queue_a_followup_for_layout_changes()
    {
        var callbacks = new Queue<Action>();
        int updates = 0;
        PdfViewportUpdateScheduler? scheduler = null;
        scheduler = new PdfViewportUpdateScheduler(
            callback => { callbacks.Enqueue(callback); return true; },
            () =>
            {
                updates++;
                if (updates == 1)
                    scheduler!.Queue();
            });

        scheduler.Queue();
        callbacks.Dequeue()();
        Assert.Single(callbacks);
        callbacks.Dequeue()();
        Assert.Equal(2, updates);
        Assert.Empty(callbacks);
    }
}
