using System.Diagnostics;
using Tungsten.Core;

namespace Tungsten.Tests;

[Trait("Category", "Server")]
public sealed class ServerContractTests
{
    private sealed class LostReply(NoteClient client) : INoteService
    {
        public Uri Origin => client.Origin;
        public Task<Note> ReadAsync(string name, CancellationToken cancellation = default) => client.ReadAsync(name, cancellation);
        public async Task<Note> SaveAsync(Draft draft, CancellationToken cancellation = default)
        { await client.SaveAsync(draft, cancellation); throw new HttpRequestException("Acknowledgement was lost."); }
    }

    [Fact]
    public async Task NativeClientInteroperatesWithTheActualServerAndRecoversUncertainWrites()
    {
        var executable = Environment.GetEnvironmentVariable("TUNGSTEN_SERVER_PATH")
            ?? throw new InvalidOperationException("Run server contracts through just windows contracts.");
        var directory = Path.Combine(Path.GetTempPath(), "tungsten-contract-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(directory);
        var start = new ProcessStartInfo(executable) { RedirectStandardError = true, RedirectStandardOutput = true, UseShellExecute = false };
        start.ArgumentList.Add("--listen"); start.ArgumentList.Add("127.0.0.1:0");
        start.ArgumentList.Add("--data-dir"); start.ArgumentList.Add(Path.Combine(directory, "data"));
        using var server = Process.Start(start) ?? throw new InvalidOperationException("Server did not start.");
        try
        {
            string? address = null;
            using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(20));
            while (address is null)
            {
                var line = await server.StandardError.ReadLineAsync(deadline.Token)
                    ?? throw new InvalidOperationException("Server exited before listening.");
                if (line.StartsWith("operation=listen outcome=ready address=", StringComparison.Ordinal)) address = line.Split("address=", 2)[1];
            }
            using var client = new NoteClient(new("http://" + address + "/"));
            var origin = client.Origin;
            var first = await client.SaveAsync(new(1, origin.AbsoluteUri, "日本語/文章.md", "# First\nbody", null, null, null));
            Assert.Equal(first, await client.ReadAsync(first.Name));
            var changed = await client.SaveAsync(Draft.From(origin, first) with { Body = "# Changed\nbody" });
            Assert.Equal(412, (await Assert.ThrowsAsync<RemoteException>(() => client.SaveAsync(Draft.From(origin, first) with { Body = "stale" }))).Status);
            Assert.Single(await client.HistoryAsync(first.Name));
            var historical = (await client.HistoryAsync(first.Name))[0];
            Assert.Equal(first.Body, (await client.SavedAsync(historical)).Body);

            var target = await client.SaveAsync(new(1, origin.AbsoluteUri, "target.md", "# Target", null, null, null));
            changed = await client.SaveAsync(Draft.From(origin, changed) with { Body = "# Changed\n" + MarkdownLinks.Note(changed.Name, new(target.Name, "Target")) });
            Assert.Contains(target.Name, (await client.ConnectionsAsync(changed.Name)).Links);
            Assert.Contains(await client.BacklinksAsync(target.Name), note => note.Name == changed.Name);
            Assert.Single((await client.NotesAsync("Changed")).Notes);
            for (var i = 0; i < 50; i++)
                await client.SaveAsync(new(1, origin.AbsoluteUri, $"page-{i}.md", "# Paging", null, null, null));
            var page = await client.NotesAsync(); Assert.Equal(50, page.Notes.Length); Assert.NotNull(page.Next);
            var remaining = await client.NotesAsync(cursor: page.Next); Assert.Equal(2, remaining.Notes.Length);

            var workspace = await client.CreateWorkspaceAsync("Writing");
            workspace = await client.MembershipAsync(workspace.Id, changed.Name, true);
            Assert.Single((await client.WorkspaceNotesAsync(workspace.Id)).Notes);
            workspace = await client.RenameWorkspaceAsync(workspace, "Renamed"); Assert.Equal("Renamed", workspace.Title);
            Assert.Single((await client.WorkspacesAsync()).Workspaces);
            workspace = await client.MembershipAsync(workspace.Id, changed.Name, false);
            Assert.Empty((await client.WorkspaceNotesAsync(workspace.Id)).Notes); await client.DeleteWorkspaceAsync(workspace);

            var path = Path.Combine(directory, "original.txt"); var bytes = new byte[] { 0, 1, 127, 255 };
            await File.WriteAllBytesAsync(path, bytes); var attachment = await client.UploadAsync(path);
            Assert.Equal(attachment, await client.AttachmentAsync(attachment.Id));
            var downloaded = Path.Combine(directory, "downloaded.txt"); await client.DownloadAsync(attachment, downloaded);
            Assert.Equal(bytes, await File.ReadAllBytesAsync(downloaded)); await client.DeleteAttachmentAsync(attachment);

            await client.DeleteAsync(changed); var trash = await client.TrashAsync(); var record = Assert.Single(trash.Entries);
            Assert.Equal(changed.Body, (await client.SavedAsync(record, true)).Body);
            var restored = await client.RestoreAsync(record, "restored.md"); Assert.Equal(changed.Body, restored.Body);
            await client.EmptyTrashAsync(); Assert.Empty((await client.TrashAsync()).Entries);

            var store = new AtomicDraftStore(Path.Combine(directory, "drafts"));
            var editor = new EditorSession(Draft.From(origin, target), new LostReply(client), store);
            editor.Edit("committed without acknowledgement");
            await Assert.ThrowsAsync<HttpRequestException>(() => editor.SaveAsync());
            var recovered = new EditorSession(Assert.Single((await store.LoadAsync(origin)).Drafts), client, store);
            Assert.Equal(SaveBlock.Uncertain, recovered.Block); await recovered.RetryAsync();
            Assert.True(recovered.IsSaved); Assert.Equal("committed without acknowledgement", recovered.Current.Body);
        }
        finally
        {
            if (!server.HasExited) { server.Kill(true); await server.WaitForExitAsync(); }
            Directory.Delete(directory, true);
        }
    }
}
