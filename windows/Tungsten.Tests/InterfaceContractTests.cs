using System.Xml.Linq;

namespace Tungsten.Tests;

public sealed class InterfaceContractTests
{
    private static string Root
    {
        get
        {
            var directory = new DirectoryInfo(AppContext.BaseDirectory);
            while (directory is not null && !File.Exists(Path.Combine(directory.FullName, "mise.toml"))) directory = directory.Parent;
            return directory?.FullName ?? throw new InvalidOperationException("Repository root is missing.");
        }
    }
    private static XDocument Document(string name) => XDocument.Load(Path.Combine(Root, "windows", "Tungsten", name));

    [Fact]
    public void WritingUsesNativeControlsWithImmediateUpdatesAndAccessibleNames()
    {
        var page = Document("MainPage.xaml"); var elements = page.Descendants().ToArray();
        Assert.Contains(elements, e => e.Name.LocalName == "NavigationView");
        Assert.Contains(elements, e => e.Name.LocalName == "AutoSuggestBox");
        var editor = Assert.Single(elements, e => (string?)e.Attribute("AutomationProperties.AutomationId") == "Editor");
        Assert.Equal("RichEditBox", editor.Name.LocalName); Assert.Equal("True", (string?)editor.Attribute("AcceptsReturn"));
        Assert.Equal("Wrap", (string?)editor.Attribute("TextWrapping"));
        Assert.Equal("EditorTextChanging", (string?)editor.Attribute("TextChanging"));
        Assert.Equal("PlainText", (string?)editor.Attribute("ClipboardCopyFormat"));
        Assert.Equal("EditorSelectionChanged", (string?)editor.Attribute("SelectionChanged"));
        Assert.Equal("EditorPreviewKeyDown", (string?)editor.Attribute("PreviewKeyDown"));
        Assert.Null(editor.Attribute("Header"));
        foreach (var id in new[] { "Bold", "Italic", "InlineCode" })
            Assert.Single(elements, e => (string?)e.Attribute("AutomationProperties.AutomationId") == id);
        Assert.NotNull(editor.Attribute("AutomationProperties.Name"));
        Assert.Contains(elements, e => e.Name.LocalName == "InfoBar");
        Assert.Contains(elements, e => e.Name.LocalName == "CommandBar");
        Assert.True(elements.Count(e => e.Name.LocalName == "ListView") >= 4);
        Assert.DoesNotContain(elements, e => e.Name.LocalName.Contains("WebView", StringComparison.Ordinal));
        Assert.DoesNotContain(elements.Attributes(), a => a.Name.LocalName == "IsTextScaleFactorEnabled" && a.Value == "False");
    }

    [Fact]
    public void PackageCandidatesAreSelfContainedWithStableIdentityAndBothArchitectures()
    {
        var project = Document("Tungsten.csproj");
        string? Value(string name) => project.Descendants(name).SingleOrDefault()?.Value;
        Assert.Equal("true", Value("SelfContained")); Assert.Equal("true", Value("WindowsAppSDKSelfContained"));
        Assert.Equal("x64;ARM64", Value("Platforms"));
        Assert.Equal("win-x64;win-arm64", Value("RuntimeIdentifiers"));
        Assert.Equal("2.7.0", project.Descendants("PackageReference").Single(e => (string?)e.Attribute("Include") == "Microsoft.WindowsAppSDK").Attribute("Version")?.Value);
        Assert.Contains(project.Descendants("PackageReference"), e => (string?)e.Attribute("Include") == "Microsoft.Windows.SDK.BuildTools.WinUIAnalyzer" && (string?)e.Attribute("PrivateAssets") == "all");
        var manifest = Document("Package.appxmanifest");
        var identity = manifest.Descendants().Single(e => e.Name.LocalName == "Identity");
        Assert.Equal("P4suta.Tungsten", (string?)identity.Attribute("Name"));
        Assert.Equal("CN=P4suta", (string?)identity.Attribute("Publisher"));
        Assert.DoesNotContain(manifest.Descendants(), e => e.Name.LocalName == "Capability" && (string?)e.Attribute("Name") == "systemAIModels");
        Assert.Contains(manifest.Descendants(), e => e.Name.LocalName == "Resource" && (string?)e.Attribute("Language") == "en-US");
        var window = File.ReadAllText(Path.Combine(Root, "windows", "Tungsten", "MainWindow.xaml.cs"));
        Assert.DoesNotContain("ExtendsContentIntoTitleBar = true", window);
    }
    [Fact]
    public void NewNoteRemainsReachableWhenTheDetailPaneIsHidden()
    {
        var page = Document("MainPage.xaml");
        var create = Assert.Single(page.Descendants(), e => (string?)e.Attribute("AutomationProperties.AutomationId") == "NewNote");
        XNamespace x = "http://schemas.microsoft.com/winfx/2006/xaml";
        Assert.DoesNotContain(create.Ancestors(), e => (string?)e.Attribute(x + "Name") == "EditorPane");
    }

    [Fact]
    public void CreationBelongsToTheListAndNoteActionsUseOneAdaptiveBar()
    {
        var page = Document("MainPage.xaml");
        XNamespace x = "http://schemas.microsoft.com/winfx/2006/xaml";
        var create = Assert.Single(page.Descendants(), e => (string?)e.Attribute("AutomationProperties.AutomationId") == "NewNote");
        Assert.Contains(create.Ancestors(), e => (string?)e.Attribute(x + "Name") == "NotesPane");
        var editor = page.Descendants().Single(e => (string?)e.Attribute(x + "Name") == "EditorPane");
        Assert.Single(editor.Descendants(), e => e.Name.LocalName == "CommandBar");
        Assert.DoesNotContain(editor.Descendants(), e => (string?)e.Attribute("Text") == "{x:Bind ViewModel.Title, Mode=OneWay}");
    }

    [Fact]
    public void ConnectionIsSeparateFromWritingAndSettingsRemainsDiscoverable()
    {
        var page = Document("MainPage.xaml");
        XNamespace x = "http://schemas.microsoft.com/winfx/2006/xaml";
        var server = page.Descendants().Single(e => (string?)e.Attribute(x + "Name") == "ServerAddress");
        Assert.Contains(server.Ancestors(), e => (string?)e.Attribute(x + "Name") == "ConnectionPanel");
        var navigation = page.Descendants().Single(e => e.Name.LocalName == "NavigationView");
        Assert.Equal("True", (string?)navigation.Attribute("IsSettingsVisible"));
        Assert.Equal("CollectionInvoked", (string?)navigation.Attribute("ItemInvoked"));
        Assert.DoesNotContain(server.Ancestors(), e => e.Name.LocalName == "NavigationView");
    }

    [Fact]
    public void NoteSummariesSupportRecognitionWithoutOpeningEveryNote()
    {
        var page = Document("MainPage.xaml");
        XNamespace x = "http://schemas.microsoft.com/winfx/2006/xaml";
        var list = page.Descendants().Single(e => (string?)e.Attribute(x + "Name") == "NotesList");
        Assert.Contains(list.Descendants(), e => (string?)e.Attribute("Text") == "{x:Bind Snippet, Mode=OneTime}");
    }

    [Fact]
    public void NoteRowsConstrainWrappingAndExposeTitlesToAssistiveTechnology()
    {
        var page = Document("MainPage.xaml");
        foreach (var id in new[] { "NotesList", "TrashList" })
        {
            var list = page.Descendants().Single(e => (string?)e.Attribute("AutomationProperties.AutomationId") == id);
            Assert.Contains(list.Descendants("{http://schemas.microsoft.com/winfx/2006/xaml/presentation}Setter"),
                e => (string?)e.Attribute("Property") == "AutomationProperties.Name" && (string?)e.Attribute("Value") == "{Binding Title}");
            Assert.DoesNotContain(list.Descendants(), e => e.Name.LocalName == "StackPanel" && (string?)e.Attribute("Orientation") == "Horizontal");
        }
    }

    [Fact]
    public void VisibleXamlCopyHasEnglishStringResources()
    {
        var resources = XDocument.Load(Path.Combine(Root, "windows", "Tungsten", "Strings", "en-US", "Resources.resw"))
            .Descendants("data").ToDictionary(e => (string)e.Attribute("name")!, e => e.Element("value")!.Value);
        XNamespace x = "http://schemas.microsoft.com/winfx/2006/xaml";
        foreach (var element in Document("MainPage.xaml").Descendants())
            foreach (var attribute in element.Attributes().Where(a => new[] { "Header", "Label", "Content", "Title", "PlaceholderText", "Text" }.Contains(a.Name.LocalName)
                && a.Value.Length > 0 && !a.Value.StartsWith('{')))
            {
                var uid = (string?)element.Attribute(x + "Uid"); Assert.NotNull(uid);
                Assert.Equal(attribute.Value, resources[uid + "." + attribute.Name.LocalName]);
            }
    }

    [Fact]
    public void InteractiveShellControlsHaveStableAutomationIds()
    {
        string[] controls = ["Button", "AppBarButton", "TextBox", "RichEditBox", "ComboBox", "ListView", "NavigationViewItem", "MenuFlyoutItem", "AutoSuggestBox"];
        foreach (var element in Document("MainPage.xaml").Descendants().Where(e => controls.Contains(e.Name.LocalName)))
            Assert.False(string.IsNullOrWhiteSpace((string?)element.Attribute("AutomationProperties.AutomationId")), element.ToString());
    }

    [Fact]
    public void DetailsExposeReadableLinkAndAttachmentLabels()
    {
        var page = Document("MainPage.xaml");
        XNamespace x = "http://schemas.microsoft.com/winfx/2006/xaml";
        Assert.Equal("Text", (string?)page.Descendants().Single(e => (string?)e.Attribute(x + "Name") == "DetailLinks").Attribute("DisplayMemberPath"));
        Assert.Equal("Text", (string?)page.Descendants().Single(e => (string?)e.Attribute(x + "Name") == "DetailAttachments").Attribute("DisplayMemberPath"));
    }

    [Fact]
    public void DynamicShellCopyUsesExistingEnglishResources()
    {
        var source = File.ReadAllText(Path.Combine(Root, "windows", "Tungsten", "MainPage.xaml.cs"));
        Assert.DoesNotMatch("\\b(?:Content|Header|PlaceholderText|PrimaryButtonText|CloseButtonText)\\s*=\\s*\\$?\"[^\"]+\"", source);
        Assert.DoesNotMatch("\\b(?:Dialog|ConfirmAsync|WorkspaceTitleAsync)\\(\\$?\"", source);
        var resources = XDocument.Load(Path.Combine(Root, "windows", "Tungsten", "Strings", "en-US", "Resources.resw"))
            .Descendants("data").Select(e => ((string)e.Attribute("name")!).Replace('.', '/')).ToHashSet();
        var keys = System.Text.RegularExpressions.Regex.Matches(source, "UiText\\.Get\\(\"([^\"]+)\"").Select(m => m.Groups[1].Value).ToArray();
        Assert.NotEmpty(keys);
        foreach (var key in keys) Assert.Contains(key, resources);
    }
}
