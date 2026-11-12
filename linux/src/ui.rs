use adw::prelude::*;
use gtk::{gio, glib};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    rc::Rc,
    time::Duration,
};
use tungsten_linux::{
    APP_ID, Error, Result,
    client::Client,
    drafts::{Draft, DraftStore},
    editor::Editor,
    types::{self, Note, SavedNote, ServerAddress, Summary, Workspace},
};
use tungsten_linux_model::Block;

#[derive(Clone)]
enum Collection {
    All,
    Drafts,
    Trash,
    Workspace(Workspace),
}

#[derive(Clone)]
enum Entry {
    Note(Summary),
    Draft(Draft),
    Trash(SavedNote),
}

pub struct Ui {
    window: adw::ApplicationWindow,
    runtime: tokio::runtime::Runtime,
    store: DraftStore,
    client: RefCell<Option<Client>>,
    editor: RefCell<Option<Editor>>,
    generation: Cell<u64>,
    connection: Cell<u64>,
    listing: Cell<u64>,
    workspace_listing: Cell<u64>,
    collection: RefCell<Collection>,
    entries: RefCell<Vec<Entry>>,
    cursor: RefCell<Option<String>>,
    workspaces: RefCell<Vec<Workspace>>,
    workspace_section: adw::SidebarSection,
    sidebar: adw::Sidebar,
    outer: adw::NavigationSplitView,
    inner: adw::NavigationSplitView,
    list: gtk::ListBox,
    list_title: adw::WindowTitle,
    search: gtk::SearchEntry,
    more: gtk::Button,
    empty: adw::StatusPage,
    text: gtk::TextView,
    heading: adw::WindowTitle,
    status: gtk::Label,
    banner: adw::Banner,
    changing: Cell<bool>,
    composing: Cell<bool>,
    downloads: RefCell<Vec<tempfile::TempPath>>,
}

type WindowAction = (&'static str, fn(&Rc<Ui>));

fn button(icon: &str, label: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(label)
        .build();
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    button
}

fn scrolled(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(child)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build()
}

fn toolbar(
    title: &str,
    child: &impl IsA<gtk::Widget>,
) -> (adw::ToolbarView, adw::HeaderBar, adw::WindowTitle) {
    let label = adw::WindowTitle::new(title, "");
    let header = adw::HeaderBar::builder().title_widget(&label).build();
    let view = adw::ToolbarView::builder().content(child).build();
    view.add_top_bar(&header);
    (view, header, label)
}

pub fn activate(application: &adw::Application) -> Result<()> {
    if let Some(window) = application.active_window() {
        window.present();
        return Ok(());
    }
    let ui = build(application, DraftStore::xdg()?)?;
    ui.install_actions(application);
    ui.connect_signals();
    ui.window.present();
    if let Some(address) = std::env::var("TUNGSTEN_SERVER")
        .ok()
        .and_then(|value| ServerAddress::parse(&value, cfg!(debug_assertions)).ok())
    {
        ui.connect(address);
    } else {
        match ui.store.remembered_server() {
            Ok(Some(address)) => ui.connect(address),
            Ok(None) => ui.settings(),
            Err(error) => {
                ui.error(&error);
                ui.settings();
            }
        }
    }
    let keep = ui.clone();
    application.connect_shutdown(move |_| {
        let _ = keep.persist();
    });
    Ok(())
}

fn build(application: &adw::Application, store: DraftStore) -> Result<Rc<Ui>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(Error::Storage)?;
    let sidebar = adw::Sidebar::new();
    let collections = adw::SidebarSection::new();
    for (label, icon) in [
        ("All Notes", "folder-documents-symbolic"),
        ("Drafts", "document-edit-symbolic"),
        ("Trash", "user-trash-symbolic"),
    ] {
        collections.append(
            adw::SidebarItem::builder()
                .title(label)
                .icon_name(icon)
                .build(),
        );
    }
    sidebar.append(collections);
    let workspace_section = adw::SidebarSection::new();
    workspace_section.set_title(Some("Workspaces"));
    sidebar.append(workspace_section.clone());
    let (collections, collections_header, _) = toolbar("Collections", &sidebar);
    let settings = button("emblem-system-symbolic", "Connection Settings");
    settings.set_action_name(Some("win.settings"));
    collections_header.pack_end(&settings);
    let workspace = button("folder-new-symbolic", "New Workspace");
    workspace.set_action_name(Some("win.new-workspace"));
    collections_header.pack_start(&workspace);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .activate_on_single_click(true)
        .build();
    list.add_css_class("navigation-sidebar");
    let empty = adw::StatusPage::builder()
        .title("No Notes")
        .description("Connect to a server, then create a note or search your notes.")
        .icon_name("folder-documents-symbolic")
        .build();
    let list_stack = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search Notes")
        .build();
    search.update_property(&[gtk::accessible::Property::Label("Search Notes")]);
    list_stack.append(&search);
    list_stack.append(&empty);
    list_stack.append(&scrolled(&list));
    let more = gtk::Button::with_label("Load More");
    more.set_visible(false);
    list_stack.append(&more);
    let (notes, notes_header, list_title) = toolbar("All Notes", &list_stack);
    let create = button("document-new-symbolic", "New Note");
    create.set_action_name(Some("win.new"));
    notes_header.pack_end(&create);
    let collection_menu = gio::Menu::new();
    for (label, action) in [
        ("Rename Workspace", "win.rename-workspace"),
        ("Delete Workspace", "win.delete-workspace"),
        ("Empty Trash", "win.empty-trash"),
    ] {
        collection_menu.append(Some(label), Some(action));
    }
    let collection_button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text("Collection Menu")
        .menu_model(&collection_menu)
        .build();
    collection_button.update_property(&[gtk::accessible::Property::Label("Collection Menu")]);
    notes_header.pack_end(&collection_button);

    let text = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::WordChar)
        .editable(false)
        .accepts_tab(false)
        .top_margin(18)
        .bottom_margin(18)
        .left_margin(18)
        .right_margin(18)
        .hexpand(true)
        .vexpand(true)
        .build();
    text.buffer().set_enable_undo(true);
    text.update_property(&[gtk::accessible::Property::Label("Markdown Source")]);
    let (editor, editor_header, heading) = toolbar("Select a Note", &scrolled(&text));
    let status = gtk::Label::builder()
        .label("Connect to a server to begin.")
        .wrap(true)
        .accessible_role(gtk::AccessibleRole::Status)
        .build();
    editor.add_bottom_bar(&status);
    let banner = adw::Banner::builder().button_label("Retry").build();
    editor.add_top_bar(&banner);
    let menu = gio::Menu::new();
    for (label, action) in [
        ("Note Details", "win.details"),
        ("Save", "win.save"),
        ("Reload", "win.reload"),
        ("Save as New Note", "win.copy"),
        ("Insert Link", "win.link"),
        ("Attach File", "win.attach"),
        ("Add to Workspace", "win.membership"),
        ("Remove from Workspace", "win.remove-membership"),
        ("Move to Trash", "win.delete"),
        ("Connection Settings", "win.settings"),
    ] {
        menu.append(Some(label), Some(action));
    }
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Note Menu")
        .menu_model(&menu)
        .build();
    menu_button.update_property(&[gtk::accessible::Property::Label("Note Menu")]);
    editor_header.pack_end(&menu_button);

    let inner = adw::NavigationSplitView::builder()
        .sidebar(&adw::NavigationPage::new(&notes, "Notes"))
        .content(&adw::NavigationPage::new(&editor, "Note"))
        .min_sidebar_width(240.0)
        .max_sidebar_width(360.0)
        .build();
    let outer = adw::NavigationSplitView::builder()
        .sidebar(&adw::NavigationPage::new(&collections, "Collections"))
        .content(&adw::NavigationPage::new(&inner, "Notes"))
        .min_sidebar_width(180.0)
        .max_sidebar_width(260.0)
        .build();
    let window = adw::ApplicationWindow::builder()
        .application(application)
        .title("Tungsten")
        .icon_name(APP_ID)
        .default_width(1100)
        .default_height(760)
        .content(&outer)
        .build();
    let medium = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        1000.0,
        adw::LengthUnit::Sp,
    ));
    medium.add_setter(&outer, "collapsed", Some(&true.to_value()));
    window.add_breakpoint(medium);
    let narrow = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        650.0,
        adw::LengthUnit::Sp,
    ));
    narrow.add_setter(&inner, "collapsed", Some(&true.to_value()));
    narrow.add_setter(&outer, "collapsed", Some(&true.to_value()));
    window.add_breakpoint(narrow);
    Ok(Rc::new(Ui {
        window,
        runtime,
        store,
        client: RefCell::new(None),
        editor: RefCell::new(None),
        generation: Cell::new(0),
        connection: Cell::new(0),
        listing: Cell::new(0),
        workspace_listing: Cell::new(0),
        collection: RefCell::new(Collection::All),
        entries: RefCell::new(Vec::new()),
        cursor: RefCell::new(None),
        workspaces: RefCell::new(Vec::new()),
        workspace_section,
        sidebar,
        outer,
        inner,
        list,
        list_title,
        search,
        more,
        empty,
        text,
        heading,
        status,
        banner,
        changing: Cell::new(false),
        composing: Cell::new(false),
        downloads: RefCell::new(Vec::new()),
    }))
}

impl Ui {
    fn task<T: Send + 'static>(
        self: &Rc<Self>,
        future: impl Future<Output = Result<T>> + Send + 'static,
        finish: impl FnOnce(&Rc<Self>, Result<T>) + 'static,
    ) {
        let (send, receive) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let _ = send.send(future.await);
        });
        let weak = Rc::downgrade(self);
        let connection = self.connection.get();
        glib::MainContext::default().spawn_local(async move {
            if let (Some(ui), Ok(result)) = (weak.upgrade(), receive.await) {
                if ui.connection.get() != connection {
                    return;
                }
                finish(&ui, result);
            }
        });
    }

    fn error(&self, error: &Error) {
        self.guard_navigation();
        self.banner.set_title(&error.to_string());
        self.banner.set_revealed(true);
        self.set_status(&error.to_string());
    }

    fn set_status(&self, message: &str) {
        if self.status.text().as_str() == message {
            return;
        }
        self.status.set_label(message);
        self.status
            .announce(message, gtk::AccessibleAnnouncementPriority::Medium);
    }

    fn client(&self) -> Result<Client> {
        self.client.borrow().clone().ok_or(Error::Address)
    }

    fn advance(&self) -> Result<u64> {
        let generation = self
            .generation
            .get()
            .checked_add(1)
            .ok_or(Error::Generation)?;
        self.generation.set(generation);
        Ok(generation)
    }

    fn persist(&self) -> bool {
        if self.composing.get() {
            self.status
                .set_label("Finish text composition before leaving the editor.");
            return false;
        }
        let result = self
            .editor
            .borrow_mut()
            .as_mut()
            .map(|editor| editor.persist(&self.store));
        if let Some(Err(error)) = result {
            self.error(&error);
            return false;
        }
        true
    }

    fn guard_navigation(&self) {
        let may_leave = !self.composing.get()
            && self
                .editor
                .borrow()
                .as_ref()
                .is_none_or(|editor| editor.state().may_leave());
        if let Some(page) = self.inner.content() {
            page.set_can_pop(may_leave);
        }
        if let Some(page) = self.outer.content() {
            page.set_can_pop(may_leave);
        }
    }

    fn install_actions(self: &Rc<Self>, application: &adw::Application) {
        let actions: &[WindowAction] = &[
            ("new", Self::create),
            ("save", Self::save),
            ("settings", Self::settings),
            ("reload", Self::reload),
            ("copy", Self::copy),
            ("details", Self::details),
            ("delete", Self::delete),
            ("attach", Self::attach),
            ("link", Self::link),
            ("new-workspace", Self::new_workspace),
            ("membership", Self::membership),
            ("rename-workspace", Self::rename_workspace),
            ("delete-workspace", Self::delete_workspace),
            ("remove-membership", Self::remove_membership),
            ("empty-trash", Self::empty_trash),
            ("search", |ui| {
                if !ui.persist() {
                    return;
                }
                ui.outer.set_show_content(true);
                ui.inner.set_show_content(false);
                ui.search.grab_focus();
            }),
        ];
        for (name, perform) in actions {
            let action = gio::SimpleAction::new(name, None);
            let weak = Rc::downgrade(self);
            let perform = *perform;
            action.connect_activate(move |_, _| {
                if let Some(ui) = weak.upgrade() {
                    perform(&ui);
                }
            });
            self.window.add_action(&action);
        }
        for (action, keys) in [
            ("win.new", &["<Control>n"][..]),
            ("win.save", &["<Control>s"][..]),
            ("win.search", &["<Control>f"][..]),
            ("win.reload", &["F5"][..]),
            ("win.settings", &["<Control>comma"][..]),
            ("window.close", &["<Control>w"][..]),
        ] {
            application.set_accels_for_action(action, keys);
        }
    }

    fn connect_signals(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.text.connect_preedit_changed(move |_, preedit| {
            if let Some(ui) = weak.upgrade() {
                ui.composing.set(!preedit.is_empty());
                ui.guard_navigation();
            }
        });
        let weak = Rc::downgrade(self);
        self.window
            .connect_close_request(move |_| match weak.upgrade() {
                Some(ui) if !ui.persist() => glib::Propagation::Stop,
                _ => glib::Propagation::Proceed,
            });
        let weak = Rc::downgrade(self);
        self.text.buffer().connect_changed(move |buffer| {
            if let Some(ui) = weak.upgrade() {
                if ui.changing.get() {
                    return;
                }
                let body = buffer
                    .text(&buffer.start_iter(), &buffer.end_iter(), true)
                    .to_string();
                let result = ui
                    .editor
                    .borrow_mut()
                    .as_mut()
                    .map(|editor| editor.edit(body, &ui.store));
                if let Some(Err(error)) = result {
                    ui.error(&error);
                } else {
                    ui.update_status();
                }
            }
        });
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(600), move || {
            let Some(ui) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            ui.save();
            glib::ControlFlow::Continue
        });
        let weak = Rc::downgrade(self);
        self.banner.connect_button_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.retry();
            }
        });
        let weak = Rc::downgrade(self);
        self.sidebar.connect_activated(move |_, index| {
            if let Some(ui) = weak.upgrade() {
                if !ui.persist() {
                    return;
                }
                let collection = match index {
                    0 => Collection::All,
                    1 => Collection::Drafts,
                    2 => Collection::Trash,
                    value => match ui
                        .workspaces
                        .borrow()
                        .get(value.saturating_sub(3) as usize)
                        .cloned()
                    {
                        Some(workspace) => Collection::Workspace(workspace),
                        None => return,
                    },
                };
                *ui.collection.borrow_mut() = collection;
                ui.refresh(false);
                ui.outer.set_show_content(true);
                ui.inner.set_show_content(false);
            }
        });
        let weak = Rc::downgrade(self);
        self.search.connect_search_changed(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.refresh(false);
            }
        });
        let weak = Rc::downgrade(self);
        self.more.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.refresh(true);
            }
        });
        let weak = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, row| {
            if let Some(ui) = weak.upgrade() {
                let entry = ui.entries.borrow().get(row.index() as usize).cloned();
                match entry {
                    Some(Entry::Note(note)) => ui.open(note.name),
                    Some(Entry::Draft(draft)) => ui.recover(draft),
                    Some(Entry::Trash(record)) => ui.preview_record(record, true),
                    None => {}
                }
            }
        });
    }

    fn update_status(&self) {
        self.guard_navigation();
        if let Some(editor) = self.editor.borrow().as_ref() {
            self.heading.set_title(&types::title(&editor.draft().body));
            let state = editor.state();
            self.set_status(if !state.may_leave() {
                "Not saved on this device. Keep the editor open."
            } else if state.pending().is_some() {
                "Saving…"
            } else if state.is_saved() {
                "Saved"
            } else {
                match state.block() {
                    Block::None => "Draft saved on this device",
                    Block::Conflict => "Conflict. Reload or save as a new note.",
                    Block::Missing => "Deleted on another device. Save as a new note.",
                    Block::Uncertain => "Delivery is uncertain. Check the server before retrying.",
                }
            });
        }
    }

    fn show_editor(&self) {
        self.changing.set(true);
        if let Some(editor) = self.editor.borrow().as_ref() {
            self.text.buffer().set_text(&editor.draft().body);
            self.text.set_editable(true);
        } else {
            self.text.buffer().set_text("");
            self.text.set_editable(false);
        }
        self.changing.set(false);
        let uncertain = self
            .editor
            .borrow()
            .as_ref()
            .is_some_and(|editor| editor.state().block() == Block::Uncertain);
        if uncertain {
            self.banner.set_title("Delivery is uncertain");
        }
        self.banner.set_revealed(uncertain);
        self.update_status();
        self.outer.set_show_content(true);
        self.inner.set_show_content(true);
        self.text.grab_focus();
    }

    fn connect(self: &Rc<Self>, address: ServerAddress) {
        if !self.persist() {
            return;
        }
        if let Err(error) = self.store.remember_server(&address) {
            self.error(&error);
            return;
        }
        match Client::new(address) {
            Ok(client) => {
                if let Err(error) = self.advance() {
                    self.error(&error);
                    return;
                }
                let Some(connection) = self.connection.get().checked_add(1) else {
                    self.error(&Error::Generation);
                    return;
                };
                self.connection.set(connection);
                let dialogs = self.window.dialogs();
                let open: Vec<_> = (0..dialogs.n_items())
                    .filter_map(|index| dialogs.item(index).and_downcast::<adw::Dialog>())
                    .collect();
                for dialog in open {
                    dialog.force_close();
                }
                *self.client.borrow_mut() = Some(client);
                *self.editor.borrow_mut() = None;
                *self.collection.borrow_mut() = Collection::All;
                self.show_editor();
                self.refresh(false);
                self.load_workspaces(None);
            }
            Err(error) => self.error(&error),
        }
    }

    fn create(self: &Rc<Self>) {
        if !self.persist() {
            return;
        }
        let result = (|| -> Result<Editor> {
            let mut editor = Editor::create(self.client()?.address().clone(), self.advance()?);
            editor.persist(&self.store)?;
            Ok(editor)
        })();
        match result {
            Ok(editor) => {
                *self.editor.borrow_mut() = Some(editor);
                self.show_editor();
            }
            Err(error) => self.error(&error),
        }
    }

    fn open(self: &Rc<Self>, name: tungsten_linux::NoteName) {
        if !self.persist() {
            return;
        }
        let (client, generation) = match self
            .client()
            .and_then(|client| Ok((client, self.advance()?)))
        {
            Ok(value) => value,
            Err(error) => {
                self.error(&error);
                return;
            }
        };
        let address = client.address().clone();
        self.text.set_editable(false);
        self.task(
            async move { client.read(&name).await },
            move |ui, result| {
                if ui.generation.get() != generation {
                    return;
                }
                match result {
                    Ok(note) => match ui.store.recover(&address) {
                        Ok(recovery) => {
                            if let Some(draft) = recovery
                                .drafts
                                .into_iter()
                                .find(|draft| draft.name == note.name)
                            {
                                *ui.editor.borrow_mut() = Some(Editor::recover(draft, generation));
                            } else {
                                *ui.editor.borrow_mut() =
                                    Some(Editor::loaded(address, note, generation));
                            }
                            ui.show_editor();
                            if !recovery.damaged.is_empty() {
                                ui.error(&Error::Recovery);
                            }
                        }
                        Err(error) => {
                            ui.text.set_editable(ui.editor.borrow().is_some());
                            ui.error(&error);
                        }
                    },
                    Err(error) => {
                        ui.text.set_editable(ui.editor.borrow().is_some());
                        ui.error(&error);
                    }
                }
            },
        );
    }

    fn recover(self: &Rc<Self>, draft: Draft) {
        if !self.persist() {
            return;
        }
        if self.editor.borrow().as_ref().is_some_and(|editor| {
            editor.draft().server == draft.server && editor.draft().name == draft.name
        }) {
            if let Err(error) = self.advance() {
                self.error(&error);
                return;
            }
            self.text.set_editable(true);
            self.outer.set_show_content(true);
            self.inner.set_show_content(true);
            self.text.grab_focus();
            return;
        }
        let result = (|| {
            if self.client()?.address() != &draft.server {
                return Err(Error::Identity);
            }
            let recovery = self.store.recover(&draft.server)?;
            if let Some(latest) = recovery
                .drafts
                .into_iter()
                .find(|latest| latest.name == draft.name)
            {
                *self.editor.borrow_mut() = Some(Editor::recover(latest, self.advance()?));
                self.show_editor();
            } else {
                self.refresh(false);
            }
            if !recovery.damaged.is_empty() {
                return Err(Error::Recovery);
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.error(&error);
        }
    }

    fn save(self: &Rc<Self>) {
        if self.composing.get() {
            return;
        }
        let Ok(client) = self.client() else {
            return;
        };
        let result = self
            .editor
            .borrow_mut()
            .as_mut()
            .map(|editor| editor.begin(&self.store));
        match result {
            Some(Ok(Some((ticket, draft)))) => {
                self.update_status();
                self.task(
                    async move { client.save(&draft).await },
                    move |ui, result| {
                        let completed = ui
                            .editor
                            .borrow_mut()
                            .as_mut()
                            .map(|editor| editor.complete(ticket, result, &ui.store));
                        match completed {
                            Some(Ok(true)) => {
                                ui.banner.set_revealed(false);
                                ui.update_status();
                                ui.refresh(false);
                            }
                            Some(Err(error)) => ui.error(&error),
                            _ => {}
                        }
                    },
                );
            }
            Some(Err(error)) => self.error(&error),
            _ => {}
        }
    }

    fn retry(self: &Rc<Self>) {
        if !self.persist() {
            return;
        }
        let Ok(client) = self.client() else {
            self.settings();
            return;
        };
        let name = self
            .editor
            .borrow()
            .as_ref()
            .map(|editor| editor.draft().name.clone());
        let Some(name) = name else {
            self.refresh(false);
            return;
        };
        let generation = self.generation.get();
        self.task(
            async move { client.read(&name).await },
            move |ui, remote| {
                if ui.generation.get() != generation {
                    return;
                }
                let result = ui
                    .editor
                    .borrow_mut()
                    .as_mut()
                    .map(|editor| editor.reconcile(remote, &ui.store));
                if let Some(Err(error)) = result {
                    ui.error(&error);
                } else {
                    ui.banner.set_revealed(false);
                    ui.save();
                }
            },
        );
    }

    fn reload(self: &Rc<Self>) {
        let Some(name) = self
            .editor
            .borrow()
            .as_ref()
            .map(|editor| editor.draft().name.clone())
        else {
            return;
        };
        let ui = self.clone();
        self.confirm(
            "Reload Note?",
            "The local draft will remain in Drafts. Open the server's saved text?",
            "Reload",
            false,
            move || ui.reload_saved(name),
        );
    }

    fn reload_saved(self: &Rc<Self>, name: tungsten_linux::NoteName) {
        if !self.persist() {
            return;
        }
        let Ok(client) = self.client() else {
            return;
        };
        let Ok(generation) = self.advance() else {
            return;
        };
        let address = client.address().clone();
        self.text.set_editable(false);
        self.task(
            async move { client.read(&name).await },
            move |ui, result| {
                if ui.generation.get() != generation {
                    return;
                }
                match result {
                    Ok(note) => {
                        let preserved = ui
                            .editor
                            .borrow()
                            .as_ref()
                            .filter(|editor| !editor.state().is_saved())
                            .map(|editor| editor.copy(generation, &ui.store));
                        if let Some(Err(error)) = preserved {
                            ui.text.set_editable(true);
                            ui.error(&error);
                            return;
                        }
                        *ui.editor.borrow_mut() = Some(Editor::loaded(address, note, generation));
                        ui.show_editor();
                    }
                    Err(error) => {
                        ui.text.set_editable(true);
                        ui.error(&error);
                    }
                }
            },
        );
    }

    fn copy(self: &Rc<Self>) {
        if !self.persist() {
            return;
        }
        let Ok(scope) = self.advance() else {
            return;
        };
        let result = self
            .editor
            .borrow()
            .as_ref()
            .map(|editor| editor.copy(scope, &self.store));
        match result {
            Some(Ok(editor)) => {
                *self.editor.borrow_mut() = Some(editor);
                self.show_editor();
                self.save();
            }
            Some(Err(error)) => self.error(&error),
            _ => {}
        }
    }

    fn confirm(
        self: &Rc<Self>,
        heading: &str,
        body: &str,
        response: &str,
        destructive: bool,
        perform: impl FnOnce() + 'static,
    ) {
        let dialog = adw::AlertDialog::builder()
            .heading(heading)
            .body(body)
            .build();
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("confirm", response);
        dialog.set_close_response("cancel");
        dialog.set_default_response(Some("cancel"));
        if destructive {
            dialog.set_response_appearance("confirm", adw::ResponseAppearance::Destructive);
        }
        let perform = RefCell::new(Some(perform));
        let weak = Rc::downgrade(self);
        let connection = self.connection.get();
        let generation = self.generation.get();
        dialog.connect_response(None, move |_, response| {
            if response == "confirm"
                && weak.upgrade().is_some_and(|ui| {
                    ui.connection.get() == connection && ui.generation.get() == generation
                })
                && let Some(perform) = perform.borrow_mut().take()
            {
                perform();
            }
        });
        dialog.present(Some(&self.window));
    }

    fn settings(self: &Rc<Self>) {
        let dialog = adw::PreferencesDialog::builder()
            .title("Connection Settings")
            .build();
        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::builder()
            .title("Shared Server")
            .description("Enter the HTTPS origin of your Tungsten server.")
            .build();
        let address = adw::EntryRow::builder().title("Server Address").build();
        if let Ok(client) = self.client() {
            address.set_text(client.address().as_str());
        }
        let connect = gtk::Button::with_label("Connect");
        connect.set_valign(gtk::Align::Center);
        address.add_suffix(&connect);
        group.add(&address);
        page.add(&group);
        dialog.add(&page);
        let ui = self.clone();
        let target = dialog.clone();
        connect.connect_clicked(move |_| {
            match ServerAddress::parse(address.text().as_str(), cfg!(debug_assertions)) {
                Ok(address) => {
                    ui.connect(address);
                    target.close();
                }
                Err(error) => {
                    address.add_css_class("error");
                    ui.error(&error);
                }
            }
        });
        dialog.present(Some(&self.window));
    }

    fn refresh(self: &Rc<Self>, append: bool) {
        let Ok(client) = self.client() else {
            return;
        };
        let listing = self.listing.get().saturating_add(1);
        self.listing.set(listing);
        let collection = self.collection.borrow().clone();
        for (action, enabled) in [
            (
                "rename-workspace",
                matches!(collection, Collection::Workspace(_)),
            ),
            (
                "delete-workspace",
                matches!(collection, Collection::Workspace(_)),
            ),
            (
                "remove-membership",
                matches!(collection, Collection::Workspace(_)),
            ),
            ("empty-trash", matches!(collection, Collection::Trash)),
        ] {
            if let Some(action) = self
                .window
                .lookup_action(action)
                .and_downcast::<gio::SimpleAction>()
            {
                action.set_enabled(enabled);
            }
        }
        let cursor = if append {
            self.cursor.borrow().clone()
        } else {
            None
        };
        let query = self.search.text().to_string();
        self.more.set_sensitive(false);
        self.search
            .set_visible(matches!(collection, Collection::All | Collection::Drafts));
        self.list_title.set_title(match &collection {
            Collection::All => "All Notes",
            Collection::Drafts => "Drafts",
            Collection::Trash => "Trash",
            Collection::Workspace(workspace) => &workspace.title,
        });
        if matches!(collection, Collection::Drafts) {
            match self.store.recover(client.address()) {
                Ok(recovery) => {
                    let query = query.to_lowercase();
                    let entries = recovery
                        .drafts
                        .into_iter()
                        .filter(|draft| draft.body.to_lowercase().contains(&query))
                        .map(Entry::Draft)
                        .collect();
                    self.render_entries(entries, None, false);
                    if !recovery.damaged.is_empty() {
                        self.error(&Error::Recovery);
                    }
                }
                Err(error) => self.error(&error),
            }
            return;
        }
        self.task(
            async move {
                match collection {
                    Collection::All => {
                        let page = client.notes(&query, cursor.as_deref()).await?;
                        Ok((
                            page.notes.into_iter().map(Entry::Note).collect(),
                            page.next,
                            None,
                        ))
                    }
                    Collection::Workspace(workspace) => {
                        let page = client
                            .workspace_notes(&workspace.id, cursor.as_deref())
                            .await?;
                        Ok((
                            page.notes.into_iter().map(Entry::Note).collect(),
                            page.next,
                            Some(page.workspace),
                        ))
                    }
                    Collection::Trash => {
                        let page = client.trash(cursor.as_deref()).await?;
                        Ok((
                            page.entries.into_iter().map(Entry::Trash).collect(),
                            page.next,
                            None,
                        ))
                    }
                    Collection::Drafts => unreachable!(),
                }
            },
            move |ui, result| {
                if ui.listing.get() != listing {
                    return;
                }
                match result {
                    Ok((entries, cursor, workspace)) => {
                        if let Some(workspace) = workspace {
                            ui.remember_workspace(&workspace);
                        }
                        ui.render_entries(entries, cursor, append);
                    }
                    Err(error) => {
                        ui.more.set_sensitive(true);
                        ui.error(&error);
                    }
                }
            },
        );
    }

    fn render_entries(&self, entries: Vec<Entry>, cursor: Option<String>, append: bool) {
        if !append {
            self.list.remove_all();
            self.entries.borrow_mut().clear();
        }
        for entry in &entries {
            let (title, icon) = match entry {
                Entry::Note(note) => (note.title.clone(), "text-x-generic-symbolic"),
                Entry::Draft(draft) => (types::title(&draft.body), "document-edit-symbolic"),
                Entry::Trash(record) => (record.title.clone(), "user-trash-symbolic"),
            };
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(&title).as_str())
                .activatable(true)
                .build();
            row.add_prefix(&gtk::Image::from_icon_name(icon));
            self.list.append(&row);
        }
        self.entries.borrow_mut().extend(entries);
        self.empty.set_visible(self.entries.borrow().is_empty());
        self.more.set_visible(cursor.is_some());
        self.more.set_sensitive(true);
        *self.cursor.borrow_mut() = cursor;
    }

    fn remember_workspace(&self, workspace: &Workspace) {
        if let Some((index, current)) = self
            .workspaces
            .borrow_mut()
            .iter_mut()
            .enumerate()
            .find(|(_, current)| current.id == workspace.id)
        {
            *current = workspace.clone();
            if let Some(item) = self.workspace_section.item(index as u32) {
                item.set_title(Some(&workspace.title));
            }
        }
        if let Collection::Workspace(current) = &mut *self.collection.borrow_mut()
            && current.id == workspace.id
        {
            *current = workspace.clone();
            self.list_title.set_title(&workspace.title);
        }
    }

    fn load_workspaces(self: &Rc<Self>, after: Option<String>) {
        let Ok(client) = self.client() else {
            return;
        };
        let address = client.address().clone();
        if after.is_none() {
            let Some(listing) = self.workspace_listing.get().checked_add(1) else {
                self.error(&Error::Generation);
                return;
            };
            self.workspace_listing.set(listing);
            self.workspace_section.remove_all();
            self.workspaces.borrow_mut().clear();
        }
        let listing = self.workspace_listing.get();
        self.task(
            async move { client.workspaces(after.as_deref()).await },
            move |ui, result| {
                if ui.workspace_listing.get() != listing
                    || ui.client().is_err_and(|_| true)
                    || ui.client().is_ok_and(|client| client.address() != &address)
                {
                    return;
                }
                match result {
                    Ok(page) => {
                        for workspace in &page.workspaces {
                            ui.remember_workspace(workspace);
                            ui.workspace_section.append(
                                adw::SidebarItem::builder()
                                    .title(&workspace.title)
                                    .icon_name("folder-symbolic")
                                    .build(),
                            );
                        }
                        ui.workspaces.borrow_mut().extend(page.workspaces);
                        if let Some(cursor) = page.next {
                            ui.load_workspaces(Some(cursor));
                        }
                    }
                    Err(error) => ui.error(&error),
                }
            },
        );
    }

    fn delete(self: &Rc<Self>) {
        if !self.persist() {
            return;
        }
        let note = self.editor.borrow().as_ref().and_then(|editor| {
            if !editor.state().is_saved() {
                return None;
            }
            Some(Note {
                name: editor.draft().name.clone(),
                body: editor.draft().body.clone(),
                revision: editor.draft().revision.clone()?,
            })
        });
        let Some(note) = note else {
            self.error(&Error::Busy);
            return;
        };
        let ui = self.clone();
        self.confirm(
            "Move Note to Trash?",
            "You can restore the note from Trash.",
            "Move to Trash",
            true,
            move || {
                let Ok(client) = ui.client() else {
                    return;
                };
                let generation = ui.generation.get();
                ui.text.set_editable(false);
                ui.task(
                    async move { client.delete(&note).await },
                    move |ui, result| {
                        if ui.generation.get() != generation {
                            return;
                        }
                        match result {
                            Ok(()) => {
                                *ui.editor.borrow_mut() = None;
                                ui.show_editor();
                                ui.refresh(false);
                            }
                            Err(error) => {
                                ui.text.set_editable(true);
                                ui.error(&error);
                            }
                        }
                    },
                );
            },
        );
    }

    fn new_workspace(self: &Rc<Self>) {
        let ui = self.clone();
        self.text_dialog("New Workspace", "Workspace Name", "Create", move |title| {
            let Ok(client) = ui.client() else {
                return;
            };
            ui.task(
                async move { client.create_workspace(&title).await },
                |ui, result| match result {
                    Ok(workspace) => {
                        ui.remember_workspace(&workspace);
                        ui.load_workspaces(None);
                    }
                    Err(error) => ui.error(&error),
                },
            );
        });
    }

    fn rename_workspace(self: &Rc<Self>) {
        let Collection::Workspace(workspace) = self.collection.borrow().clone() else {
            return;
        };
        let ui = self.clone();
        self.text_dialog(
            "Rename Workspace",
            "Workspace Name",
            "Rename",
            move |title| {
                let Ok(client) = ui.client() else {
                    return;
                };
                let workspace = workspace.clone();
                ui.task(
                    async move { client.rename_workspace(&workspace, &title).await },
                    |ui, result| match result {
                        Ok(workspace) => {
                            ui.remember_workspace(&workspace);
                            ui.load_workspaces(None);
                            ui.refresh(false);
                        }
                        Err(error) => ui.error(&error),
                    },
                );
            },
        );
    }

    fn delete_workspace(self: &Rc<Self>) {
        let Collection::Workspace(workspace) = self.collection.borrow().clone() else {
            return;
        };
        let ui = self.clone();
        self.confirm(
            "Delete Workspace?",
            "Notes will remain in All Notes.",
            "Delete Workspace",
            true,
            move || {
                let Ok(client) = ui.client() else {
                    return;
                };
                let id = workspace.id.clone();
                ui.task(
                    async move { client.delete_workspace(&workspace).await },
                    move |ui, result| match result {
                        Ok(()) => {
                            let selected = matches!(&*ui.collection.borrow(), Collection::Workspace(current) if current.id == id);
                            if selected {
                                *ui.collection.borrow_mut() = Collection::All;
                            }
                            ui.load_workspaces(None);
                            ui.refresh(false);
                        }
                        Err(error) => ui.error(&error),
                    },
                );
            },
        );
    }

    fn remove_membership(self: &Rc<Self>) {
        let Collection::Workspace(workspace) = self.collection.borrow().clone() else {
            return;
        };
        let Some(name) = self
            .editor
            .borrow()
            .as_ref()
            .map(|editor| editor.draft().name.clone())
        else {
            return;
        };
        let Ok(client) = self.client() else {
            return;
        };
        self.task(
            async move { client.membership(&workspace.id, &name, false).await },
            |ui, result| match result {
                Ok(workspace) => {
                    ui.remember_workspace(&workspace);
                    ui.load_workspaces(None);
                    ui.refresh(false);
                }
                Err(error) => ui.error(&error),
            },
        );
    }

    fn empty_trash(self: &Rc<Self>) {
        let ui = self.clone();
        self.confirm(
            "Empty Trash?",
            "Deleted notes will be permanently removed. This action cannot be undone.",
            "Empty Trash",
            true,
            move || {
                let Ok(client) = ui.client() else {
                    return;
                };
                ui.task(
                    async move { client.empty_trash().await },
                    |ui, result| match result {
                        Ok(()) => ui.refresh(false),
                        Err(error) => ui.error(&error),
                    },
                );
            },
        );
    }

    fn text_dialog(
        self: &Rc<Self>,
        title: &str,
        label: &str,
        accept: &str,
        perform: impl Fn(String) + 'static,
    ) {
        let dialog = adw::AlertDialog::builder().heading(title).build();
        let field = gtk::Entry::builder()
            .placeholder_text(label)
            .activates_default(true)
            .build();
        field.update_property(&[gtk::accessible::Property::Label(label)]);
        dialog.set_extra_child(Some(&field));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("accept", accept);
        dialog.set_close_response("cancel");
        dialog.set_default_response(Some("accept"));
        let weak = Rc::downgrade(self);
        let connection = self.connection.get();
        dialog.connect_response(None, move |_, response| {
            if response == "accept"
                && weak
                    .upgrade()
                    .is_some_and(|ui| ui.connection.get() == connection)
            {
                perform(field.text().to_string());
            }
        });
        dialog.present(Some(&self.window));
    }

    fn membership(self: &Rc<Self>) {
        let Some(name) = self
            .editor
            .borrow()
            .as_ref()
            .map(|editor| editor.draft().name.clone())
        else {
            return;
        };
        let dialog = adw::Dialog::builder()
            .title("Add to Workspace")
            .content_width(420)
            .build();
        let list = gtk::ListBox::new();
        for workspace in self.workspaces.borrow().iter().cloned() {
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(&workspace.title).as_str())
                .activatable(true)
                .build();
            row.add_prefix(&gtk::Image::from_icon_name("folder-symbolic"));
            let ui = self.clone();
            let name = name.clone();
            let target = dialog.clone();
            row.connect_activated(move |_| {
                let Ok(client) = ui.client() else {
                    return;
                };
                let id = workspace.id.clone();
                let name = name.clone();
                ui.task(
                    async move { client.membership(&id, &name, true).await },
                    |ui, result| match result {
                        Ok(workspace) => {
                            ui.remember_workspace(&workspace);
                            ui.load_workspaces(None);
                            ui.refresh(false);
                        }
                        Err(error) => ui.error(&error),
                    },
                );
                target.close();
            });
            list.append(&row);
        }
        let (view, _, _) = toolbar("Add to Workspace", &scrolled(&list));
        dialog.set_child(Some(&view));
        dialog.present(Some(&self.window));
    }

    fn link(self: &Rc<Self>) {
        if self.editor.borrow().is_none() || !self.persist() {
            return;
        }
        let dialog = adw::Dialog::builder()
            .title("Insert Link")
            .content_width(480)
            .content_height(500)
            .build();
        let search = gtk::SearchEntry::builder()
            .placeholder_text("Search Notes")
            .build();
        search.update_property(&[gtk::accessible::Property::Label("Search Notes")]);
        let list = gtk::ListBox::new();
        let more = gtk::Button::with_label("Load More");
        more.set_visible(false);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&search);
        content.append(&scrolled(&list));
        content.append(&more);
        let (view, _, _) = toolbar("Insert Link", &content);
        dialog.set_child(Some(&view));
        let cursor = Rc::new(RefCell::new(None::<String>));
        let generation = self.generation.get();
        let request: Rc<dyn Fn(bool)> = {
            let weak = Rc::downgrade(self);
            let dialog = dialog.downgrade();
            let search = search.clone();
            let list = list.clone();
            let more = more.clone();
            let cursor = cursor.clone();
            Rc::new(move |append| {
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                let Ok(client) = ui.client() else {
                    return;
                };
                let Some(dialog) = dialog.upgrade() else {
                    return;
                };
                let query = search.text().to_string();
                let after = if append {
                    cursor.borrow().clone()
                } else {
                    None
                };
                let list = list.clone();
                let more = more.clone();
                let cursor = cursor.clone();
                let search = search.clone();
                more.set_sensitive(false);
                let expected_query = query.clone();
                let dialog = dialog.downgrade();
                ui.task(
                    async move { client.notes(&query, after.as_deref()).await },
                    move |ui, result| {
                        if ui.generation.get() != generation
                            || search.text().as_str() != expected_query
                        {
                            return;
                        }
                        let Some(dialog) = dialog.upgrade() else {
                            return;
                        };
                        more.set_sensitive(true);
                        match result {
                            Ok(page) => {
                                if !append {
                                    list.remove_all();
                                }
                                for note in page.notes {
                                    let row = adw::ActionRow::builder()
                                        .title(glib::markup_escape_text(&note.title).as_str())
                                        .activatable(true)
                                        .build();
                                    row.add_prefix(&gtk::Image::from_icon_name(
                                        "text-x-generic-symbolic",
                                    ));
                                    let weak = Rc::downgrade(ui);
                                    let target = dialog.downgrade();
                                    row.connect_activated(move |_| {
                                        if let Some(ui) = weak.upgrade()
                                            && ui.generation.get() == generation
                                        {
                                            ui.insert_link(&note);
                                        }
                                        if let Some(dialog) = target.upgrade() {
                                            dialog.close();
                                        }
                                    });
                                    list.append(&row);
                                }
                                more.set_visible(page.next.is_some());
                                *cursor.borrow_mut() = page.next;
                            }
                            Err(error) => ui.error(&error),
                        }
                    },
                );
            })
        };
        let refresh = request.clone();
        search.connect_search_changed(move |_| refresh(false));
        let refresh = request.clone();
        more.connect_clicked(move |_| refresh(true));
        dialog.present(Some(&self.window));
        request(false);
        search.grab_focus();
    }

    fn insert_link(&self, note: &Summary) {
        let Some(source) = self
            .editor
            .borrow()
            .as_ref()
            .map(|editor| editor.draft().name.clone())
        else {
            return;
        };
        let path = tungsten::markdown::relative_note_path(&source, &note.name);
        let title: String = note
            .title
            .chars()
            .map(|character| match character {
                '[' | ']' | '\\' => format!("\\{character}"),
                character => character.to_string(),
            })
            .collect();
        self.text
            .buffer()
            .insert_at_cursor(&format!("[{title}]({})", path));
    }

    fn attach(self: &Rc<Self>) {
        if self.editor.borrow().is_none() {
            return;
        }
        let dialog = gtk::FileDialog::builder().title("Attach File").build();
        let ui = self.clone();
        let generation = self.generation.get();
        dialog.open(Some(&self.window), gio::Cancellable::NONE, move |result| {
            let file = match result {
                Ok(file) => file,
                Err(_) => return,
            };
            let Some(path) = file.path() else {
                ui.error(&Error::Identity);
                return;
            };
            let Some(name) = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
            else {
                return;
            };
            let Ok(client) = ui.client() else {
                return;
            };
            ui.task(
                async move { client.upload(&path, &name).await },
                move |ui, result| match result {
                    Ok(attachment) if ui.generation.get() == generation => {
                        ui.text.buffer().insert_at_cursor(&format!(
                            "[{}](.attachments/{})",
                            attachment.name.replace(['[', ']', '\n'], ""),
                            attachment.id
                        ));
                    }
                    Ok(_) => {}
                    Err(error) => ui.error(&error),
                },
            );
        });
    }

    fn paged_rows<T: Clone + 'static>(
        self: &Rc<Self>,
        group: &adw::PreferencesGroup,
        values: Vec<T>,
        row: impl Fn(&Rc<Self>, T) -> adw::ActionRow + 'static,
    ) {
        let more = adw::ButtonRow::builder()
            .title("Load More")
            .start_icon_name("go-down-symbolic")
            .build();
        let append: Rc<dyn Fn()> = {
            let weak = Rc::downgrade(self);
            let group = group.downgrade();
            let more = more.downgrade();
            let offset = Cell::new(0usize);
            Rc::new(move || {
                let (Some(ui), Some(group), Some(more)) =
                    (weak.upgrade(), group.upgrade(), more.upgrade())
                else {
                    return;
                };
                if more.parent().is_some() {
                    group.remove(&more);
                }
                let end = offset.get().saturating_add(50).min(values.len());
                for value in values.iter().take(end).skip(offset.get()) {
                    group.add(&row(&ui, value.clone()));
                }
                offset.set(end);
                if end < values.len() {
                    group.add(&more);
                }
            })
        };
        let load = append.clone();
        more.connect_activated(move |_| load());
        append();
    }

    fn details(self: &Rc<Self>) {
        let Some(name) = self
            .editor
            .borrow()
            .as_ref()
            .map(|editor| editor.draft().name.clone())
        else {
            return;
        };
        let Ok(client) = self.client() else {
            return;
        };
        let generation = self.generation.get();
        self.task(
            async move {
                let links = client.connections(&name).await?;
                let backlinks = client.backlinks(&name).await?;
                let history = client.history(&name).await?;
                Ok((links, backlinks, history))
            },
            move |ui, result| {
                if ui.generation.get() != generation {
                    return;
                }
                let (links, backlinks, history) = match result {
                    Ok(value) => value,
                    Err(error) => {
                        ui.error(&error);
                        return;
                    }
                };
                let dialog = adw::PreferencesDialog::builder()
                    .title("Note Details")
                    .build();
                let page = adw::PreferencesPage::new();
                let group = adw::PreferencesGroup::builder()
                    .title("Linked Notes")
                    .build();
                let target = dialog.downgrade();
                ui.paged_rows(&group, links.links, move |ui, name| {
                    let row = adw::ActionRow::builder()
                        .title("Loading Note")
                        .activatable(true)
                        .build();
                    row.add_prefix(&gtk::Image::from_icon_name("go-jump-symbolic"));
                    let weak = Rc::downgrade(ui);
                    let dialog = target.clone();
                    let selected = name.clone();
                    row.connect_activated(move |_| {
                        if let Some(dialog) = dialog.upgrade() {
                            dialog.close();
                        }
                        if let Some(ui) = weak.upgrade() {
                            ui.open(selected.clone());
                        }
                    });
                    if let Ok(client) = ui.client() {
                        let row = row.downgrade();
                        ui.task(async move { client.read(&name).await }, move |_, result| {
                            if let Some(row) = row.upgrade() {
                                let title = result
                                    .map(|note| types::title(&note.body))
                                    .unwrap_or_else(|_| "Unavailable Note".into());
                                row.set_title(glib::markup_escape_text(&title).as_str());
                            }
                        });
                    }
                    row
                });
                page.add(&group);
                let group = adw::PreferencesGroup::builder()
                    .title("Incoming Links")
                    .build();
                let target = dialog.downgrade();
                ui.paged_rows(&group, backlinks, move |ui, note| {
                    let row = adw::ActionRow::builder()
                        .title(glib::markup_escape_text(&note.title).as_str())
                        .activatable(true)
                        .build();
                    row.add_prefix(&gtk::Image::from_icon_name("go-jump-symbolic"));
                    let weak = Rc::downgrade(ui);
                    let dialog = target.clone();
                    row.connect_activated(move |_| {
                        if let Some(dialog) = dialog.upgrade() {
                            dialog.close();
                        }
                        if let Some(ui) = weak.upgrade() {
                            ui.open(note.name.clone());
                        }
                    });
                    row
                });
                page.add(&group);
                let group = adw::PreferencesGroup::builder()
                    .title("Attachments")
                    .build();
                ui.paged_rows(&group, links.attachments, |ui, id| {
                    let row = adw::ActionRow::builder()
                        .title("Loading File")
                        .activatable(true)
                        .build();
                    row.add_prefix(&gtk::Image::from_icon_name("mail-attachment-symbolic"));
                    let weak = Rc::downgrade(ui);
                    let selected = id.clone();
                    row.connect_activated(move |_| {
                        if let Some(ui) = weak.upgrade() {
                            ui.open_attachment(selected.clone());
                        }
                    });
                    if let Ok(client) = ui.client() {
                        let row = row.downgrade();
                        ui.task(
                            async move { client.attachment(&id).await },
                            move |_, result| {
                                if let Some(row) = row.upgrade() {
                                    match result {
                                        Ok(file) => row.set_title(
                                            glib::markup_escape_text(&file.name).as_str(),
                                        ),
                                        Err(_) => {
                                            row.set_title("Unavailable File");
                                            row.set_activatable(false);
                                        }
                                    }
                                }
                            },
                        );
                    }
                    row
                });
                page.add(&group);
                let group = adw::PreferencesGroup::builder().title("History").build();
                for record in history {
                    let captured = glib::DateTime::from_unix_local((record.captured / 1000) as i64)
                        .and_then(|date| date.format("%c"))
                        .map(|value| value.to_string())
                        .unwrap_or_else(|_| "Saved Version".into());
                    let row = adw::ActionRow::builder()
                        .title(glib::markup_escape_text(&captured).as_str())
                        .activatable(true)
                        .build();
                    row.add_prefix(&gtk::Image::from_icon_name("document-open-recent-symbolic"));
                    let ui = ui.clone();
                    let dialog = dialog.clone();
                    row.connect_activated(move |_| {
                        dialog.close();
                        ui.preview_record(record.clone(), false);
                    });
                    group.add(&row);
                }
                page.add(&group);
                dialog.add(&page);
                dialog.present(Some(&ui.window));
            },
        );
    }

    fn preview_record(self: &Rc<Self>, record: SavedNote, trash: bool) {
        let Ok(client) = self.client() else {
            return;
        };
        let generation = self.generation.get();
        let history_read = self.editor.borrow().as_ref().map(Editor::history_read);
        let selected = record.clone();
        self.task(
            async move { client.saved(&selected, trash).await },
            move |ui, result| {
                if ui.generation.get() != generation {
                    return;
                }
                let note = match result {
                    Ok(note) => note,
                    Err(error) => {
                        ui.error(&error);
                        return;
                    }
                };
                let dialog = adw::Dialog::builder()
                    .title(if trash {
                        "Deleted Note"
                    } else {
                        "Saved Version"
                    })
                    .content_width(650)
                    .content_height(500)
                    .build();
                let preview = gtk::TextView::builder()
                    .editable(false)
                    .wrap_mode(gtk::WrapMode::WordChar)
                    .top_margin(18)
                    .bottom_margin(18)
                    .left_margin(18)
                    .right_margin(18)
                    .build();
                preview.buffer().set_text(&note.body);
                preview
                    .update_property(&[gtk::accessible::Property::Label("Saved Markdown Source")]);
                let (view, header, _) = toolbar(
                    if trash {
                        "Deleted Note"
                    } else {
                        "Saved Version"
                    },
                    &scrolled(&preview),
                );
                let restore = gtk::Button::with_label("Restore");
                header.pack_end(&restore);
                let ui = ui.clone();
                let target = dialog.clone();
                let parent = ui.window.clone();
                restore.connect_clicked(move |_| {
                    if ui.generation.get() != generation {
                        target.close();
                        return;
                    }
                    if trash {
                        let Ok(client) = ui.client() else {
                            return;
                        };
                        let record = record.clone();
                        let name = record.name.clone();
                        ui.task(
                            async move {
                                match client.restore_trash(&record, &name).await {
                                    Err(Error::Exists) => {
                                        client
                                            .restore_trash(&record, &types::generated_name())
                                            .await
                                    }
                                    result => result,
                                }
                            },
                            move |ui, result| match result {
                                Ok(note) => {
                                    if ui.generation.get() == generation {
                                        ui.open(note.name);
                                    }
                                    ui.refresh(false);
                                }
                                Err(error) => ui.error(&error),
                            },
                        );
                    } else {
                        let result = history_read
                            .as_ref()
                            .map(|read| ui.restore_history(read, note.body.clone()));
                        match result {
                            Some(Ok(())) => {
                                ui.save();
                            }
                            Some(Err(error)) => ui.error(&error),
                            _ => {}
                        }
                    }
                    target.close();
                });
                dialog.set_child(Some(&view));
                dialog.present(Some(&parent));
            },
        );
    }

    fn restore_history(
        &self,
        read: &tungsten_linux::editor::HistoryRead,
        body: String,
    ) -> Result<()> {
        if self.composing.get() {
            return Err(Error::Busy);
        }
        self.editor
            .borrow_mut()
            .as_mut()
            .ok_or(Error::Busy)?
            .replace_from_history(read, body.clone(), &self.store)?;
        self.changing.set(true);
        let buffer = self.text.buffer();
        buffer.begin_user_action();
        buffer.delete(&mut buffer.start_iter(), &mut buffer.end_iter());
        buffer.insert_at_cursor(&body);
        buffer.end_user_action();
        self.changing.set(false);
        let result = self
            .editor
            .borrow_mut()
            .as_mut()
            .ok_or(Error::Busy)?
            .persist(&self.store);
        self.update_status();
        result
    }

    fn open_attachment(self: &Rc<Self>, id: String) {
        let Ok(client) = self.client() else {
            return;
        };
        let directory = glib::user_cache_dir().join(APP_ID).join("attachments");
        self.task(
            async move {
                let attachment = client.attachment(&id).await?;
                client.download(&attachment, &directory).await
            },
            |ui, result| match result {
                Ok(path) => {
                    let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(&path)));
                    let weak = Rc::downgrade(ui);
                    launcher.launch(Some(&ui.window), gio::Cancellable::NONE, move |result| {
                        if result.is_err()
                            && let Some(ui) = weak.upgrade()
                        {
                            ui.status.set_label(
                                "Unable to open the attachment in the default application.",
                            );
                        }
                    });
                    ui.downloads.borrow_mut().push(path);
                }
                Err(error) => ui.error(&error),
            },
        );
    }
}

#[cfg(test)]
#[path = "../native/workflows.rs"]
mod tests;
