using Tungsten.Core;

namespace Tungsten.Tests;

public sealed class DraftStoreTests : IDisposable
{
    private readonly string directory = Path.Combine(Path.GetTempPath(), "tungsten-tests-" + Guid.NewGuid().ToString("N"));
    private static readonly Uri Server = new("https://notes.example/");
    private static Draft Input(string body = "writing") => new(1, Server.AbsoluteUri, "note.md", body, null, null, null);

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public async Task AbortedDiscardPreparationKeepsBothVersionsAndReleasesTheWriter(bool replace)
    {
        var store = new AtomicDraftStore(directory);
        await store.WriteAsync(Input("backup")); await store.WriteAsync(Input("local"));
        var path = store.PathFor(Server, "note.md");
        var primary = await File.ReadAllBytesAsync(path); var backup = await File.ReadAllBytesAsync(path + ".previous");
        var change = await store.PrepareDiscardAsync(Input("local"), replace ? Input("remote") : null);
        Assert.Equal(primary, await File.ReadAllBytesAsync(path)); Assert.Equal(backup, await File.ReadAllBytesAsync(path + ".previous"));
        Assert.Equal("local", Assert.Single((await store.LoadAsync(Server)).Drafts).Body);
        var queued = store.WriteAsync(Input("later")); Assert.False(queued.IsCompleted);
        change.Dispose(); change.Dispose();
        await queued.WaitAsync(TimeSpan.FromSeconds(2));
        Assert.Throws<InvalidOperationException>(change.Commit);
        Assert.Equal("later", Assert.Single((await store.LoadAsync(Server)).Drafts).Body);
        Assert.Empty(Directory.EnumerateFiles(directory, "*.tmp"));
    }

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public async Task DiscardPublishesOnlyAtCommitAndCannotCommitTwice(bool replace)
    {
        var store = new AtomicDraftStore(directory);
        await store.WriteAsync(Input("backup")); await store.WriteAsync(Input("local"));
        using (var change = await store.PrepareDiscardAsync(Input("local"), replace ? Input("remote") : null))
        {
            Assert.Equal("local", Assert.Single((await store.LoadAsync(Server)).Drafts).Body);
            change.Commit(); Assert.Throws<InvalidOperationException>(change.Commit);
        }
        var recovered = await store.LoadAsync(Server);
        if (replace) Assert.Equal("remote", Assert.Single(recovered.Drafts).Body); else Assert.Empty(recovered.Drafts);
        Assert.Empty(Directory.EnumerateFiles(directory, "*.tmp"));
    }

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public async Task DiscardRejectsAReplacementFromAnotherServerOrNote(bool otherServer)
    {
        var store = new AtomicDraftStore(directory); await store.WriteAsync(Input());
        var replacement = otherServer ? Input() with { Server = "https://other.example/" } : Input() with { Name = "other.md" };
        await Assert.ThrowsAsync<InvalidDataException>(() => store.PrepareDiscardAsync(Input(), replacement));
        Assert.Equal(Input(), Assert.Single((await store.LoadAsync(Server)).Drafts));
        Assert.Empty(Directory.EnumerateFiles(directory, "*.tmp"));
    }

    private sealed class WindowsFactAttribute : FactAttribute
    { public WindowsFactAttribute() { if (!OperatingSystem.IsWindows()) Skip = "Windows file replacement requires native sharing checks."; } }

    [WindowsFact]
    public async Task FailedDiscardCommitKeepsTheCurrentFileAndCleansItsPreparedReplacement()
    {
        var store = new AtomicDraftStore(directory); await store.WriteAsync(Input("local"));
        var path = store.PathFor(Server, "note.md"); var original = await File.ReadAllBytesAsync(path);
        using (var change = await store.PrepareDiscardAsync(Input("local"), Input("remote")))
        {
            using var locked = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.None);
            Assert.Throws<IOException>(change.Commit);
        }
        Assert.Equal(original, await File.ReadAllBytesAsync(path));
        Assert.Empty(Directory.EnumerateFiles(directory, "*.tmp"));
        await store.WriteAsync(Input("later")).WaitAsync(TimeSpan.FromSeconds(2));
        Assert.Equal("later", Assert.Single((await store.LoadAsync(Server)).Drafts).Body);
    }

    [Fact]
    public async Task AnUnreadableCurrentDraftNeverRecoversAnOlderBackup()
    {
        var store = new AtomicDraftStore(directory); await store.WriteAsync(Input("backup")); await store.WriteAsync(Input("latest"));
        var path = store.PathFor(Server, "note.md");
        var latest = await File.ReadAllBytesAsync(path); var backup = await File.ReadAllBytesAsync(path + ".previous");
        using (var locked = new FileStream(path, FileMode.Open, FileAccess.ReadWrite, FileShare.None))
        {
            await Assert.ThrowsAsync<IOException>(() => store.LoadAsync(Server));
            await Assert.ThrowsAsync<IOException>(() => store.FindAsync(Server, "note.md"));
        }
        Assert.Equal(latest, await File.ReadAllBytesAsync(path)); Assert.Equal(backup, await File.ReadAllBytesAsync(path + ".previous"));
        Assert.Equal("latest", Assert.Single((await store.LoadAsync(Server)).Drafts).Body);
    }

    [Fact]
    public async Task LookingUpOneDraftIgnoresOtherFilesAndChecksItsIdentity()
    {
        var store = new AtomicDraftStore(directory); var other = new Uri("https://other.example/");
        await store.WriteAsync(Input()); await store.WriteAsync(Input("other") with { Server = other.AbsoluteUri });
        var unrelated = store.PathFor(other, "note.md");
        using (var locked = new FileStream(unrelated, FileMode.Open, FileAccess.ReadWrite, FileShare.None))
            Assert.Equal(Input(), await store.FindAsync(Server, "note.md"));
        var bytes = await File.ReadAllBytesAsync(unrelated);
        await File.WriteAllBytesAsync(store.PathFor(Server, "note.md"), bytes);
        Assert.Null(await store.FindAsync(Server, "note.md"));
        Assert.Equal(bytes, await File.ReadAllBytesAsync(store.PathFor(Server, "note.md")));
    }

    [Fact]
    public async Task AtomicReplacementRecoversLatestInputAndKeepsPreviousBytes()
    {
        var store = new AtomicDraftStore(directory);
        await store.WriteAsync(Input());
        var original = await File.ReadAllBytesAsync(store.PathFor(Server, "note.md"));
        await store.WriteAsync(Input("later"));
        Assert.Equal(original, await File.ReadAllBytesAsync(store.PathFor(Server, "note.md") + ".previous"));
        var recovered = await new AtomicDraftStore(directory).LoadAsync(Server);
        Assert.Equal("later", Assert.Single(recovered.Drafts).Body);
        Assert.Empty(recovered.DamagedFiles);
        Assert.Empty(Directory.EnumerateFiles(directory, "*.tmp"));
    }

    [Fact]
    public async Task MaximumLocalDraftRecoversAfterJsonEscaping()
    {
        var store = new AtomicDraftStore(directory);
        var draft = Input(new string('<', 8 * Rules.MaxNoteBytes));
        await store.WriteAsync(draft);
        Assert.True(new FileInfo(store.PathFor(Server, draft.Name)).Length > 32 * Rules.MaxNoteBytes);
        var recovered = await new AtomicDraftStore(directory).LoadAsync(Server);
        Assert.Empty(recovered.DamagedFiles);
        Assert.Equal(draft, Assert.Single(recovered.Drafts));
        await store.WriteAsync(Input("later"));
        Assert.Empty(Directory.EnumerateFiles(directory, "*.damaged-*"));
        File.Delete(store.PathFor(Server, draft.Name));
        Assert.Equal(draft.Body, Assert.Single((await store.LoadAsync(Server)).Drafts).Body);
    }

    [Fact]
    public async Task ServerAndNoteScopesNeverCollide()
    {
        var store = new AtomicDraftStore(directory); var other = new Uri("https://other.example/");
        Assert.NotEqual(store.PathFor(Server, "note.md"), store.PathFor(other, "note.md"));
        await store.WriteAsync(Input());
        await store.WriteAsync(Input("other") with { Server = other.AbsoluteUri });
        Assert.Equal("writing", Assert.Single((await store.LoadAsync(Server)).Drafts).Body);
        Assert.Equal("other", Assert.Single((await store.LoadAsync(other)).Drafts).Body);
    }

    [Fact]
    public async Task DamagedOriginalSurvivesWhileValidDraftsRecover()
    {
        var store = new AtomicDraftStore(directory); await store.WriteAsync(Input());
        var damaged = Path.Combine(directory, "damaged.json");
        await File.WriteAllTextAsync(damaged, "broken original");
        var result = await store.LoadAsync(Server);
        Assert.Single(result.Drafts); Assert.Equal(damaged, Assert.Single(result.DamagedFiles));
        Assert.Equal("broken original", await File.ReadAllTextAsync(damaged));
    }

    [Fact]
    public async Task IdentityMismatchAndCancellationPreserveExistingRecords()
    {
        var store = new AtomicDraftStore(directory); await store.WriteAsync(Input());
        var original = await File.ReadAllBytesAsync(store.PathFor(Server, "note.md"));
        using var canceled = new CancellationTokenSource(); canceled.Cancel();
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => store.WriteAsync(Input("later"), canceled.Token));
        Assert.Equal(original, await File.ReadAllBytesAsync(store.PathFor(Server, "note.md")));
        File.Move(store.PathFor(Server, "note.md"), Path.Combine(directory, "mismatch.json"));
        var result = await store.LoadAsync(Server);
        Assert.Empty(result.Drafts); Assert.Single(result.DamagedFiles);
    }

    [Fact]
    public async Task FailureToCreateDirectoryNeverClaimsPersistence()
    {
        Directory.CreateDirectory(directory);
        var blocked = Path.Combine(directory, "blocked"); await File.WriteAllTextAsync(blocked, "occupied");
        var store = new AtomicDraftStore(blocked);
        await Assert.ThrowsAnyAsync<IOException>(() => store.WriteAsync(Input()));
        Assert.Equal("occupied", await File.ReadAllTextAsync(blocked));
    }

    [Fact]
    public async Task ExplicitRemovalClearsOnlyTheSelectedDraftAndBackup()
    {
        var store = new AtomicDraftStore(directory); await store.WriteAsync(Input()); await store.WriteAsync(Input("later"));
        await store.WriteAsync(Input("keep") with { Name = "other.md" });
        await store.RemoveAsync(Input());
        Assert.False(File.Exists(store.PathFor(Server, "note.md") + ".previous"));
        Assert.Equal("keep", Assert.Single((await store.LoadAsync(Server)).Drafts).Body);
    }

    [Fact]
    public async Task DamagedOriginalSurvivesSubsequentWritesToTheSameNote()
    {
        var store = new AtomicDraftStore(directory);
        Directory.CreateDirectory(directory);
        var path = store.PathFor(Server, "note.md");
        await File.WriteAllTextAsync(path, "damaged original bytes");
        Assert.Single((await store.LoadAsync(Server)).DamagedFiles);
        await store.WriteAsync(Input("recovered"));
        await store.WriteAsync(Input("later"));
        Assert.Contains(Directory.EnumerateFiles(directory), file => File.ReadAllText(file) == "damaged original bytes");
        Assert.Equal("later", Assert.Single((await store.LoadAsync(Server)).Drafts).Body);
    }

    [Theory]
    [InlineData(true)]
    [InlineData(false)]
    public async Task AValidBackupRecoversWithoutAutomaticallySendingOlderText(bool damaged)
    {
        var store = new AtomicDraftStore(directory); await store.WriteAsync(Input("backup")); await store.WriteAsync(Input("latest"));
        var path = store.PathFor(Server, "note.md");
        if (damaged) await File.WriteAllTextAsync(path, "damaged latest bytes"); else File.Delete(path);
        var recovery = await store.LoadAsync(Server);
        var recovered = Assert.Single(recovery.Drafts);
        Assert.Equal(recovered, await store.FindAsync(Server, "note.md"));
        Assert.Equal("backup", recovered.Body); Assert.Equal(SaveBlock.Uncertain, recovered.Block);
        Assert.False(SavePolicy.CanSend(recovered));
        Assert.True(File.Exists(path + ".previous"));
        if (damaged)
        {
            Assert.Contains(path, recovery.DamagedFiles);
            Assert.Equal("damaged latest bytes", await File.ReadAllTextAsync(path));
        }
    }

    public void Dispose() { if (Directory.Exists(directory)) Directory.Delete(directory, true); }
}
