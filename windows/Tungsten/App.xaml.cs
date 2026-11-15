using Microsoft.UI.Xaml;
using Microsoft.Windows.AppLifecycle;
using Tungsten.Core;
using Windows.Storage;

namespace Tungsten;

public partial class App : Application
{
    public static MainWindow Window { get; private set; } = null!;
    public static string DataDirectory { get; private set; } = "";
    private AppInstance? instance;
    public App() => InitializeComponent();
    protected override async void OnLaunched(LaunchActivatedEventArgs args)
    {
        var uiTest = StoragePaths.IsUiTest(Environment.GetCommandLineArgs());
        instance = AppInstance.FindOrRegisterForKey(uiTest ? "Tungsten.UiTest" : "Tungsten");
        if (!instance.IsCurrent)
        {
            await instance.RedirectActivationToAsync(AppInstance.GetCurrent().GetActivatedEventArgs());
            Exit(); return;
        }
        instance.Activated += (_, _) => { var window = Window; window?.DispatcherQueue.TryEnqueue(window.Activate); };
        DataDirectory = StoragePaths.For(ApplicationData.Current.LocalFolder.Path, uiTest);
        Window = new MainWindow();
        Window.Activate();
    }
}
