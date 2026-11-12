use super::*;
use std::{path::Path, time::Instant};

fn wait(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "native workflow timed out");
        for _ in 0..16 {
            if !glib::MainContext::default().pending() {
                break;
            }
            glib::MainContext::default().iteration(false);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn draw(window: &adw::ApplicationWindow, path: &Path) {
    let paintable = gtk::WidgetPaintable::new(Some(window));
    let painted = Rc::new(Cell::new(false));
    let frame = painted.clone();
    let clock = window.frame_clock().unwrap();
    let handler = clock.connect_after_paint(move |_| frame.set(true));
    window.queue_draw();
    wait(|| painted.get());
    clock.disconnect(handler);
    let mut rendered = None;
    wait(|| {
        let snapshot = gtk::Snapshot::new();
        paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);
        rendered = snapshot.to_node();
        rendered.is_some()
    });
    let node = rendered.unwrap();
    let texture = window.renderer().unwrap().render_texture(&node, None);
    texture.save_to_png(path).unwrap();
}

fn draft_rows_preserve_current_writing_and_expose_uncertain_recovery(
    application: &adw::Application,
) {
    let directory = tempfile::tempdir().unwrap();
    let ui = build(application, DraftStore::at(directory.path().to_owned())).unwrap();
    ui.install_actions(application);
    ui.connect_signals();
    let address = ServerAddress::parse("https://notes.example", false).unwrap();
    *ui.client.borrow_mut() = Some(Client::new(address.clone()).unwrap());
    let mut editor = Editor::create(address.clone(), ui.advance().unwrap());
    editor.edit("Listed writing".into(), &ui.store).unwrap();
    *ui.editor.borrow_mut() = Some(editor);
    ui.show_editor();
    *ui.collection.borrow_mut() = Collection::Drafts;
    ui.refresh(false);
    let row = ui.list.row_at_index(0).unwrap();
    let (ticket, _) = ui
        .editor
        .borrow_mut()
        .as_mut()
        .unwrap()
        .begin(&ui.store)
        .unwrap()
        .unwrap();
    ui.text.buffer().set_text("Latest writing");
    ui.list.emit_by_name::<()>("row-activated", &[&row]);
    assert_eq!(
        ui.editor.borrow().as_ref().unwrap().draft().body,
        "Latest writing"
    );
    assert_eq!(
        ui.editor.borrow().as_ref().unwrap().state().pending(),
        Some(ticket)
    );
    assert!(ui.persist());
    assert_eq!(
        ui.store.recover(&address).unwrap().drafts[0].body,
        "Latest writing"
    );

    let draft = ui.editor.borrow().as_ref().unwrap().draft().clone();
    *ui.editor.borrow_mut() = Some(Editor::recover(draft, ui.advance().unwrap()));
    ui.show_editor();
    assert!(ui.banner.is_revealed());
    assert_eq!(ui.banner.button_label().as_deref(), Some("Retry"));
    assert!(ui.banner.title().contains("uncertain"));

    let mut other = Editor::create(address.clone(), 100);
    other
        .edit("Cached other writing".into(), &ui.store)
        .unwrap();
    let mut selected = other.draft().clone();
    ui.refresh(false);
    let position = ui
        .entries
        .borrow()
        .iter()
        .position(|entry| matches!(entry, Entry::Draft(draft) if draft.name == selected.name))
        .unwrap();
    let row = ui.list.row_at_index(position as i32).unwrap();
    selected.body = "Latest other writing".into();
    ui.store.save(&selected).unwrap();
    ui.list.emit_by_name::<()>("row-activated", &[&row]);
    assert_eq!(
        ui.editor.borrow().as_ref().unwrap().draft().body,
        selected.body
    );
    ui.window.destroy();
}

#[test]
fn native_writing_navigation_recovery_input_and_adaptive_layout() {
    adw::init().unwrap();
    let application = adw::Application::builder()
        .application_id("io.github.p4suta.tungsten.tests")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    application.register(gio::Cancellable::NONE).unwrap();
    draft_rows_preserve_current_writing_and_expose_uncertain_recovery(&application);
    links_resolve_from_the_current_note_folder(&application);
    let local = tempfile::tempdir().unwrap();
    let drafts = local.path().join("drafts");
    let ui = build(&application, DraftStore::at(drafts.clone())).unwrap();
    ui.install_actions(&application);
    ui.connect_signals();
    ui.window.present();
    wait(|| ui.window.is_mapped());
    let directory = tempfile::tempdir().unwrap();
    let listener = ui
        .runtime
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let router = tungsten::api::app_with_origin(
        directory.path(),
        tungsten::api::ServerOrigin::development(&origin).unwrap(),
    )
    .unwrap();
    let server = ui.runtime.spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    ui.connect(ServerAddress::parse(&origin, true).unwrap());
    ui.create();
    assert!(ui.text.is_editable());
    ui.text.grab_focus();
    wait(|| ui.text.has_focus());
    let source = "# 日本語のノート\n\nMarkdown **source** stays editable.\n\n".to_owned()
        + &"A long paragraph wraps without horizontal scrolling. ".repeat(40);
    ui.text.buffer().set_text(&source);
    assert_eq!(
        ui.text
            .create_pango_layout(Some("日本語のノート"))
            .unknown_glyphs_count(),
        0
    );
    ui.text.emit_by_name::<()>("preedit-changed", &[&"かな"]);
    ui.save();
    assert!(!ui.persist());
    assert!(!ui.inner.content().unwrap().can_pop());
    assert!(
        ui.editor
            .borrow()
            .as_ref()
            .unwrap()
            .state()
            .pending()
            .is_none()
    );
    ui.text.emit_by_name::<()>("preedit-changed", &[&""]);
    ui.save();
    wait(|| ui.editor.borrow().as_ref().unwrap().state().is_saved());
    let name = ui.editor.borrow().as_ref().unwrap().draft().name.clone();
    wait(|| {
        ui.entries
            .borrow()
            .iter()
            .any(|entry| matches!(entry, Entry::Note(note) if note.name == name))
    });
    let current = ui.editor.borrow().as_ref().unwrap().draft().clone();
    let replacement = Editor::create(ui.client().unwrap().address().clone(), 100)
        .draft()
        .clone();
    let replacement = ui
        .runtime
        .block_on(ui.client().unwrap().save(&replacement))
        .unwrap();
    ui.open(replacement.name);
    let loading = ui.generation.get();
    assert!(!ui.text.is_editable());
    ui.recover(current.clone());
    assert!(ui.text.is_editable());
    assert_ne!(ui.generation.get(), loading);
    assert_eq!(
        ui.editor.borrow().as_ref().unwrap().draft().body,
        current.body
    );
    workspace_responses_refresh_selected_and_cached_revisions(&ui, &name);
    assert_eq!(
        ui.runtime
            .block_on(ui.client().unwrap().read(&name))
            .unwrap()
            .body,
        source
    );
    let read = ui.editor.borrow().as_ref().unwrap().history_read();
    ui.restore_history(&read, "# An earlier version".into())
        .unwrap();
    assert_eq!(
        ui.editor.borrow().as_ref().unwrap().draft().body,
        "# An earlier version"
    );
    ui.text.buffer().undo();
    assert_eq!(ui.editor.borrow().as_ref().unwrap().draft().body, source);
    ui.text.buffer().begin_user_action();
    ui.text.buffer().insert_at_cursor("追記");
    ui.text.buffer().end_user_action();
    assert!(ui.text.buffer().can_undo());
    ui.text.buffer().undo();
    assert_eq!(ui.editor.borrow().as_ref().unwrap().draft().body, source);
    ui.text.buffer().redo();
    assert!(
        ui.editor
            .borrow()
            .as_ref()
            .unwrap()
            .draft()
            .body
            .contains("追記")
    );
    let draft = ui.editor.borrow().as_ref().unwrap().draft().clone();
    ui.create();
    assert!(!ui.text.buffer().can_undo());
    ui.recover(draft.clone());
    assert_eq!(
        ui.editor.borrow().as_ref().unwrap().draft().body,
        draft.body
    );
    *ui.collection.borrow_mut() = Collection::Drafts;
    ui.refresh(false);
    assert!(!ui.entries.borrow().is_empty());
    ui.search.set_text("no matching text");
    ui.refresh(false);
    assert!(ui.entries.borrow().is_empty());
    ui.search.set_text("");
    *ui.collection.borrow_mut() = Collection::All;
    ui.refresh(false);
    wait(|| !ui.entries.borrow().is_empty());
    let screenshots = std::env::var_os("TUNGSTEN_SCREENSHOT_DIRECTORY")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| directory.path().join("screenshots"));
    std::fs::create_dir_all(&screenshots).unwrap();
    let style = adw::StyleManager::default();
    assert_eq!(
        style.is_high_contrast(),
        std::env::var("ADW_DEBUG_HIGH_CONTRAST").as_deref() == Ok("1")
    );
    style.set_color_scheme(adw::ColorScheme::ForceLight);
    draw(&ui.window, &screenshots.join("linux-light.png"));
    style.set_color_scheme(adw::ColorScheme::ForceDark);
    draw(&ui.window, &screenshots.join("linux-dark.png"));
    let settings = gtk::Settings::default().unwrap();
    settings.set_gtk_font_name(Some("Sans 18"));
    ui.window.set_default_size(400, 700);
    wait(|| ui.inner.is_collapsed() && ui.outer.is_collapsed());
    assert!(ui.text.width() > 100);
    assert!(
        ui.text
            .buffer()
            .text(
                &ui.text.buffer().start_iter(),
                &ui.text.buffer().end_iter(),
                true
            )
            .contains("追記")
    );
    draw(&ui.window, &screenshots.join("linux-narrow-large.png"));
    ui.inner.set_show_content(false);
    ui.outer.set_show_content(false);
    assert_eq!(
        ui.editor.borrow().as_ref().unwrap().draft().body,
        draft.body
    );
    assert!(ui.persist());
    std::fs::rename(&drafts, local.path().join("protected-drafts")).unwrap();
    std::fs::write(&drafts, b"storage unavailable").unwrap();
    ui.text
        .buffer()
        .insert_at_cursor(" retained after storage failure");
    let retained = ui.editor.borrow().as_ref().unwrap().draft().body.clone();
    assert!(!ui.persist());
    assert!(!ui.inner.content().unwrap().can_pop());
    ui.create();
    assert_eq!(ui.editor.borrow().as_ref().unwrap().draft().body, retained);
    ui.window.close();
    assert!(ui.window.is_visible());
    std::fs::remove_file(&drafts).unwrap();
    std::fs::rename(local.path().join("protected-drafts"), &drafts).unwrap();
    assert!(ui.persist());
    ui.window.close();
    server.abort();
}

fn links_resolve_from_the_current_note_folder(application: &adw::Application) {
    let directory = tempfile::tempdir().unwrap();
    let ui = build(application, DraftStore::at(directory.path().to_owned())).unwrap();
    ui.connect_signals();
    let address = ServerAddress::parse("https://notes.example", false).unwrap();
    for (source, target) in [
        ("Folder/Source.md", "Folder/Target.md"),
        ("Folder/Source.md", "Target.md"),
        ("Folder/Source.md", "Other/Target.md"),
        ("Source.md", "Folder/Target.md"),
        ("日本語/原文.md", "日本語/空白 #?[]%.md"),
        ("Target.md/Source.md", "Target.md"),
    ] {
        let scope = ui.advance().unwrap();
        let mut draft = Editor::create(address.clone(), scope).draft().clone();
        draft.name = tungsten_linux::NoteName::parse(source.to_owned()).unwrap();
        *ui.editor.borrow_mut() = Some(Editor::recover(draft, scope));
        ui.show_editor();
        ui.insert_link(&Summary {
            name: tungsten_linux::NoteName::parse(target.to_owned()).unwrap(),
            title: "Target [label]".into(),
            snippet: String::new(),
        });
        let body = ui.editor.borrow().as_ref().unwrap().draft().body.clone();
        assert_eq!(
            tungsten::markdown::preview(source, &body).links,
            vec![target],
            "{source}: {body}"
        );
    }
    ui.window.destroy();
}

fn action_row(widget: gtk::Widget) -> Option<adw::ActionRow> {
    if let Some(row) = widget.downcast_ref::<adw::ActionRow>() {
        return Some(row.clone());
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if let Some(row) = action_row(widget) {
            return Some(row);
        }
    }
    None
}

fn workspace_responses_refresh_selected_and_cached_revisions(
    ui: &Rc<Ui>,
    name: &tungsten_linux::NoteName,
) {
    let client = ui.client().unwrap();
    let original = ui
        .runtime
        .block_on(client.create_workspace("Writing workspace"))
        .unwrap();
    let populated = ui
        .runtime
        .block_on(client.membership(&original.id, name, true))
        .unwrap();
    ui.workspace_listing.set(ui.workspace_listing.get() + 1);
    *ui.workspaces.borrow_mut() = vec![original.clone()];
    *ui.collection.borrow_mut() = Collection::Workspace(original.clone());
    ui.entries.borrow_mut().clear();
    ui.refresh(false);
    wait(|| {
        ui.entries
            .borrow()
            .iter()
            .any(|entry| matches!(entry, Entry::Note(note) if &note.name == name))
    });
    let Collection::Workspace(selected) = ui.collection.borrow().clone() else {
        panic!("workspace selection changed");
    };
    assert_eq!(selected.revision, populated.revision);
    assert_eq!(ui.workspaces.borrow()[0].revision, populated.revision);

    ui.rename_workspace();
    let dialog = ui
        .window
        .visible_dialog()
        .and_downcast::<adw::AlertDialog>()
        .unwrap();
    dialog
        .extra_child()
        .and_downcast::<gtk::Entry>()
        .unwrap()
        .set_text("Renamed workspace");
    dialog.emit_by_name::<()>("response", &[&"accept"]);
    wait(
        || matches!(&*ui.collection.borrow(), Collection::Workspace(workspace) if workspace.title == "Renamed workspace"),
    );
    let renamed = ui
        .runtime
        .block_on(client.workspace_notes(&original.id, None))
        .unwrap()
        .workspace;
    wait(|| {
        ui.workspaces
            .borrow()
            .first()
            .is_some_and(|workspace| workspace.revision == renamed.revision)
    });

    ui.remove_membership();
    wait(|| ui.entries.borrow().is_empty());
    let removed = ui
        .runtime
        .block_on(client.workspace_notes(&original.id, None))
        .unwrap()
        .workspace;
    assert_ne!(removed.revision, renamed.revision);
    wait(|| {
        ui.workspaces
            .borrow()
            .first()
            .is_some_and(|workspace| workspace.revision == removed.revision)
    });
    assert!(
        matches!(&*ui.collection.borrow(), Collection::Workspace(workspace) if workspace.revision == removed.revision)
    );

    ui.membership();
    let dialog = ui.window.visible_dialog().unwrap();
    action_row(dialog.child().unwrap())
        .unwrap()
        .emit_by_name::<()>("activated", &[]);
    wait(
        || matches!(&*ui.collection.borrow(), Collection::Workspace(workspace) if workspace.revision != removed.revision),
    );
    let added = ui
        .runtime
        .block_on(client.workspace_notes(&original.id, None))
        .unwrap()
        .workspace;
    wait(|| {
        ui.workspaces
            .borrow()
            .first()
            .is_some_and(|workspace| workspace.revision == added.revision)
    });
    ui.runtime
        .block_on(client.delete_workspace(&added))
        .unwrap();
    *ui.collection.borrow_mut() = Collection::All;
    ui.refresh(false);
}
