using Tungsten.Core;

namespace Tungsten.Tests;

public sealed class ActionQueueTests
{
    [Fact]
    public async Task NavigationWaitsForAnActiveSaveAndRunsExactlyOnce()
    {
        var queue = new ActionQueue();
        var release = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var events = new List<string>();
        var saving = queue.RunAsync(async () => { events.Add("save"); await release.Task; events.Add("saved"); });
        var navigation = queue.RunAsync(() => { events.Add("navigate"); return Task.CompletedTask; });
        Assert.True(queue.IsBusy);
        Assert.False(navigation.IsCompleted);
        release.SetResult(); await Task.WhenAll(saving, navigation);
        Assert.Equal(["save", "saved", "navigate"], events);
        Assert.False(queue.IsBusy);
    }

    [Fact]
    public async Task AnOperationFailureDoesNotDropQueuedUserActions()
    {
        var queue = new ActionQueue();
        var release = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var failing = queue.RunAsync(async () => { await release.Task; throw new IOException("offline"); });
        var called = false;
        var next = queue.RunAsync(() => { called = true; return Task.CompletedTask; });
        release.SetResult(); await Assert.ThrowsAsync<IOException>(() => failing); await next;
        Assert.True(called); Assert.False(queue.IsBusy);
    }
}
