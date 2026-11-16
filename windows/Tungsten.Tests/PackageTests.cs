using System.IO.Compression;
using System.Reflection.PortableExecutable;
using System.Xml.Linq;

namespace Tungsten.Tests;

public sealed class PackageTests : IDisposable
{
    private readonly string directory = Path.Combine(Path.GetTempPath(), "tungsten-package-tests-" + Guid.NewGuid().ToString("N"));

    private static void Validate(string path, string architecture)
    {
        static void Require(bool condition, string message) { if (!condition) throw new InvalidDataException(message); }
        using var zip = ZipFile.OpenRead(path);
        Require(zip.GetEntry("AppxSignature.p7x") is null, "Candidates must remain unsigned.");
        foreach (var runtime in new[] { "coreclr.dll", "Microsoft.UI.Xaml.dll" })
            Require(zip.Entries.Any(e => Path.GetFileName(e.FullName).Equals(runtime, StringComparison.OrdinalIgnoreCase)), $"Missing bundled runtime: {runtime}");
        using var manifestStream = (zip.GetEntry("AppxManifest.xml") ?? throw new InvalidDataException("Missing package manifest.")).Open();
        var manifest = XDocument.Load(manifestStream);
        var identity = manifest.Descendants().Single(e => e.Name.LocalName == "Identity");
        Require((string?)identity.Attribute("Name") == "P4suta.Tungsten" && (string?)identity.Attribute("Publisher") == "CN=P4suta", "Package identity changed.");
        Require((string?)identity.Attribute("ProcessorArchitecture") == architecture, "Unexpected package architecture.");
        Require(!manifest.Descendants().Any(e => e.Name.LocalName == "PackageDependency"), "Self-contained candidates must not require an external framework package.");
        var executable = manifest.Descendants().Single(e => e.Name.LocalName == "Application").Attribute("Executable")?.Value;
        var entry = zip.GetEntry(executable ?? throw new InvalidDataException("Missing executable identity.")) ?? throw new InvalidDataException("Missing executable.");
        Require(entry.Length <= 8 * 1024 * 1024, "Unexpected app host size.");
        using var stream = entry.Open(); using var bytes = new MemoryStream(); stream.CopyTo(bytes); bytes.Position = 0;
        using var pe = new PEReader(bytes);
        Require(pe.PEHeaders.CoffHeader.Machine == (architecture == "x64" ? Machine.Amd64 : Machine.Arm64), "Executable architecture does not match the manifest.");
    }

    private string Fixture(bool runtime = true, bool signed = false)
    {
        Directory.CreateDirectory(directory);
        var path = Path.Combine(directory, Guid.NewGuid().ToString("N") + ".msix");
        using var zip = ZipFile.Open(path, ZipArchiveMode.Create);
        using (var writer = new StreamWriter(zip.CreateEntry("AppxManifest.xml").Open()))
            writer.Write("<Package><Identity Name=\"P4suta.Tungsten\" Publisher=\"CN=P4suta\" ProcessorArchitecture=\"x64\"/><Applications><Application Executable=\"Tungsten.exe\"/></Applications></Package>");
        var executable = File.ReadAllBytes(typeof(PackageTests).Assembly.Location);
        var pe = BitConverter.ToInt32(executable, 0x3c);
        executable[pe + 4] = 0x64; executable[pe + 5] = 0x86;
        using (var stream = zip.CreateEntry("Tungsten.exe").Open()) stream.Write(executable);
        zip.CreateEntry("Microsoft.UI.Xaml.dll");
        if (runtime) zip.CreateEntry("coreclr.dll");
        if (signed) zip.CreateEntry("AppxSignature.p7x");
        return path;
    }

    [Fact]
    public void CandidatesRequireTheDeclaredArchitectureBothRuntimesAndNoSignature()
    {
        Validate(Fixture(), "x64");
        Assert.Throws<InvalidDataException>(() => Validate(Fixture(), "arm64"));
        Assert.Throws<InvalidDataException>(() => Validate(Fixture(runtime: false), "x64"));
        Assert.Throws<InvalidDataException>(() => Validate(Fixture(signed: true), "x64"));
    }

    [Fact]
    [Trait("Category", "Package")]
    public void BuiltMsixContainsTheExpectedNativeApplicationAndRuntimes()
        => Validate(Environment.GetEnvironmentVariable("TUNGSTEN_PACKAGE_PATH") ?? throw new InvalidOperationException("Package path is required."),
            Environment.GetEnvironmentVariable("TUNGSTEN_PACKAGE_ARCH") ?? throw new InvalidOperationException("Package architecture is required."));

    public void Dispose() { if (Directory.Exists(directory)) Directory.Delete(directory, true); }
}
