using System.Collections.ObjectModel;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Tungsten.Core;
using Windows.Storage;
using Microsoft.Windows.Storage.Pickers;
using Windows.System;

namespace Tungsten;

public sealed partial class MainPage : Page
{
    private readonly AtomicDraftStore drafts;
    private readonly string settings;
    private readonly ObservableCollection<Summary> notes = [];
    private readonly ObservableCollection<SavedNote> deleted = [];
    private Workspace[] Workspaces => ViewModel.Workspaces;
    private readonly DispatcherTimer autosave = new() { Interval = TimeSpan.FromSeconds(1) };
    private readonly DispatcherTimer refresh = new() { Interval = TimeSpan.FromSeconds(15) };
    private readonly ActionQueue actions = new();
    private readonly NoteSearch noteSearch = new();
    private bool ready, composing, detailVisible, updatingCollections;
    private bool connectionVisible = true;
    private string collection = "all";
    private string? cursor;
    private double defaultFontSize, zoom = 1;
    private readonly LiveMarkdownEditor live;
    private EditorSession? displayedEditor;
    private string? presentationProblem, inputProblem;
    private string? operationProblem, collectionProblem, compositionProblem;
    public ClientModel ViewModel { get; }
    private NoteClient Client => ViewModel.Client ?? throw new InvalidOperationException(UiText.Get("ConnectFirst"));
    private Workspace SelectedWorkspace => Workspaces.Single(w => w.Id == collection);

    public MainPage()
    {
        var directory = App.DataDirectory;
        drafts = new(Path.Combine(directory, "drafts")); settings = Path.Combine(directory, "connection.txt");
        ViewModel = new(drafts, canLeave: () => !composing);
        InitializeComponent();
        live = new(Editor);
        live.InputRejected += (_, error) => { inputProblem = error.Message; UpdateStatus(); };
        live.SourceChanged += (_, _) =>
        {
            inputProblem = null;
            ViewModel.Body = live.Source;
            if (!composing && ready) { autosave.Stop(); autosave.Start(); }
            QueueRendering();
        };
        live.PresentationFailed += (_, error) =>
        {
            if (StoragePaths.IsUiTest(Environment.GetCommandLineArgs())) File.AppendAllText(Path.Combine(App.DataDirectory, "native-errors.log"), error + Environment.NewLine);
            presentationProblem = UiText.Get("LivePreviewFailure", error.Message); UpdateStatus();
        };
        NotesList.ItemsSource = notes; TrashList.ItemsSource = deleted;
        ViewModel.PropertyChanged += (_, change) =>
        {
            if (change.PropertyName == nameof(ClientModel.Body) || !ReferenceEquals(displayedEditor, ViewModel.Editor)) inputProblem = null;
            live.Load(ViewModel.Body, !ReferenceEquals(displayedEditor, ViewModel.Editor));
            displayedEditor = ViewModel.Editor;
            UpdateStatus();
        };
        autosave.Tick += async (_, _) =>
        {
            await ViewModel.AutosaveAsync(composing, actions.IsBusy, () => PerformAsync(SaveAndListAsync));
            UpdateStatus();
        };
        refresh.Tick += async (_, _) => await CheckRemoteAsync();
    }
    private async void PageLoaded(object sender, RoutedEventArgs args)
    {
        if (ready) return; ready = true; defaultFontSize = Editor.FontSize;
        autosave.Start(); refresh.Start();
        if (File.Exists(settings)) await PerformAsync(async () =>
        {
            ServerAddress.Text = await File.ReadAllTextAsync(settings);
            await ConnectAsync();
        });
        UpdateLayoutMode();
    }
    private void CollectionsLoaded(object sender, RoutedEventArgs args) => ConfigureSettingsItem();
    private void CollectionsPaneOpened(NavigationView sender, object args) => ConfigureSettingsItem();
    private void ConfigureSettingsItem()
    {
        Collections.ApplyTemplate();
        if (Collections.SettingsItem is NavigationViewItem settingsItem)
        {
            settingsItem.Content = UiText.Get("SettingsTitle");
            AutomationProperties.SetAutomationId(settingsItem, "ConnectionSettings");
        }
    }
    private async Task PerformAsync(Func<Task> operation)
    {
        await actions.RunAsync(async () =>
        {
            operationProblem = null;
            try { await operation(); }
            catch (CompositionInProgressException error) { ReportCompositionProblem(error.Message); }
            catch (Exception error) when (error is IOException or InvalidDataException or HttpRequestException or UnauthorizedAccessException or OperationCanceledException or InvalidOperationException)
            { operationProblem = error.Message; }
            finally { UpdateStatus(); }
        });
    }
    private void UpdateStatus()
    {
        SaveStatus.Text = ViewModel.Editor is { Block: not SaveBlock.None, IsDurable: true }
            ? UiText.Get("LocalDraftStatus") : ViewModel.Status;
        ConnectionPanel.Visibility = connectionVisible ? Visibility.Visible : Visibility.Collapsed;
        Collections.Visibility = connectionVisible ? Visibility.Collapsed : Visibility.Visible;
        CancelConnection.Visibility = ViewModel.HasConnection ? Visibility.Visible : Visibility.Collapsed;
        Editor.Visibility = ViewModel.HasEditor ? Visibility.Visible : Visibility.Collapsed;
        EmptyEditor.Visibility = ViewModel.HasEditor ? Visibility.Collapsed : Visibility.Visible;
        CollectionTitle.Text = collection switch
        {
            "all" => UiText.Get("AllNotes/Content"),
            "drafts" => UiText.Get("Drafts/Content"),
            "trash" => UiText.Get("Trash/Content"),
            _ => Workspaces.FirstOrDefault(w => w.Id == collection)?.Title ?? UiText.Get("AllNotes/Content")
        };
        RenameWorkspace.IsEnabled = DeleteWorkspace.IsEnabled = ViewModel.HasConnection && Workspaces.Any(w => w.Id == collection);
        var message = inputProblem ?? presentationProblem ?? (composing ? compositionProblem : null) ?? (ViewModel.Problem.Length > 0 ? ViewModel.Problem
            : ViewModel.Editor is { Block: not SaveBlock.None } ? ViewModel.Status : collectionProblem ?? operationProblem);
        Problem.Message = message ?? ""; Problem.IsOpen = !string.IsNullOrEmpty(message);
    }
    private void ReportProblem(string message) { operationProblem = message; UpdateStatus(); }
    private void ReportCompositionProblem(string message) { compositionProblem = message; UpdateStatus(); }
    private async Task ConnectAsync()
    {
        if (composing) throw new InvalidOperationException(UiText.Get("CompositionBeforeServerChange"));
        var origin = Rules.Origin(ServerAddress.Text,
#if DEBUG
            true
#else
            false
#endif
        );
        var changed = ViewModel.Client?.Origin != origin;
        if (changed) await ViewModel.ConnectAsync(origin);
        await AtomicDraftStore.ReplaceAsync(settings, Rules.Utf8.GetBytes(origin.AbsoluteUri));
        ServerAddress.Text = origin.AbsoluteUri;
        connectionVisible = false;
        if (changed) { collection = "all"; detailVisible = false; Search.Text = ""; }
        UpdateStatus();
        await ViewModel.RefreshWorkspacesAsync(); UpdateWorkspaceNavigation();
        await LoadNotesAsync();
        UpdateLayoutMode();
    }
    private void UpdateWorkspaceNavigation()
    {
        updatingCollections = true;
        try
        {
            while (Collections.MenuItems.Count > 4) Collections.MenuItems.RemoveAt(4);
            foreach (var workspace in Workspaces)
            {
                var item = new NavigationViewItem { Content = workspace.Title, Tag = workspace.Id, Icon = new SymbolIcon(Symbol.Folder) };
                AutomationProperties.SetName(item, workspace.Title); AutomationProperties.SetAutomationId(item, "Workspace" + workspace.Id); Collections.MenuItems.Add(item);
            }
            Collections.SelectedItem = Collections.MenuItems.OfType<NavigationViewItem>().FirstOrDefault(item => item.Tag?.ToString() == collection) ?? Collections.MenuItems[0];
        }
        finally { updatingCollections = false; }
    }
    private async void ConnectClicked(object sender, RoutedEventArgs args) => await PerformAsync(ConnectAsync);
    private async void CollectionInvoked(NavigationView sender, NavigationViewItemInvokedEventArgs args)
    {
        if (!args.IsSettingsInvoked) return;
        await PerformAsync(async () =>
        {
            if (composing || !await ViewModel.CanLeaveAsync()) return;
            ServerAddress.Text = Client.Origin.AbsoluteUri;
            connectionVisible = true; UpdateStatus();
            ServerAddress.Focus(FocusState.Programmatic);
        });
    }
    private void CancelConnectionClicked(object sender, RoutedEventArgs args)
    {
        if (!ViewModel.HasConnection) return;
        ServerAddress.Text = Client.Origin.AbsoluteUri;
        connectionVisible = false; UpdateWorkspaceNavigation(); UpdateStatus(); UpdateLayoutMode();
        if (ViewModel.HasEditor) Editor.Focus(FocusState.Programmatic);
        else NotesList.Focus(FocusState.Programmatic);
    }
    private async void CollectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (!ready || updatingCollections || args.IsSettingsSelected || args.SelectedItem is not NavigationViewItem item || ViewModel.Client is null) return;
        var target = item.Tag?.ToString() ?? "all";
        await PerformAsync(async () =>
        {
            if (composing || !await ViewModel.CanLeaveAsync()) throw new IOException(UiText.Get("StoreBeforeLeaving"));
            collection = target; detailVisible = false; await LoadNotesAsync(); UpdateLayoutMode();
        });
    }
    private async Task LoadNotesAsync(bool append = false, string? query = null)
    {
        var submittedQuery = query ?? Search.Text;
        Search.Visibility = collection is "all" or "drafts" ? Visibility.Visible : Visibility.Collapsed;
        if (!append) { notes.Clear(); deleted.Clear(); cursor = null; collectionProblem = null; }
        NotesList.Visibility = collection == "trash" ? Visibility.Collapsed : Visibility.Visible;
        TrashList.Visibility = collection == "trash" ? Visibility.Visible : Visibility.Collapsed;
        EmptyTrashButton.Visibility = TrashList.Visibility;
        if (collection == "trash")
        {
            var page = await Client.TrashAsync(cursor); foreach (var entry in page.Entries) deleted.Add(entry); cursor = page.Next;
        }
        else if (collection == "drafts")
        {
            var recovery = await drafts.LoadAsync(Client.Origin);
            foreach (var draft in recovery.Drafts.Where(d => d.Body != d.BaseBody || d.Pending is not null || d.Block != SaveBlock.None)
                .Where(d => d.Body.Contains(submittedQuery, StringComparison.OrdinalIgnoreCase)))
                notes.Add(new(draft.Name, Rules.Title(draft.Body)));
            if (recovery.DamagedFiles.Length > 0)
                collectionProblem = UiText.Get("DamagedDrafts");
        }
        else
        {
            if (collection == "all")
            {
                var request = noteSearch.Begin(append ? null : submittedQuery);
                if (request is null) return;
                try
                {
                    var page = await Client.NotesAsync(request.Query, request.Cursor);
                    if (!noteSearch.Complete(request, page.Next)) return;
                    MergeNotes(page.Notes); cursor = noteSearch.Cursor;
                }
                catch { noteSearch.Fail(request); throw; }
            }
            else
            { var page = await Client.WorkspaceNotesAsync(collection, cursor); MergeNotes(page.Notes); cursor = page.Next; }
        }
        MoreButton.Visibility = cursor is null ? Visibility.Collapsed : Visibility.Visible;
        ListStatus.Text = notes.Count + deleted.Count == 0 ? UiText.Get("EmptyNoteList") : "";
    }
    private void MergeNotes(IEnumerable<Summary> incoming)
    {
        var merged = NoteList.Merge(notes, incoming);
        for (var index = 0; index < merged.Length; index++)
        {
            if (index == notes.Count) notes.Add(merged[index]);
            else if (notes[index] != merged[index]) notes[index] = merged[index];
        }
    }
    private async Task SaveAndListAsync()
    {
        var editor = ViewModel.Editor;
        await ViewModel.SaveAsync();
        if (ReferenceEquals(editor, ViewModel.Editor) && editor?.Current is { Revision: not null, BaseBody: not null } saved && collection == "all" && noteSearch.Query.Length == 0)
        { MergeNotes([new(saved.Name, Rules.Title(saved.BaseBody))]); ListStatus.Text = ""; }
    }
    private async void SearchSubmitted(AutoSuggestBox sender, AutoSuggestBoxQuerySubmittedEventArgs args) => await PerformAsync(() => LoadNotesAsync(query: args.QueryText));
    private async void MoreClicked(object sender, RoutedEventArgs args)
    {
        if (collection == "all" && !noteSearch.CanLoadMore) return;
        await PerformAsync(() => LoadNotesAsync(true));
    }
    private async void NoteClicked(object sender, ItemClickEventArgs args) => await PerformAsync(async () =>
    {
        if (composing) return;
        await ViewModel.OpenAsync(((Summary)args.ClickedItem).Name);
        detailVisible = true; UpdateLayoutMode(); Editor.Focus(FocusState.Programmatic);
    });
    private async void NewClicked(object sender, RoutedEventArgs args) => await NewNoteAsync();
    private async Task NewNoteAsync() => await PerformAsync(async () =>
    { if (composing) return; await ViewModel.NewAsync(); detailVisible = true; UpdateLayoutMode(); Editor.Focus(FocusState.Programmatic); });
    private async void SaveClicked(object sender, RoutedEventArgs args) => await PerformAsync(SaveAndListAsync);
    private async void SaveAsNewClicked(object sender, RoutedEventArgs args) => await PerformAsync(async () =>
    { if (!composing) { await ViewModel.SaveAsNewAsync(); Editor.Focus(FocusState.Programmatic); } });
    private async void DiscardClicked(object sender, RoutedEventArgs args) => await PerformAsync(async () =>
    {
        if (composing || ViewModel.Editor is null) return;
        if (await ConfirmAsync(UiText.Get("DiscardTitle"), UiText.Get("DiscardBody"), UiText.Get("ActionDiscard")))
        { await ViewModel.DiscardAsync(); detailVisible = ViewModel.HasEditor; UpdateLayoutMode(); if (ViewModel.HasEditor) Editor.Focus(FocusState.Programmatic); else NotesList.Focus(FocusState.Programmatic); }
    });
    private async void RetryClicked(object sender, RoutedEventArgs args) => await PerformAsync(async () =>
    {
        if (composing) return;
        if (ViewModel.Editor is not { } editor) { await ConnectAsync(); return; }
        await editor.RetryAsync();
        if (editor.Block == SaveBlock.None) await SaveAndListAsync();
    });
    private void EditorTextChanging(RichEditBox sender, RichEditBoxTextChangingEventArgs args)
    {
        live?.ReadChanges();
    }
    private void EditorSelectionChanged(object sender, RoutedEventArgs args) { live?.TrackSelection(); QueueRendering(); }
    private void EditorFocusChanged(object sender, RoutedEventArgs args) => QueueRendering();
    private void EditorThemeChanged(FrameworkElement sender, object args) => QueueRendering(true);
    private void BoldClicked(object sender, RoutedEventArgs args) => live.Format(MarkdownStyle.Bold);
    private void ItalicClicked(object sender, RoutedEventArgs args) => live.Format(MarkdownStyle.Italic);
    private void CodeClicked(object sender, RoutedEventArgs args) => live.Format(MarkdownStyle.Code);
    private void EditorPreviewKeyDown(object sender, KeyRoutedEventArgs args)
    {
        if (!Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Control).HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down)) return;
        if (args.Key is VirtualKey.Z or VirtualKey.Y)
        {
            args.Handled = true;
            var shift = Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Shift).HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down);
            Undo(args.Key == VirtualKey.Y || shift); return;
        }
        if (args.Key is VirtualKey.B or VirtualKey.I or VirtualKey.U)
        {
            args.Handled = true;
            if (!composing && args.Key != VirtualKey.U) live.Format(args.Key == VirtualKey.B ? MarkdownStyle.Bold : MarkdownStyle.Italic);
        }
    }
    private void Undo(bool redo)
    {
        if (!live.Undo(redo)) return;
        ViewModel.Body = live.Source; autosave.Stop(); autosave.Start();
    }
    private void EditorUndoClicked(object sender, RoutedEventArgs args) => Undo(false);
    private void EditorRedoClicked(object sender, RoutedEventArgs args) => Undo(true);
    private void EditorCopyClicked(object sender, RoutedEventArgs args) => live.Copy(false);
    private void EditorCutClicked(object sender, RoutedEventArgs args) { if (!composing) live.Copy(true); }
    private async void EditorPasteClicked(object sender, RoutedEventArgs args) => await PasteAsync();
    private async Task PasteAsync()
    {
        if (composing) return;
        try { await live.PasteAsync(); }
        catch (Exception error) when (error is System.Runtime.InteropServices.COMException or InvalidOperationException or InvalidDataException)
        { ReportProblem(error.Message); }
    }
    private void EditorSelectAllClicked(object sender, RoutedEventArgs args) => live.SelectAll();
    private void EditorMenuOpening(object sender, object args) { EditorUndo.IsEnabled = live.CanUndo; EditorRedo.IsEnabled = live.CanRedo; }
    private void QueueRendering(bool force = false) => DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, () => live?.Render(force));
    private void EditorCopying(RichEditBox sender, TextControlCopyingToClipboardEventArgs args) { args.Handled = true; live.Copy(false); }
    private void EditorCutting(RichEditBox sender, TextControlCuttingToClipboardEventArgs args) { args.Handled = true; if (!composing) live.Copy(true); }
    private async void EditorPaste(object sender, TextControlPasteEventArgs args)
    {
        args.Handled = true; await PasteAsync();
    }
    private void CompositionStarted(RichEditBox sender, TextCompositionStartedEventArgs args) { composing = true; live.Composing = true; }
    private void CompositionEnded(RichEditBox sender, TextCompositionEndedEventArgs args)
    { composing = false; live.Composing = false; compositionProblem = null; UpdateStatus(); QueueRendering(); autosave.Stop(); autosave.Start(); }
    public bool CanCloseNow() => ViewModel.CanCloseNow();
    public async Task<bool> CanCloseAsync()
    {
        if (composing) { ReportCompositionProblem(UiText.Get("CompositionBeforeClose")); return false; }
        var result = await ViewModel.CanLeaveAsync();
        if (composing) { ReportCompositionProblem(UiText.Get("CompositionBeforeClose")); return false; }
        UpdateStatus(); return result;
    }
    public async Task CheckRemoteAsync()
    {
        if (!ready || actions.IsBusy || composing || ViewModel.Editor is not { } editor) return;
        await PerformAsync(async () =>
        {
            await editor.RefreshAsync(() => !composing);
            if (ReferenceEquals(editor, ViewModel.Editor) && !composing && ViewModel.Body != editor.Current.Body) ViewModel.Body = editor.Current.Body;
        });
    }
    private void WorkspaceSizeChanged(object sender, SizeChangedEventArgs args) => UpdateLayoutMode();
    private void CollectionsDisplayModeChanged(NavigationView sender, NavigationViewDisplayModeChangedEventArgs args) => UpdateLayoutMode();
    private void UpdateLayoutMode()
    {
        if (!ready) return;
        var layout = ListDetailLayout.For(WorkspaceGrid.ActualWidth, detailVisible);
        NotesPane.Visibility = layout.ShowNotes ? Visibility.Visible : Visibility.Collapsed;
        EditorPane.Visibility = layout.ShowEditor ? Visibility.Visible : Visibility.Collapsed;
        NotesColumn.Width = layout.Stacked ? (layout.ShowEditor ? new(0) : new(1, GridUnitType.Star)) : new(300);
        Collections.IsBackEnabled = layout.CanGoBack;
        Collections.IsBackButtonVisible = layout.CanGoBack ? NavigationViewBackButtonVisible.Visible : NavigationViewBackButtonVisible.Collapsed;
        var inset = Collections.DisplayMode == NavigationViewDisplayMode.Minimal ? (layout.CanGoBack ? 96 : 48) : 0;
        NotesHeader.Margin = EditorCommands.Margin = new Thickness(inset, 0, 0, 0);
    }
    private async void BackRequested(NavigationView sender, NavigationViewBackRequestedEventArgs args) => await PerformAsync(async () =>
    { if (!composing && await ViewModel.CanLeaveAsync()) { detailVisible = false; UpdateLayoutMode(); NotesList.Focus(FocusState.Programmatic); } });
    private void Zoom(double value) { zoom = Math.Clamp(value, 1, 2); Editor.FontSize = defaultFontSize * zoom; QueueRendering(true); }
    private void ZoomInClicked(object sender, RoutedEventArgs args) => Zoom(zoom + .1);
    private void ZoomOutClicked(object sender, RoutedEventArgs args) => Zoom(zoom - .1);
    private void ZoomResetClicked(object sender, RoutedEventArgs args) => Zoom(1);
    private async void RootKeyDown(object sender, KeyRoutedEventArgs args)
    {
        var ctrl = Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Control).HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down);
        if (args.Key == VirtualKey.F6)
        {
            args.Handled = true;
            var focused = FocusManager.GetFocusedElement(Root.XamlRoot) as DependencyObject;
            var current = Within(focused, EditorPane) ? FocusRegion.Editor : Within(focused, NotesPane) ? FocusRegion.Notes : FocusRegion.Collections;
            var reverse = Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Shift).HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down);
            switch (FocusNavigation.Move(current, reverse, NotesPane.Visibility == Visibility.Visible, EditorPane.Visibility == Visibility.Visible && ViewModel.HasEditor))
            {
                case FocusRegion.Collections: Collections.Focus(FocusState.Keyboard); break;
                case FocusRegion.Notes:
                    if (collection == "trash") { if (!TrashList.Focus(FocusState.Keyboard)) Search.Focus(FocusState.Keyboard); }
                    else if (!NotesList.Focus(FocusState.Keyboard)) Search.Focus(FocusState.Keyboard);
                    break;
                case FocusRegion.Editor: Editor.Focus(FocusState.Keyboard); break;
            }
        }
        if (!ctrl || composing || connectionVisible) return;
        switch (args.Key)
        {
            case VirtualKey.N: args.Handled = true; await NewNoteAsync(); break;
            case VirtualKey.S: args.Handled = true; await PerformAsync(SaveAndListAsync); break;
            case VirtualKey.F:
                args.Handled = true;
                await PerformAsync(async () =>
                {
                    if (composing || !await ViewModel.CanLeaveAsync() || composing) return;
                    if (collection is not ("all" or "drafts"))
                    {
                        collection = "all"; Search.Text = "";
                        await LoadNotesAsync(); UpdateWorkspaceNavigation();
                    }
                    detailVisible = false; UpdateLayoutMode(); Search.Focus(FocusState.Keyboard);
                });
                break;
            case VirtualKey.Number0: args.Handled = true; Zoom(1); break;
            case VirtualKey.Add: args.Handled = true; Zoom(zoom + .1); break;
            case VirtualKey.Subtract: args.Handled = true; Zoom(zoom - .1); break;
        }
    }
    private static bool Within(DependencyObject? child, DependencyObject ancestor)
    {
        while (child is not null) { if (ReferenceEquals(child, ancestor)) return true; child = VisualTreeHelper.GetParent(child); }
        return false;
    }
    private ContentDialog Dialog(string title, object content, string? action = null)
    {
        var dialog = new ContentDialog { XamlRoot = XamlRoot, Title = title, Content = content, PrimaryButtonText = action ?? "", CloseButtonText = UiText.Get("ActionCancel"), DefaultButton = ContentDialogButton.Close };
        AutomationProperties.SetAutomationId(dialog, "NoteActionDialog"); return dialog;
    }
    private async Task<ContentDialogResult> ShowDialogAsync(ContentDialog dialog)
    {
        if (composing) { ReportCompositionProblem(UiText.Get("CompositionBeforeDialog")); return ContentDialogResult.None; }
        if (!await ViewModel.CanLeaveAsync()) { UpdateStatus(); return ContentDialogResult.None; }
        return await dialog.ShowAsync();
    }
    private async Task<bool> ConfirmAsync(string title, string content, string action)
        => await ShowDialogAsync(Dialog(title, new TextBlock { Text = content, TextWrapping = TextWrapping.Wrap }, action)) == ContentDialogResult.Primary;
    private async Task<string?> WorkspaceTitleAsync(string title, string value = "")
    {
        var input = new TextBox { Header = UiText.Get("WorkspaceNameHeader"), Text = value, MaxLength = 120 };
        AutomationProperties.SetName(input, UiText.Get("WorkspaceNameHeader")); AutomationProperties.SetAutomationId(input, "WorkspaceName");
        return await ShowDialogAsync(Dialog(title, input, UiText.Get("ActionSave"))) == ContentDialogResult.Primary ? input.Text : null;
    }
    private async void CreateWorkspaceClicked(object sender, RoutedEventArgs args) => await PerformAsync(async () =>
    { var title = await WorkspaceTitleAsync(UiText.Get("CreateWorkspaceTitle")); if (title is not null) { await ViewModel.CreateWorkspaceAsync(title); UpdateWorkspaceNavigation(); } });
    private async void RenameWorkspaceClicked(object sender, RoutedEventArgs args) => await PerformAsync(async () =>
    {
        var workspace = SelectedWorkspace; var title = await WorkspaceTitleAsync(UiText.Get("RenameWorkspaceTitle"), workspace.Title);
        if (title is not null) { await ViewModel.RenameWorkspaceAsync(workspace.Id, title); UpdateWorkspaceNavigation(); }
    });
    private async void DeleteWorkspaceClicked(object sender, RoutedEventArgs args) => await PerformAsync(async () =>
    {
        var workspace = SelectedWorkspace;
        if (await ConfirmAsync(UiText.Get("DeleteWorkspaceTitle"), UiText.Get("DeleteWorkspaceBody", workspace.Title), UiText.Get("ActionDelete")))
        { await ViewModel.DeleteWorkspaceAsync(workspace.Id); collection = "all"; UpdateWorkspaceNavigation(); await LoadNotesAsync(); }
    });
    private async void DeleteNoteClicked(object sender, RoutedEventArgs args) => await PerformAsync(async () =>
    {
        if (ViewModel.Editor is not { } editor || editor.Current.Revision is null || composing) return;
        if (!await ViewModel.CanLeaveAsync()) return;
        if (!await ConfirmAsync(UiText.Get("TrashNoteTitle"), UiText.Get("TrashNoteBody", ViewModel.Title), UiText.Get("ActionMoveToTrash"))) return;
        await Client.DeleteAsync(new(editor.Current.Name, editor.Current.BaseBody!, editor.Current.Revision));
        await editor.RefreshAsync(); await LoadNotesAsync();
    });
    private async void EmptyTrashClicked(object sender, RoutedEventArgs args) => await PerformAsync(async () =>
    { if (await ConfirmAsync(UiText.Get("EmptyTrashTitle"), UiText.Get("EmptyTrashBody"), UiText.Get("ActionDeletePermanently"))) { await Client.EmptyTrashAsync(); await LoadNotesAsync(); } });
    private async void TrashClicked(object sender, ItemClickEventArgs args) => await PerformAsync(async () =>
    {
        var record = (SavedNote)args.ClickedItem; var saved = await Client.SavedAsync(record, true);
        var preview = new TextBox { Header = record.Title, Text = saved.Body, IsReadOnly = true, AcceptsReturn = true, TextWrapping = TextWrapping.Wrap, MinHeight = 180 };
        AutomationProperties.SetAutomationId(preview, "DeletedNoteSource"); AutomationProperties.SetName(preview, record.Title);
        if (await ShowDialogAsync(Dialog(UiText.Get("DeletedNoteTitle"), preview, UiText.Get("ActionRestore"))) == ContentDialogResult.Primary)
        { await Client.RestoreAsync(record, Rules.GeneratedName()); await LoadNotesAsync(); }
    });
    private async void AttachClicked(object sender, RoutedEventArgs args) => await PerformAsync(async () =>
    {
        if (ViewModel.Editor is null || composing) return;
        if (!await ViewModel.CanLeaveAsync()) return;
        var picker = new FileOpenPicker(App.Window.AppWindow.Id);
        picker.FileTypeFilter.Add("*");
        var file = await picker.PickSingleFileAsync(); if (file is null) return;
        var attachment = await Client.UploadAsync(file.Path);
        live.ReplaceSelection(MarkdownLinks.Original(ViewModel.Editor.Current.Name, attachment));
        await ViewModel.CanLeaveAsync(); Editor.Focus(FocusState.Programmatic);
    });
    private async void LinkClicked(object sender, RoutedEventArgs args) => await PerformAsync(async () =>
    {
        if (ViewModel.Editor is null || composing) return;
        var search = new AutoSuggestBox { Header = UiText.Get("FindNoteHeader"), QueryIcon = new SymbolIcon(Symbol.Find) };
        AutomationProperties.SetAutomationId(search, "LinkSearch"); AutomationProperties.SetName(search, UiText.Get("FindNoteHeader"));
        var list = new ListView { DisplayMemberPath = "Title", SelectionMode = ListViewSelectionMode.Single, MaxHeight = 320 };
        AutomationProperties.SetAutomationId(list, "LinkChoices"); AutomationProperties.SetName(list, UiText.Get("LinkChoicesName"));
        var panel = new StackPanel { Spacing = 8 }; panel.Children.Add(search); panel.Children.Add(list);
        var more = new Button { Content = UiText.Get("MoreLinksLabel") }; AutomationProperties.SetAutomationId(more, "MoreLinks"); panel.Children.Add(more);
        var choices = new ObservableCollection<Summary>(); list.ItemsSource = choices; var searchState = new NoteSearch();
        async Task Load(string? query)
        {
            var request = searchState.Begin(query);
            if (request is null) return;
            if (query is not null) choices.Clear();
            more.IsEnabled = false;
            try
            {
                var page = await Client.NotesAsync(request.Query, request.Cursor);
                if (!searchState.Complete(request, page.Next)) return;
                foreach (var item in page.Notes) choices.Add(item);
            }
            catch (Exception error) when (error is IOException or InvalidDataException or HttpRequestException or OperationCanceledException)
            { if (searchState.Fail(request)) ReportProblem(error.Message); }
            finally { more.IsEnabled = searchState.CanLoadMore; }
        }
        search.QuerySubmitted += async (_, submitted) => await Load(submitted.QueryText); more.Click += async (_, _) => await Load(null);
        await Load("");
        if (await ShowDialogAsync(Dialog(UiText.Get("InsertLinkTitle"), panel, UiText.Get("ActionInsert"))) == ContentDialogResult.Primary && list.SelectedItem is Summary note)
        { live.ReplaceSelection(MarkdownLinks.Note(ViewModel.Editor.Current.Name, note)); await ViewModel.CanLeaveAsync(); }
        Editor.Focus(FocusState.Programmatic);
    });
    private async void DetailsClicked(object sender, RoutedEventArgs args) => await PerformAsync(async () =>
    {
        if (ViewModel.Editor is not { } editor || editor.Current.Revision is null) return;
        var observed = editor.Current.Body; var name = editor.Current.Name;
        var connections = await Client.ConnectionsAsync(name); var backlinks = await Client.BacklinksAsync(name); var history = await Client.HistoryAsync(name);
        var references = MarkdownLinks.References(Client.Origin, name, editor.Current.BaseBody!, connections);
        DetailLinks.ItemsSource = references.Links; DetailBacklinks.ItemsSource = backlinks;
        DetailHistory.ItemsSource = history; DetailAttachments.ItemsSource = references.Attachments;
        DetailWorkspace.ItemsSource = Workspaces;
        string? target = null; SavedNote? version = null; string? original = null;
        ItemClickEventHandler linkClick = (_, e) => { target = ((NoteReference)e.ClickedItem).Name; DetailsDialog.Hide(); };
        ItemClickEventHandler backClick = (_, e) => { target = ((Summary)e.ClickedItem).Name; DetailsDialog.Hide(); };
        ItemClickEventHandler historyClick = (_, e) => { version = (SavedNote)e.ClickedItem; DetailsDialog.Hide(); };
        ItemClickEventHandler attachmentClick = (_, e) => { original = ((AttachmentReference)e.ClickedItem).Id; DetailsDialog.Hide(); };
        var membershipBusy = false; Task pendingMembership = Task.CompletedTask;
        async Task Membership(bool included)
        {
            if (DetailWorkspace.SelectedItem is not Workspace w) return;
            membershipBusy = true; IncludeWorkspace.IsEnabled = ExcludeWorkspace.IsEnabled = false;
            try { await ViewModel.MembershipAsync(w.Id, name, included); UpdateWorkspaceNavigation(); DetailsDialog.Hide(); }
            catch (Exception error) when (error is IOException or InvalidDataException or HttpRequestException or OperationCanceledException)
            { ReportProblem(error.Message); DetailsDialog.Hide(); }
            finally { membershipBusy = false; IncludeWorkspace.IsEnabled = ExcludeWorkspace.IsEnabled = true; }
        }
        void StartMembership(bool included) { if (!membershipBusy) pendingMembership = Membership(included); }
        RoutedEventHandler includeClick = (_, _) => StartMembership(true);
        RoutedEventHandler excludeClick = (_, _) => StartMembership(false);
        DetailLinks.ItemClick += linkClick; DetailBacklinks.ItemClick += backClick; DetailHistory.ItemClick += historyClick;
        DetailAttachments.ItemClick += attachmentClick; IncludeWorkspace.Click += includeClick; ExcludeWorkspace.Click += excludeClick;
        try { await ShowDialogAsync(DetailsDialog); }
        finally
        {
            DetailLinks.ItemClick -= linkClick; DetailBacklinks.ItemClick -= backClick; DetailHistory.ItemClick -= historyClick;
            DetailAttachments.ItemClick -= attachmentClick; IncludeWorkspace.Click -= includeClick; ExcludeWorkspace.Click -= excludeClick;
        }
        await pendingMembership;
        if (target is not null) await ViewModel.OpenAsync(target);
        if (version is not null)
        {
            var saved = await Client.SavedAsync(version);
            if (ReferenceEquals(editor, ViewModel.Editor) && observed == ViewModel.Body && await ViewModel.CanLeaveAsync()
                && await ConfirmAsync(UiText.Get("RestoreVersionTitle"), UiText.Get("RestoreVersionBody"), UiText.Get("ActionRestore")))
            { live.SelectAll(); live.ReplaceSelection(saved.Body); await ViewModel.CanLeaveAsync(); }
        }
        if (original is not null) await ShowAttachmentAsync(await Client.AttachmentAsync(original));
        Editor.Focus(FocusState.Programmatic);
    });
    private async Task ShowAttachmentAsync(Attachment attachment)
    {
        var actions = new ComboBox { Header = attachment.Name, ItemsSource = new[] { UiText.Get("ActionOpenOriginal"), UiText.Get("ActionSaveOriginal"), UiText.Get("ActionDeletePermanently") }, SelectedIndex = 0 };
        AutomationProperties.SetAutomationId(actions, "AttachmentAction"); AutomationProperties.SetName(actions, attachment.Name);
        if (await ShowDialogAsync(Dialog(UiText.Get("AttachmentTitle"), actions, UiText.Get("ActionContinue"))) != ContentDialogResult.Primary) return;
        if (actions.SelectedIndex == 2)
        {
            if (await ConfirmAsync(UiText.Get("DeleteAttachmentTitle"), UiText.Get("DeleteAttachmentBody", attachment.Name), UiText.Get("ActionDeletePermanently"))) await Client.DeleteAttachmentAsync(attachment);
            return;
        }
        if (actions.SelectedIndex == 1)
        {
            if (!await ViewModel.CanLeaveAsync()) return;
            var picker = new FileSavePicker(App.Window.AppWindow.Id) { SuggestedFileName = Path.GetFileNameWithoutExtension(attachment.Name) };
            picker.FileTypeChoices.Add(UiText.Get("OriginalFileType"), new List<string> { Path.GetExtension(attachment.Id) });
            var file = await picker.PickSaveFileAsync(); if (file is not null) await Client.DownloadAsync(attachment, file.Path);
        }
        else
        {
            var path = Path.Combine(ApplicationData.Current.TemporaryFolder.Path, attachment.Id);
            await Client.DownloadAsync(attachment, path); await Launcher.LaunchFileAsync(await StorageFile.GetFileFromPathAsync(path));
        }
    }
}
