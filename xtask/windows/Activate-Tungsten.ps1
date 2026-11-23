$ErrorActionPreference = 'Stop'
$package = Get-AppxPackage -Name P4suta.Tungsten
if (!$package -or !$package.IsDevelopmentMode) {
    throw 'The Tungsten development package is missing.'
}

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class TungstenUiActivation
{
    [ComImport, Guid("2E941141-7F97-4756-BA1D-9DECDE894A3D")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IApplicationActivationManager
    {
        void ActivateApplication(
            [MarshalAs(UnmanagedType.LPWStr)] string appId,
            [MarshalAs(UnmanagedType.LPWStr)] string arguments,
            uint options,
            out uint processId);
    }

    [DllImport("ole32.dll", PreserveSig = false)]
    private static extern void CoCreateInstance(
        ref Guid classId, IntPtr outer, uint context, ref Guid interfaceId,
        [MarshalAs(UnmanagedType.Interface)] out IApplicationActivationManager instance);

    public static uint Activate(string appId)
    {
        var classId = new Guid("45BA127D-10A8-46EA-8AB7-56EA9078943C");
        var interfaceId = typeof(IApplicationActivationManager).GUID;
        const uint localServer = 4;
        IApplicationActivationManager manager;
        CoCreateInstance(ref classId, IntPtr.Zero, localServer, ref interfaceId, out manager);
        uint processId;
        manager.ActivateApplication(appId, "--ui-test", 0, out processId);
        return processId;
    }
}
'@

$processId = [TungstenUiActivation]::Activate(($package.PackageFamilyName + '!App'))
[pscustomobject]@{ ProcessId = $processId } | ConvertTo-Json -Compress
