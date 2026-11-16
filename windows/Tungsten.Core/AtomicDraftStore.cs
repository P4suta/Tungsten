using System.Security.Cryptography;
using System.Text.Json;

namespace Tungsten.Core;

public sealed record DraftRecovery(Draft[] Drafts, string[] DamagedFiles);

public sealed class AtomicDraftStore(string directory) : IDraftRepository
{
    private const long MaxSerializedDraftBytes = 64L * Rules.MaxNoteBytes;
    private readonly SemaphoreSlim writes = new(1);
    private static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web);
    public string DirectoryPath { get; } = Path.GetFullPath(directory);
    public string PathFor(Uri server, string name)
    {
        Rules.NoteName(name);
        return Path.Combine(DirectoryPath, Convert.ToHexString(SHA256.HashData(Rules.Utf8.GetBytes(server.AbsoluteUri + "\n" + name))) + ".json");
    }
    public async Task WriteAsync(Draft draft, CancellationToken cancellation = default)
    {
        draft.Validate();
        await writes.WaitAsync(cancellation);
        try
        {
            var path = PathFor(new(draft.Server), draft.Name);
            foreach (var candidate in new[] { path, path + ".previous" })
            {
                if (!File.Exists(candidate)) continue;
                var damaged = false;
                try
                {
                    if (new FileInfo(candidate).Length > MaxSerializedDraftBytes) damaged = true;
                    else
                    {
                        var previous = (JsonSerializer.Deserialize<Draft>(await File.ReadAllBytesAsync(candidate, cancellation), Json)
                            ?? throw new InvalidDataException("Missing draft.")).Validate();
                        damaged = previous.Server != draft.Server || previous.Name != draft.Name;
                    }
                }
                catch (Exception error) when (error is InvalidDataException or JsonException or ArgumentException) { damaged = true; }
                if (damaged) File.Move(candidate, candidate + ".damaged-" + Guid.NewGuid().ToString("N"));
            }
            await ReplaceAsync(path, JsonSerializer.SerializeToUtf8Bytes(draft, Json), cancellation);
        }
        finally { writes.Release(); }
    }
    public static async Task ReplaceAsync(string path, byte[] bytes, CancellationToken cancellation = default)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        var temporary = path + "." + Guid.NewGuid().ToString("N") + ".tmp";
        try
        {
            await using (var file = new FileStream(temporary, FileMode.CreateNew, FileAccess.Write, FileShare.None, 4096,
                FileOptions.Asynchronous | FileOptions.WriteThrough))
            {
                await file.WriteAsync(bytes, cancellation);
                await file.FlushAsync(cancellation);
                file.Flush(true);
            }
            if (File.Exists(path)) File.Replace(temporary, path, path + ".previous");
            else File.Move(temporary, path);
        }
        finally { if (File.Exists(temporary)) File.Delete(temporary); }
    }
    private async Task<Draft?> ReadAsync(string path, string identity, List<string> damaged)
    {
        try
        {
            await using var file = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read | FileShare.Delete, 4096, FileOptions.Asynchronous | FileOptions.SequentialScan);
            if (file.Length > MaxSerializedDraftBytes) throw new InvalidDataException("Oversized draft.");
            var bytes = new byte[(int)file.Length]; await file.ReadExactlyAsync(bytes);
            var draft = (JsonSerializer.Deserialize<Draft>(bytes, Json)
                ?? throw new InvalidDataException("Missing draft.")).Validate();
            if (identity != PathFor(new(draft.Server), draft.Name)) throw new InvalidDataException("Draft identity differs.");
            return draft;
        }
        catch (FileNotFoundException) { return null; }
        catch (Exception error) when (error is InvalidDataException or JsonException or ArgumentException)
        { damaged.Add(path); return null; }
    }
    private async Task<Draft?> RecoverAsync(string path, List<string> damaged)
    {
        var draft = await ReadAsync(path, path, damaged);
        if (draft is not null) return draft;
        var backup = await ReadAsync(path + ".previous", path, damaged);
        return backup is null ? null : backup with { Block = SaveBlock.Uncertain };
    }
    public Task<Draft?> FindAsync(Uri server, string name)
    {
        Directory.CreateDirectory(DirectoryPath);
        return RecoverAsync(PathFor(server, name), []);
    }
    public async Task<DraftRecovery> LoadAsync(Uri server)
    {
        var drafts = new List<Draft>(); var damaged = new List<string>();
        Directory.CreateDirectory(DirectoryPath);
        damaged.AddRange(Directory.EnumerateFiles(DirectoryPath, "*.damaged-*"));
        var paths = Directory.EnumerateFiles(DirectoryPath, "*.json")
            .Concat(Directory.EnumerateFiles(DirectoryPath, "*.json.previous").Select(path => path[..^9])).Distinct().ToArray();
        foreach (var path in paths)
        {
            var draft = await RecoverAsync(path, damaged);
            if (draft?.Server == server.AbsoluteUri) drafts.Add(draft);
        }
        return new(drafts.ToArray(), damaged.ToArray());
    }
    public async Task RemoveAsync(Draft draft)
    {
        await writes.WaitAsync();
        try { var path = PathFor(new(draft.Server), draft.Name); File.Delete(path); File.Delete(path + ".previous"); }
        finally { writes.Release(); }
    }
}
