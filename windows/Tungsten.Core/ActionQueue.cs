namespace Tungsten.Core;

public sealed class ActionQueue
{
    private readonly SemaphoreSlim pending = new(1);
    public bool IsBusy => pending.CurrentCount == 0;
    public async Task RunAsync(Func<Task> operation)
    {
        await pending.WaitAsync();
        try { await operation(); }
        finally { pending.Release(); }
    }
}
