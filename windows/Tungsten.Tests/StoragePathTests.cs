using Tungsten.Core;

namespace Tungsten.Tests;

public sealed class StoragePathTests
{
    [Fact]
    public void UiTestUsesAnExplicitCommandLineFlag()
    {
        Assert.True(StoragePaths.IsUiTest(["Tungsten.exe", "--ui-test"]));
        Assert.False(StoragePaths.IsUiTest(["Tungsten.exe"]));
        Assert.False(StoragePaths.IsUiTest(["Tungsten.exe", "--ui-test-other"]));
    }
    [Fact]
    public void NativeUiTestsUseTheirOwnDataDirectory()
    {
        var local = Path.GetTempPath();
        Assert.Equal(local, StoragePaths.For(local, false));
        Assert.Equal(Path.Combine(local, "ui-tests"), StoragePaths.For(local, true));
        Assert.NotEqual(StoragePaths.For(local, false), StoragePaths.For(local, true));
    }
}
