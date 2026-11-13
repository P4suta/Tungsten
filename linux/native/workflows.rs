use super::*;
use std::{path::Path, time::Instant};

#[track_caller]
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
    history_sync_failures_preserve_the_displayed_text(&application);
    workspace_reads_keep_metadata_from_the_newer_observation(&application);
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

fn history_sync_failures_preserve_the_displayed_text(application: &adw::Application) {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let directory = tempfile::tempdir().unwrap();
    let fail_on = Arc::new(AtomicUsize::new(0));
    let armed = fail_on.clone();
    let store = DraftStore::with_directory_sync(directory.path().to_owned(), move |path| {
        if armed
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok_and(|remaining| remaining == 1)
        {
            return Err(std::io::Error::other(
                "directory sync failed after replacement",
            ));
        }
        std::fs::File::open(path)?.sync_all()
    });
    let ui = build(application, store).unwrap();
    ui.connect_signals();
    let address = ServerAddress::parse("https://notes.example", false).unwrap();
    let mut editor = Editor::create(address.clone(), ui.advance().unwrap());
    editor.edit("Visible writing".into(), &ui.store).unwrap();
    *ui.editor.borrow_mut() = Some(editor);
    ui.show_editor();
    let read = ui.editor.borrow().as_ref().unwrap().history_read();
    fail_on.store(1, Ordering::SeqCst);
    assert!(ui.restore_history(&read, "Historical".into()).is_err());
    assert_eq!(
        ui.editor.borrow().as_ref().unwrap().draft().body,
        "Visible writing"
    );
    assert_eq!(
        ui.text.buffer().text(
            &ui.text.buffer().start_iter(),
            &ui.text.buffer().end_iter(),
            true
        ),
        "Visible writing"
    );
    assert_eq!(
        ui.store.recover(&address).unwrap().drafts[0].body,
        "Visible writing"
    );
    ui.editor
        .borrow_mut()
        .as_mut()
        .unwrap()
        .persist(&ui.store)
        .unwrap();
    let read = ui.editor.borrow().as_ref().unwrap().history_read();
    fail_on.store(2, Ordering::SeqCst);
    assert!(ui.restore_history(&read, "Historical".into()).is_err());
    assert_eq!(
        ui.editor.borrow().as_ref().unwrap().draft().body,
        "Historical"
    );
    assert_eq!(
        ui.text.buffer().text(
            &ui.text.buffer().start_iter(),
            &ui.text.buffer().end_iter(),
            true
        ),
        "Historical"
    );
    assert!(!ui.editor.borrow().as_ref().unwrap().state().may_leave());
    assert_eq!(
        ui.store.recover(&address).unwrap().drafts[0].body,
        "Historical"
    );
    ui.window.destroy();
}

fn workspace_reads_keep_metadata_from_the_newer_observation(application: &adw::Application) {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    };
    struct Replies {
        hold: AtomicU8,
        defer_read: AtomicBool,
        ready: AtomicBool,
        release: tokio::sync::Notify,
    }
    let replies = Arc::new(Replies {
        hold: AtomicU8::new(0),
        defer_read: AtomicBool::new(false),
        ready: AtomicBool::new(false),
        release: tokio::sync::Notify::new(),
    });
    let directory = tempfile::tempdir().unwrap();
    let ui = build(application, DraftStore::at(directory.path().join("drafts"))).unwrap();
    ui.connect_signals();
    ui.window.present();
    wait(|| ui.window.is_mapped());
    let listener = ui
        .runtime
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let address = ServerAddress::parse(&origin, true).unwrap();
    let client = Client::new(address.clone()).unwrap();
    *ui.client.borrow_mut() = Some(client.clone());
    let gate = replies.clone();
    let router = tungsten::api::app_with_origin(
        &directory.path().join("server"),
        tungsten::api::ServerOrigin::development(&origin).unwrap(),
    )
    .unwrap()
    .layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            let gate = gate.clone();
            async move {
                let kind = if request.method() != axum::http::Method::GET {
                    0
                } else if request.uri().path() == "/api/workspaces" {
                    1
                } else if request.uri().path().starts_with("/api/workspaces/")
                    && request.uri().path().ends_with("/notes")
                {
                    2
                } else {
                    0
                };
                let held = kind != 0
                    && gate
                        .hold
                        .compare_exchange(kind, 0, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok();
                let deferred = held && gate.defer_read.swap(false, Ordering::SeqCst);
                if deferred {
                    gate.ready.store(true, Ordering::SeqCst);
                    gate.release.notified().await;
                }
                let response = next.run(request).await;
                if held && !deferred {
                    gate.ready.store(true, Ordering::SeqCst);
                    gate.release.notified().await;
                }
                response
            }
        },
    ));
    let server = ui.runtime.spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let draft = Editor::create(address.clone(), 1).draft().clone();
    let note = ui.runtime.block_on(client.save(&draft)).unwrap();
    let workspace = ui
        .runtime
        .block_on(client.create_workspace("Response ordering"))
        .unwrap();
    ui.load_workspaces(None);
    wait(|| ui.workspaces.borrow().len() == 1);
    *ui.collection.borrow_mut() = Collection::Workspace(workspace.clone());
    replies.hold.store(1, Ordering::SeqCst);
    ui.load_workspaces(None);
    wait(|| replies.ready.load(Ordering::SeqCst));
    let populated = ui
        .runtime
        .block_on(client.membership(&workspace.id, &note.name, true))
        .unwrap();
    ui.refresh(false);
    wait(
        || matches!(&*ui.collection.borrow(), Collection::Workspace(current) if current.revision == populated.revision),
    );
    replies.release.notify_one();
    wait(|| ui.workspaces.borrow().len() == 1);
    assert_eq!(ui.workspaces.borrow()[0].revision, populated.revision);
    assert!(
        matches!(&*ui.collection.borrow(), Collection::Workspace(current) if current.revision == populated.revision)
    );

    replies.ready.store(false, Ordering::SeqCst);
    replies.hold.store(2, Ordering::SeqCst);
    ui.refresh(false);
    wait(|| replies.ready.load(Ordering::SeqCst));
    let removed = ui
        .runtime
        .block_on(client.membership(&workspace.id, &note.name, false))
        .unwrap();
    ui.load_workspaces(None);
    wait(
        || matches!(&*ui.collection.borrow(), Collection::Workspace(current) if current.revision == removed.revision),
    );
    replies.release.notify_one();
    wait(|| ui.entries.borrow().is_empty() && ui.more.is_sensitive());
    assert_eq!(ui.workspaces.borrow()[0].revision, removed.revision);
    assert!(
        matches!(&*ui.collection.borrow(), Collection::Workspace(current) if current.revision == removed.revision)
    );
    replies.ready.store(false, Ordering::SeqCst);
    replies.defer_read.store(true, Ordering::SeqCst);
    replies.hold.store(1, Ordering::SeqCst);
    ui.load_workspaces(None);
    wait(|| replies.ready.load(Ordering::SeqCst));
    let populated = ui
        .runtime
        .block_on(client.membership(&workspace.id, &note.name, true))
        .unwrap();
    ui.refresh(false);
    wait(
        || matches!(&*ui.collection.borrow(), Collection::Workspace(current) if current.revision == populated.revision),
    );
    let changed = ui
        .runtime
        .block_on(client.rename_workspace(&populated, "Changed while reading"))
        .unwrap();
    replies.release.notify_one();
    wait(|| ui.workspaces.borrow().len() == 1);
    assert_eq!(ui.workspaces.borrow()[0].revision, changed.revision);
    assert!(
        matches!(&*ui.collection.borrow(), Collection::Workspace(current) if current.revision == changed.revision)
    );
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
        .set_text("Renamed through UI");
    dialog.emit_by_name::<()>("response", &[&"accept"]);
    wait(
        || matches!(&*ui.collection.borrow(), Collection::Workspace(current) if current.title == "Renamed through UI"),
    );
    wait(|| {
        ui.more.is_sensitive()
            && ui
                .workspaces
                .borrow()
                .first()
                .is_some_and(|current| current.title == "Renamed through UI")
    });
    assert_eq!(
        ui.runtime
            .block_on(client.workspace_notes(&workspace.id, None))
            .unwrap()
            .workspace
            .title,
        "Renamed through UI"
    );
    server.abort();
    ui.window.destroy();
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
        ("Source.md", "Meeting: Notes.md"),
        ("Area: Notes/Source.md", "Area: Notes/Target: Note.md"),
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
