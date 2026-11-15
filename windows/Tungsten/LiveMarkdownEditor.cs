using Microsoft.UI.Text;
using Microsoft.UI.Xaml.Controls;
using Tungsten.Core;
using Windows.ApplicationModel.DataTransfer;

namespace Tungsten;

public sealed class LiveMarkdownEditor(RichEditBox control)
{
    private MarkdownSource text = new("");
    private MarkdownHistory history = new("");
    private MarkdownSpan[]? rendered;
    private bool updating;
    private bool composing, atomic;
    public event EventHandler<Exception>? PresentationFailed;
    public event EventHandler? SourceChanged;
    public string Source => text.Source;
    public bool Composing
    {
        get => composing;
        set { if (value == composing) return; composing = value; if (value) history.BeginComposition(); else history.EndComposition(); }
    }
    public bool CanUndo => history.CanUndo;
    public bool CanRedo => history.CanRedo;
    public void Load(string source, bool resetHistory)
    {
        if (Source == source && !resetHistory) return;
        var selection = control.Document.Selection;
        var start = selection.StartPosition; var end = selection.EndPosition;
        updating = true;
        try
        {
            text = new(source); control.Document.SetText(TextSetOptions.None, text.NativeText);
            selection.SetRange(Math.Min(start, text.NativeText.Length), Math.Min(end, text.NativeText.Length));
            history = new(source); history.Select(CurrentSelection());
            control.TextDocument.UndoLimit = 0; rendered = null;
        }
        finally { updating = false; }
        Render(true);
    }
    public bool ReadChanges()
    {
        if (updating) return false;
        var range = control.Document.GetRange(0, Math.Max(0, control.Document.Selection.StoryLength - 1));
        range.GetText(TextGetOptions.None, out var native);
        if (native == text.NativeText) return false;
        text.Update(native); history.Record(Source, CurrentSelection(), !atomic); rendered = null;
        SourceChanged?.Invoke(this, EventArgs.Empty);
        return true;
    }
    private TextSelection CurrentSelection()
    {
        var selection = control.Document.Selection;
        var start = text.ToSource(selection.StartPosition); var end = text.ToSource(selection.EndPosition);
        return new(start, end - start);
    }
    public void TrackSelection() { if (!updating) history.Select(CurrentSelection()); }
    public void ReplaceSelection(string source)
    {
        history.BreakGroup(); atomic = true;
        try
        {
            var selection = control.Document.Selection;
            selection.SetText(TextSetOptions.None, MarkdownSource.Normalize(source));
            ReadChanges();
            var end = selection.EndPosition; selection.SetRange(end, end);
            history.AmendSelection(CurrentSelection());
        }
        finally { atomic = false; history.BreakGroup(); }
    }
    public bool Undo(bool redo = false)
    {
        if (Composing || !(redo ? CanRedo : CanUndo)) return false;
        var state = redo ? history.Redo() : history.Undo();
        updating = true;
        try
        {
            text = new(state.Source); control.Document.SetText(TextSetOptions.None, text.NativeText);
            control.Document.Selection.SetRange(text.ToNative(state.Selection.Start), text.ToNative(state.Selection.Start + state.Selection.Length));
            rendered = null;
        }
        finally { updating = false; }
        Render(true); return true;
    }
    public void SelectAll() => control.Document.Selection.SetRange(0, text.NativeText.Length);
    public void Format(MarkdownStyle style)
    {
        if (Composing) return;
        var selection = control.Document.Selection;
        var start = selection.StartPosition; var end = selection.EndPosition;
        var sourceStart = text.ToSource(start); var sourceEnd = text.ToSource(end);
        var insertion = MarkdownFormatting.Wrap(Source, sourceStart, sourceEnd - sourceStart, style);
        ReplaceSelection(insertion.Text);
        var selectedStart = start + MarkdownSource.Normalize(insertion.Text[..insertion.SelectionStart]).Length;
        var selectedLength = MarkdownSource.Normalize(insertion.Text.Substring(insertion.SelectionStart, insertion.SelectionLength)).Length;
        selection.SetRange(selectedStart, selectedStart + selectedLength);
        history.AmendSelection(CurrentSelection());
        control.Focus(Microsoft.UI.Xaml.FocusState.Programmatic);
    }
    public void Copy(bool cut)
    {
        var selection = control.Document.Selection;
        var start = text.ToSource(selection.StartPosition); var end = text.ToSource(selection.EndPosition);
        if (start == end) return;
        var data = new DataPackage(); data.SetText(Source[start..end]); Clipboard.SetContent(data);
        if (cut) ReplaceSelection("");
    }
    public async Task PasteAsync()
    {
        var observedHistory = history; var observed = Source; var selection = CurrentSelection();
        var data = Clipboard.GetContent();
        if (!data.Contains(StandardDataFormats.Text)) return;
        var insertion = await data.GetTextAsync();
        if (Composing || !ReferenceEquals(history, observedHistory) || Source != observed || CurrentSelection() != selection)
            throw new InvalidOperationException(UiText.Get("PasteChanged"));
        Rules.LocalBody(observed[..selection.Start] + insertion + observed[(selection.Start + selection.Length)..]);
        ReplaceSelection(insertion);
    }
    public void Render(bool force = false)
    {
        if (updating || Composing) return;
        var selection = control.Document.Selection;
        var focused = control.FocusState != Microsoft.UI.Xaml.FocusState.Unfocused;
        var spans = LiveMarkdown.Project(Source, focused ? text.ToSource(selection.StartPosition) : -1, focused ? text.ToSource(selection.EndPosition) : -1);
        if (!force && rendered is not null && rendered.SequenceEqual(spans)) return;
        updating = true; control.TextDocument.BatchDisplayUpdates();
        try
        {
            var all = control.Document.GetRange(0, text.NativeText.Length);
            var defaults = control.Document.GetDefaultCharacterFormat();
            all.CharacterFormat.SetClone(defaults);
            all.CharacterFormat.Hidden = FormatEffect.Off;
            all.ParagraphFormat.SetClone(control.Document.GetDefaultParagraphFormat());
            foreach (var span in spans)
            {
                var range = control.Document.GetRange(text.ToNative(span.Start), text.ToNative(span.End));
                var format = range.CharacterFormat;
                switch (span.Style)
                {
                    case MarkdownStyle.Heading: format.Bold = FormatEffect.On; format.Size = defaults.Size * (span.Level <= 2 ? 1.5f : 1.2f); break;
                    case MarkdownStyle.Bold: format.Bold = FormatEffect.On; break;
                    case MarkdownStyle.Italic: format.Italic = FormatEffect.On; break;
                    case MarkdownStyle.Code: format.BackgroundColor = (Windows.UI.Color)Microsoft.UI.Xaml.Application.Current.Resources["SolidBackgroundFillColorBase"]; break;
                    case MarkdownStyle.Link: format.Underline = UnderlineType.Single; break;
                    case MarkdownStyle.Hidden: format.Hidden = FormatEffect.On; break;
                    case MarkdownStyle.Strike: format.Strikethrough = FormatEffect.On; break;
                    case MarkdownStyle.Bullet: range.ParagraphFormat.ListType = MarkerType.Bullet; break;
                    case MarkdownStyle.Number: range.ParagraphFormat.ListType = MarkerType.Arabic; range.ParagraphFormat.ListStart = span.Level; break;
                    case MarkdownStyle.Quote: format.Italic = FormatEffect.On; break;
                }
            }
            rendered = spans;
        }
        catch (Exception error) when (error is System.Runtime.InteropServices.COMException or InvalidCastException)
        { PresentationFailed?.Invoke(this, error); }
        finally { control.TextDocument.ApplyDisplayUpdates(); updating = false; }
    }
}
