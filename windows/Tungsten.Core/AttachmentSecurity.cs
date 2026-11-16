using System.Runtime.InteropServices;
using System.Runtime.Versioning;

namespace Tungsten.Core;

[SupportedOSPlatform("windows")]
internal static class AttachmentSecurity
{
    public static Task CheckAsync(string path, Uri source, CancellationToken cancellation)
    {
        var completion = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var thread = new Thread(() =>
        {
            try { cancellation.ThrowIfCancellationRequested(); Check(path, source); completion.SetResult(); }
            catch (OperationCanceledException) { completion.SetCanceled(cancellation); }
            catch (Exception error) { completion.SetException(error); }
        })
        { IsBackground = true, Name = "Tungsten attachment check" };
        thread.SetApartmentState(ApartmentState.STA);
        thread.Start();
        return completion.Task;
    }

    private static void Check(string path, Uri source)
    {
        var instance = Activator.CreateInstance(Type.GetTypeFromCLSID(new("4125dd96-e03a-4103-8f70-e0597d803b9c"), true)!)!;
        try
        {
            var services = (IAttachmentExecute)instance;
            var client = new Guid("f20ceac9-b71a-4778-bebe-10c7b4ce9337");
            services.SetClientGuid(ref client);
            services.SetLocalPath(Path.GetFullPath(path));
            services.SetSource(source.AbsoluteUri);
            services.Save();
        }
        catch (COMException error) { throw new IOException("Windows could not check the original file.", error); }
        finally { Marshal.FinalReleaseComObject(instance); }
    }

    [ComImport, Guid("73db1241-1e85-4581-8e4f-a81e1d0f8c57"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IAttachmentExecute
    {
        void SetClientTitle([MarshalAs(UnmanagedType.LPWStr)] string title);
        void SetClientGuid(ref Guid client);
        void SetLocalPath([MarshalAs(UnmanagedType.LPWStr)] string path);
        void SetFileName([MarshalAs(UnmanagedType.LPWStr)] string name);
        void SetSource([MarshalAs(UnmanagedType.LPWStr)] string source);
        void SetReferrer([MarshalAs(UnmanagedType.LPWStr)] string referrer);
        void CheckPolicy();
        void Prompt(nint window, int prompt, out int action);
        void Save();
        void Execute(nint window, [MarshalAs(UnmanagedType.LPWStr)] string? verb, out nint process);
        void SaveWithUI(nint window);
        void ClearClientState();
    }
}
