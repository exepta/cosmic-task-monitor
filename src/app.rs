// SPDX-License-Identifier: MPL-2.0

use crate::config::Config;
use crate::fl;
use cosmic::app::context_drawer;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::desktop::{self, IconSourceExt};
use cosmic::iced::alignment::Horizontal;
use cosmic::iced::{Alignment, Border, Color, Length, Subscription};
use cosmic::theme;
use cosmic::widget::{self, about::About, icon, menu, nav_bar};
use cosmic::{iced_futures, prelude::*};
use futures_util::SinkExt;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const APP_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/apps/icon.svg");
const PROCESS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const DEBUG_LOG_PATH: &str = "/tmp/cosmic-task-monitor-debug.log";

fn table_cell_style(theme: &cosmic::Theme) -> widget::container::Style {
    widget::container::Style {
        border: Border {
            color: theme.cosmic().bg_divider().into(),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

#[derive(Debug, Clone)]
struct ProcessEntry {
    name: String,
    display_name: String,
    icon_handle: Option<icon::Handle>,
    pid: u32,
    cpu_percent: f32,
    rss_bytes: u64,
    threads: u32,
}

#[derive(Clone)]
struct DesktopAppMeta {
    app_id: String,
    name: String,
    icon_handle: Option<icon::Handle>,
    primary_exec_keys: HashSet<String>,
}

#[derive(Clone)]
struct SteamAppMeta {
    name: String,
    icon_handle: Option<icon::Handle>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SortColumn {
    Name,
    Cpu,
    Pid,
    Ram,
    Threads,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct SortState {
    column: SortColumn,
    direction: SortDirection,
}

pub struct AppModel {
    core: cosmic::Core,
    context_page: ContextPage,
    about: About,
    nav: nav_bar::Model,
    key_binds: HashMap<menu::KeyBind, MenuAction>,
    config: Config,
    system: System,
    desktop_apps_by_exec: HashMap<String, DesktopAppMeta>,
    steam_apps_by_id: HashMap<String, SteamAppMeta>,
    process_entries: Vec<ProcessEntry>,
    sort_state: SortState,
    refresh_cycle: u64,
}

#[derive(Debug, Clone)]
pub enum Message {
    LaunchUrl(String),
    ToggleContextPage(ContextPage),
    UpdateConfig(Config),
    RefreshProcesses,
    ToggleSort(SortColumn),
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "dev.mmurphy.Test";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        let mut nav = nav_bar::Model::default();

        nav.insert()
            .text(fl!("page-id", num = 1))
            .data::<Page>(Page::Page1)
            .icon(icon::from_name("applications-science-symbolic"))
            .activate();

        nav.insert()
            .text(fl!("page-id", num = 2))
            .data::<Page>(Page::Page2)
            .icon(icon::from_name("applications-system-symbolic"));

        nav.insert()
            .text(fl!("page-id", num = 3))
            .data::<Page>(Page::Page3)
            .icon(icon::from_name("applications-games-symbolic"));

        let about = About::default()
            .name(fl!("app-title"))
            .icon(widget::icon::from_svg_bytes(APP_ICON))
            .version(env!("CARGO_PKG_VERSION"))
            .links([(fl!("repository"), REPOSITORY)])
            .license(env!("CARGO_PKG_LICENSE"));

        let mut app = AppModel {
            core,
            context_page: ContextPage::default(),
            about,
            nav,
            key_binds: HashMap::new(),
            config: cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                .map(|context| match Config::get_entry(&context) {
                    Ok(config) => config,
                    Err((_errors, config)) => config,
                })
                .unwrap_or_default(),
            system: System::new_all(),
            desktop_apps_by_exec: Self::load_desktop_app_map(),
            steam_apps_by_id: HashMap::new(),
            process_entries: Vec::new(),
            sort_state: SortState {
                column: SortColumn::Ram,
                direction: SortDirection::Desc,
            },
            refresh_cycle: 0,
        };
        app.refresh_processes();

        let command = app.update_title();
        (app, command)
    }

    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        let menu_bar = menu::bar(vec![menu::Tree::with_children(
            menu::root(fl!("view")).apply(Element::from),
            menu::items(
                &self.key_binds,
                vec![menu::Item::Button(fl!("about"), None, MenuAction::About)],
            ),
        )]);

        vec![menu_bar.into()]
    }

    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav)
    }

    fn context_drawer(&self) -> Option<context_drawer::ContextDrawer<'_, Self::Message>> {
        if !self.core.window.show_context {
            return None;
        }

        Some(match self.context_page {
            ContextPage::About => context_drawer::about(
                &self.about,
                |url| Message::LaunchUrl(url.to_string()),
                Message::ToggleContextPage(ContextPage::About),
            ),
        })
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let content: Element<_> = match self.nav.active_data::<Page>().unwrap() {
            Page::Page1 => {
                let header = widget::row::with_capacity(2)
                    .push(widget::text::title1("App"))
                    .push(widget::text::title3(format!(
                        "{} Eintraege",
                        self.process_entries.len()
                    )))
                    .align_y(Alignment::End)
                    .spacing(space_s);

                let table_header = widget::row::with_capacity(5)
                    .push(
                        widget::container(
                            widget::button::custom(
                                self.header_button_content("Name", SortColumn::Name),
                            )
                            .on_press(Message::ToggleSort(SortColumn::Name))
                            .width(Length::Fill),
                        )
                        .padding(10)
                        .class(theme::Container::custom(table_cell_style))
                        .width(Length::FillPortion(6)),
                    )
                    .push(
                        widget::container(
                            widget::button::custom(
                                self.header_button_content("CPU(%)", SortColumn::Cpu),
                            )
                            .on_press(Message::ToggleSort(SortColumn::Cpu))
                            .width(Length::Fill),
                        )
                        .padding(10)
                        .class(theme::Container::custom(table_cell_style))
                        .width(Length::FillPortion(2)),
                    )
                    .push(
                        widget::container(
                            widget::button::custom(
                                self.header_button_content("PID", SortColumn::Pid),
                            )
                            .on_press(Message::ToggleSort(SortColumn::Pid))
                            .width(Length::Fill),
                        )
                        .padding(10)
                        .class(theme::Container::custom(table_cell_style))
                        .width(Length::FillPortion(2)),
                    )
                    .push(
                        widget::container(
                            widget::button::custom(
                                self.header_button_content("RAM", SortColumn::Ram),
                            )
                            .on_press(Message::ToggleSort(SortColumn::Ram))
                            .width(Length::Fill),
                        )
                        .padding(10)
                        .class(theme::Container::custom(table_cell_style))
                        .width(Length::FillPortion(2)),
                    )
                    .push(
                        widget::container(
                            widget::button::custom(
                                self.header_button_content("Threads", SortColumn::Threads),
                            )
                            .on_press(Message::ToggleSort(SortColumn::Threads))
                            .width(Length::Fill),
                        )
                        .padding(10)
                        .class(theme::Container::custom(table_cell_style))
                        .width(Length::FillPortion(2)),
                    )
                    .spacing(0);

                let rows = self.process_entries.iter().fold(
                    widget::column::with_capacity(self.process_entries.len()),
                    |column, process| {
                        let name_cell_content: Element<'_, Message> =
                            if let Some(icon_handle) = process.icon_handle.as_ref() {
                                widget::row::with_capacity(2)
                                    .push(widget::icon::icon(icon_handle.clone()).size(18))
                                    .push(widget::text(process.display_name.as_str()))
                                    .align_y(Alignment::Center)
                                    .spacing(space_s)
                                    .into()
                            } else {
                                widget::text(process.display_name.as_str()).into()
                            };

                        column.push(
                            widget::row::with_capacity(5)
                                .push(
                                    widget::container(name_cell_content)
                                        .padding(10)
                                        .class(theme::Container::custom(table_cell_style))
                                        .width(Length::FillPortion(6)),
                                )
                                .push(
                                    widget::container(widget::text(format!(
                                        "{:.1}%",
                                        process.cpu_percent
                                    )))
                                    .padding(10)
                                    .class(theme::Container::custom(table_cell_style))
                                    .width(Length::FillPortion(2)),
                                )
                                .push(
                                    widget::container(widget::text(process.pid.to_string()))
                                        .padding(10)
                                        .class(theme::Container::custom(table_cell_style))
                                        .width(Length::FillPortion(2)),
                                )
                                .push(
                                    widget::container(widget::text(Self::format_rss(
                                        process.rss_bytes,
                                    )))
                                    .padding(10)
                                    .class(theme::Container::custom(table_cell_style))
                                    .width(Length::FillPortion(2)),
                                )
                                .push(
                                    widget::container(widget::text(process.threads.to_string()))
                                        .padding(10)
                                        .class(theme::Container::custom(table_cell_style))
                                        .width(Length::FillPortion(2)),
                                )
                                .spacing(0),
                        )
                    },
                );

                widget::column::with_capacity(3)
                    .push(header)
                    .push(table_header)
                    .push(widget::scrollable(rows).height(Length::Fill))
                    .spacing(space_s)
                    .height(Length::Fill)
                    .into()
            }

            Page::Page2 => {
                let header = widget::row::with_capacity(2)
                    .push(widget::text::title1(fl!("welcome")))
                    .push(widget::text::title3(fl!("page-id", num = 2)))
                    .align_y(Alignment::End)
                    .spacing(space_s);

                widget::column::with_capacity(1)
                    .push(header)
                    .spacing(space_s)
                    .height(Length::Fill)
                    .into()
            }

            Page::Page3 => {
                let header = widget::row::with_capacity(2)
                    .push(widget::text::title1(fl!("welcome")))
                    .push(widget::text::title3(fl!("page-id", num = 3)))
                    .align_y(Alignment::End)
                    .spacing(space_s);

                widget::column::with_capacity(1)
                    .push(header)
                    .spacing(space_s)
                    .height(Length::Fill)
                    .into()
            }
        };

        widget::container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let mut subscriptions = vec![
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| Message::UpdateConfig(update.config)),
        ];

        subscriptions.push(Subscription::run(|| {
            iced_futures::stream::channel(1, |mut emitter| async move {
                let mut interval = tokio::time::interval(PROCESS_REFRESH_INTERVAL);
                loop {
                    interval.tick().await;
                    _ = emitter.send(Message::RefreshProcesses).await;
                }
            })
        }));

        Subscription::batch(subscriptions)
    }

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::RefreshProcesses => self.refresh_processes(),
            Message::ToggleSort(column) => self.toggle_sort(column),
            Message::ToggleContextPage(context_page) => {
                if self.context_page == context_page {
                    self.core.window.show_context = !self.core.window.show_context;
                } else {
                    self.context_page = context_page;
                    self.core.window.show_context = true;
                }
            }
            Message::UpdateConfig(config) => self.config = config,
            Message::LaunchUrl(url) => {
                if let Err(err) = open::that_detached(&url) {
                    eprintln!("failed to open {url:?}: {err}");
                }
            }
        }
        Task::none()
    }

    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<cosmic::Action<Self::Message>> {
        self.nav.activate(id);
        self.update_title()
    }
}

impl AppModel {
    pub fn update_title(&mut self) -> Task<cosmic::Action<Message>> {
        let mut window_title = fl!("app-title");

        if let Some(page) = self.nav.text(self.nav.active()) {
            window_title.push_str(" — ");
            window_title.push_str(page);
        }

        if let Some(id) = self.core.main_window_id() {
            self.set_window_title(window_title, id)
        } else {
            Task::none()
        }
    }

    fn refresh_processes(&mut self) {
        self.refresh_cycle = self.refresh_cycle.saturating_add(1);
        let debug_enabled = Self::debug_enabled() && self.refresh_cycle % 5 == 0;

        self.desktop_apps_by_exec = Self::load_desktop_app_map();
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_memory()
                .with_cpu()
                .with_disk_usage()
                .with_user(UpdateKind::OnlyIfNotSet)
                .with_exe(UpdateKind::OnlyIfNotSet)
                // New processes need cmdline to match Flatpak/wrapper launches correctly.
                .with_cmd(UpdateKind::OnlyIfNotSet),
        );
        let cpu_core_count = self.system.cpus().len().max(1) as f32;

        let processes = self.system.processes();
        let current_user_id = self
            .system
            .process(Pid::from_u32(std::process::id()))
            .and_then(|process| process.user_id().cloned());

        let eligible_pids: HashSet<Pid> = processes
            .iter()
            .filter_map(|(pid, process)| {
                if Self::is_program_process(process, current_user_id.as_ref()) {
                    Some(*pid)
                } else {
                    None
                }
            })
            .collect();

        #[derive(Default)]
        struct Aggregate {
            name: String,
            icon_handle: Option<icon::Handle>,
            pid: u32,
            cpu_percent: f32,
            rss_bytes: u64,
            threads: u32,
        }

        let mut groups: HashMap<String, Aggregate> = HashMap::new();
        let mut matched_processes = 0usize;
        let mut unmatched_processes = 0usize;
        let mut unmatched_samples = Vec::new();
        let mut steam_apps_by_id = std::mem::take(&mut self.steam_apps_by_id);
        let steam_icon_handle = self
            .desktop_apps_by_exec
            .get("steam")
            .and_then(|meta| meta.icon_handle.clone());
        for pid in &eligible_pids {
            let Some(process) = processes.get(pid) else {
                continue;
            };
            let candidate_keys = Self::process_candidate_keys(process);
            if candidate_keys.is_empty() {
                continue;
            }

            let matched_app = Self::desktop_app_for_process(process, &self.desktop_apps_by_exec)
                .map(|app_meta| {
                    (
                        app_meta.app_id.clone(),
                        app_meta.name.clone(),
                        app_meta.icon_handle.clone(),
                    )
                })
                .or_else(|| {
                    Self::steam_app_id_for_process(process, processes).map(|steam_app_id| {
                        let steam_meta = steam_apps_by_id
                            .entry(steam_app_id.clone())
                            .or_insert_with(|| {
                                Self::load_steam_app_meta(&steam_app_id, steam_icon_handle.clone())
                            });

                        (
                            format!("steam-app-{steam_app_id}"),
                            steam_meta.name.clone(),
                            steam_meta.icon_handle.clone(),
                        )
                    })
                });

            let Some((app_id, app_name, app_icon_handle)) = matched_app else {
                unmatched_processes = unmatched_processes.saturating_add(1);
                if debug_enabled && unmatched_samples.len() < 10 {
                    unmatched_samples.push(format!(
                        "pid={} name={} keys={}",
                        process.pid().as_u32(),
                        process.name().to_string_lossy(),
                        candidate_keys.join(",")
                    ));
                }
                if debug_enabled
                    && candidate_keys.iter().any(|key| {
                        key.contains("horizon")
                            || key.contains("vmware")
                            || key.contains("omnissa")
                            || key.contains("cosmic-files")
                    })
                {
                    Self::debug_log(&format!(
                        "[cosmic-task-monitor] unmatched-focus pid={} name={} keys={}",
                        process.pid().as_u32(),
                        process.name().to_string_lossy(),
                        candidate_keys.join(",")
                    ));
                }
                continue;
            };
            if Self::is_excluded_app_id(&app_id) {
                continue;
            }
            matched_processes = matched_processes.saturating_add(1);
            if debug_enabled
                && candidate_keys.iter().any(|key| {
                    key.contains("horizon")
                        || key.contains("vmware")
                        || key.contains("omnissa")
                        || key.contains("cosmic-files")
                })
            {
                Self::debug_log(&format!(
                    "[cosmic-task-monitor] matched-focus pid={} app_id={} app_name={} keys={}",
                    process.pid().as_u32(),
                    app_id,
                    app_name,
                    candidate_keys.join(",")
                ));
            }
            let entry = groups.entry(app_id).or_insert_with(|| Aggregate {
                name: app_name,
                icon_handle: app_icon_handle,
                pid: process.pid().as_u32(),
                rss_bytes: process.memory(),
                ..Aggregate::default()
            });

            entry.cpu_percent += (process.cpu_usage() / cpu_core_count).clamp(0.0, 100.0);
            entry.pid = entry.pid.min(process.pid().as_u32());
            entry.rss_bytes = entry.rss_bytes.max(process.memory());
            entry.threads += process.tasks().map_or(1, |tasks| tasks.len() as u32);
        }

        self.process_entries = groups
            .into_iter()
            .map(|(_, entry)| ProcessEntry {
                display_name: entry.name.clone(),
                name: entry.name,
                pid: entry.pid,
                icon_handle: entry.icon_handle,
                cpu_percent: entry.cpu_percent.clamp(0.0, 100.0),
                rss_bytes: entry.rss_bytes,
                threads: entry.threads.max(1),
            })
            .collect();

        self.steam_apps_by_id = steam_apps_by_id;
        self.sort_process_entries();

        if debug_enabled {
            let shown_apps = self
                .process_entries
                .iter()
                .map(|entry| format!("{}(pid={})", entry.display_name, entry.pid))
                .collect::<Vec<_>>()
                .join(", ");
            Self::debug_log(&format!(
                "[cosmic-task-monitor] refresh={} eligible={} matched={} unmatched={} shown={}",
                self.refresh_cycle,
                eligible_pids.len(),
                matched_processes,
                unmatched_processes,
                self.process_entries.len()
            ));
            Self::debug_log(&format!("[cosmic-task-monitor] shown_apps={shown_apps}"));
            for sample in unmatched_samples {
                Self::debug_log(&format!("[cosmic-task-monitor] unmatched {sample}"));
            }
        }
    }

    fn load_desktop_app_map() -> HashMap<String, DesktopAppMeta> {
        let locales = Self::desktop_locales();
        let xdg_current_desktop = env::var("XDG_CURRENT_DESKTOP")
            .ok()
            .and_then(|desktop| desktop.split(':').next().map(ToString::to_string));

        let mut candidates_by_key: HashMap<String, Vec<DesktopAppMeta>> = HashMap::new();
        for app in desktop::load_applications(&locales, false, xdg_current_desktop.as_deref()) {
            let mut candidates = HashSet::new();
            let mut primary_exec_keys = HashSet::new();
            let Some(app_id) = Self::normalize_exec_key(&app.id) else {
                continue;
            };

            if let Some(exec) = app.exec.as_deref() {
                for key in Self::exec_candidate_keys(exec) {
                    candidates.insert(key);
                }
                for key in Self::exec_primary_keys(exec) {
                    primary_exec_keys.insert(key);
                }
                for key in Self::exec_candidate_keys(exec) {
                    primary_exec_keys.insert(key);
                }
            }
            if let Some(id_key) = Self::normalize_exec_key(&app.id) {
                candidates.insert(id_key);
            }
            if let Some(wm_class) = app.wm_class.as_deref() {
                for key in Self::exec_candidate_keys(wm_class) {
                    candidates.insert(key.clone());
                    primary_exec_keys.insert(key);
                }
            }
            for mime in &app.mime_types {
                let mime = mime.essence_str();
                if let Some(suffix) = mime.rsplit('/').next() {
                    for key in Self::exec_candidate_keys(suffix) {
                        candidates.insert(key.clone());
                        primary_exec_keys.insert(key);
                    }
                }
            }

            if candidates.is_empty() {
                continue;
            }
            if primary_exec_keys.is_empty() {
                if let Some(id_key) = Self::normalize_exec_key(&app.id) {
                    primary_exec_keys.insert(id_key);
                }
            }

            let meta = DesktopAppMeta {
                app_id,
                name: app.name.clone(),
                icon_handle: Some(app.icon.as_cosmic_icon()),
                primary_exec_keys,
            };

            for key in candidates {
                candidates_by_key.entry(key).or_default().push(meta.clone());
            }
        }

        let mut apps = HashMap::new();
        for (key, candidate_list) in candidates_by_key {
            let mut unique_by_app_id = HashMap::new();
            for meta in candidate_list {
                unique_by_app_id.entry(meta.app_id.clone()).or_insert(meta);
            }

            let mut candidates = unique_by_app_id.into_values().collect::<Vec<_>>();
            if candidates.is_empty() {
                continue;
            }
            candidates.sort_by(|a, b| {
                let rank = |meta: &DesktopAppMeta| -> u8 {
                    if meta.app_id == key {
                        0
                    } else if meta.primary_exec_keys.contains(&key) {
                        1
                    } else if meta.app_id.starts_with(&key) || key.starts_with(&meta.app_id) {
                        2
                    } else {
                        3
                    }
                };

                rank(a)
                    .cmp(&rank(b))
                    .then_with(|| {
                        a.app_id
                            .len()
                            .abs_diff(key.len())
                            .cmp(&b.app_id.len().abs_diff(key.len()))
                    })
                    .then_with(|| a.app_id.cmp(&b.app_id))
            });
            apps.insert(key, candidates.remove(0));
        }

        apps
    }

    fn desktop_locales() -> Vec<String> {
        let mut locales = Vec::new();

        if let Ok(language) = env::var("LANGUAGE") {
            for locale in language.split(':') {
                let cleaned = locale.split('.').next().unwrap_or(locale).trim();
                if !cleaned.is_empty() && !locales.iter().any(|value| value == cleaned) {
                    locales.push(cleaned.to_string());
                }
            }
        }

        if let Ok(lang) = env::var("LANG") {
            let cleaned = lang.split('.').next().unwrap_or(&lang).trim();
            if !cleaned.is_empty() && !locales.iter().any(|value| value == cleaned) {
                locales.push(cleaned.to_string());
            }
        }

        if locales.is_empty() {
            locales.push("en_US".to_string());
        }

        locales
    }

    fn desktop_app_for_process<'a>(
        process: &sysinfo::Process,
        desktop_apps: &'a HashMap<String, DesktopAppMeta>,
    ) -> Option<&'a DesktopAppMeta> {
        for key in Self::process_candidate_keys(process) {
            if let Some(app) = desktop_apps.get(&key) {
                return Some(app);
            }
        }
        None
    }

    fn steam_app_id_for_process(
        process: &sysinfo::Process,
        processes: &HashMap<Pid, sysinfo::Process>,
    ) -> Option<String> {
        if let Some(app_id) = Self::extract_steam_app_id_from_process(process) {
            return Some(app_id);
        }

        let mut visited = HashSet::new();
        let mut parent = process.parent();
        let mut depth = 0usize;

        while let Some(parent_pid) = parent {
            if depth >= 12 || !visited.insert(parent_pid) {
                break;
            }

            let Some(parent_process) = processes.get(&parent_pid) else {
                break;
            };

            if let Some(app_id) = Self::extract_steam_app_id_from_process(parent_process) {
                return Some(app_id);
            }

            parent = parent_process.parent();
            depth += 1;
        }

        None
    }

    fn extract_steam_app_id_from_process(process: &sysinfo::Process) -> Option<String> {
        if let Some(app_id) = Self::extract_steam_app_id(process.name().to_string_lossy().as_ref())
        {
            return Some(app_id);
        }

        if let Some(cmd0) = process.cmd().first() {
            if let Some(app_id) = Self::extract_steam_app_id(cmd0.to_string_lossy().as_ref()) {
                return Some(app_id);
            }
        }

        if !process.cmd().is_empty() {
            let cmdline = process
                .cmd()
                .iter()
                .map(|part| part.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ");
            if let Some(app_id) = Self::extract_steam_app_id(&cmdline) {
                return Some(app_id);
            }

            for arg in process.cmd() {
                if let Some(app_id) = Self::extract_steam_app_id(arg.to_string_lossy().as_ref()) {
                    return Some(app_id);
                }
            }
        }

        None
    }

    fn extract_steam_app_id(value: &str) -> Option<String> {
        if value.trim().is_empty() {
            return None;
        }

        let lower = value.to_ascii_lowercase();
        for marker in ["appid=", "gameid=", "-gameid", "steam_app_", "rungameid/"] {
            if let Some(app_id) = Self::extract_decimal_after_marker(value, &lower, marker) {
                return Some(app_id);
            }
        }

        None
    }

    fn extract_decimal_after_marker(original: &str, lower: &str, marker: &str) -> Option<String> {
        let mut offset = 0usize;
        while let Some(found) = lower[offset..].find(marker) {
            let start = offset + found + marker.len();
            if let Some(app_id) = Self::extract_decimal_from(original, start) {
                return Some(app_id);
            }
            offset = start;
        }
        None
    }

    fn extract_decimal_from(value: &str, mut index: usize) -> Option<String> {
        let bytes = value.as_bytes();
        while index < bytes.len() {
            let c = bytes[index];
            if c.is_ascii_digit() {
                break;
            }
            if matches!(c, b' ' | b'=' | b':' | b'/' | b'-' | b'"' | b'\'') {
                index += 1;
                continue;
            }
            return None;
        }

        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }

        if start == index {
            return None;
        }

        let app_id = &value[start..index];
        if app_id == "0" {
            None
        } else {
            Some(app_id.to_string())
        }
    }

    fn load_steam_app_meta(app_id: &str, default_icon: Option<icon::Handle>) -> SteamAppMeta {
        let name =
            Self::steam_manifest_name(app_id).unwrap_or_else(|| format!("Steam App {app_id}"));
        let icon_handle = Self::steam_icon_path(app_id)
            .map(icon::from_path)
            .or(default_icon);

        SteamAppMeta { name, icon_handle }
    }

    fn steam_manifest_name(app_id: &str) -> Option<String> {
        for library_root in Self::steam_library_roots() {
            let steamapps = Self::steamapps_dir(&library_root);
            let manifest = steamapps.join(format!("appmanifest_{app_id}.acf"));
            if !manifest.is_file() {
                continue;
            }

            if let Ok(content) = fs::read_to_string(&manifest) {
                if let Some(name) = Self::acf_value(&content, "name") {
                    let trimmed = name.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }

        None
    }

    fn steam_icon_path(app_id: &str) -> Option<PathBuf> {
        for steam_root in Self::steam_root_paths() {
            let app_dir = steam_root
                .join("appcache")
                .join("librarycache")
                .join(app_id);
            if !app_dir.is_dir() {
                continue;
            }

            if let Some(path) = Self::preferred_icon_path_in_dir(&app_dir) {
                return Some(path);
            }

            if let Ok(entries) = fs::read_dir(&app_dir) {
                let mut nested_dirs = entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| path.is_dir())
                    .collect::<Vec<_>>();
                nested_dirs.sort();

                for nested in nested_dirs {
                    if let Some(path) = Self::preferred_icon_path_in_dir(&nested) {
                        return Some(path);
                    }
                }
            }
        }

        None
    }

    fn preferred_icon_path_in_dir(dir: &Path) -> Option<PathBuf> {
        for name in ["logo.png", "library_600x900.jpg", "library_header.jpg"] {
            let path = dir.join(name);
            if path.is_file() {
                return Some(path);
            }
        }

        let mut fallback = fs::read_dir(dir)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .map(|ext| {
                            matches!(
                                ext.to_ascii_lowercase().as_str(),
                                "png" | "jpg" | "jpeg" | "webp" | "svg"
                            )
                        })
                        .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        fallback.sort();
        fallback.into_iter().next()
    }

    fn steam_root_paths() -> Vec<PathBuf> {
        let mut candidates = Vec::new();

        if let Ok(compat_root) = env::var("STEAM_COMPAT_CLIENT_INSTALL_PATH") {
            let path = PathBuf::from(compat_root);
            if path.is_dir() {
                candidates.push(path);
            }
        }

        if let Ok(home) = env::var("HOME") {
            let local_share = PathBuf::from(&home)
                .join(".local")
                .join("share")
                .join("Steam");
            if local_share.is_dir() {
                candidates.push(local_share);
            }

            let legacy = PathBuf::from(home).join(".steam").join("steam");
            if legacy.is_dir() {
                candidates.push(legacy);
            }
        }

        let mut seen = HashSet::new();
        let mut unique = Vec::new();
        for path in candidates {
            let key = path.to_string_lossy().to_string();
            if seen.insert(key) {
                unique.push(path);
            }
        }
        unique
    }

    fn steam_library_roots() -> Vec<PathBuf> {
        let mut roots = Vec::new();
        for steam_root in Self::steam_root_paths() {
            roots.push(steam_root.clone());
            let libraryfolders = steam_root.join("steamapps").join("libraryfolders.vdf");
            if let Ok(content) = fs::read_to_string(libraryfolders) {
                roots.extend(Self::steam_library_roots_from_vdf(&content));
            }
        }

        let mut seen = HashSet::new();
        let mut unique = Vec::new();
        for path in roots {
            if !path.is_dir() {
                continue;
            }
            let key = path.to_string_lossy().to_string();
            if seen.insert(key) {
                unique.push(path);
            }
        }
        unique
    }

    fn steam_library_roots_from_vdf(vdf: &str) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        for line in vdf.lines() {
            let Some((key, value)) = Self::quoted_kv(line) else {
                continue;
            };
            if key != "path" {
                continue;
            }

            let unescaped = value.replace("\\\\", "\\");
            roots.push(PathBuf::from(unescaped));
        }
        roots
    }

    fn steamapps_dir(root: &Path) -> PathBuf {
        if root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("steamapps"))
        {
            root.to_path_buf()
        } else {
            root.join("steamapps")
        }
    }

    fn acf_value(content: &str, key: &str) -> Option<String> {
        for line in content.lines() {
            let Some((line_key, line_value)) = Self::quoted_kv(line) else {
                continue;
            };
            if line_key.eq_ignore_ascii_case(key) {
                return Some(line_value);
            }
        }
        None
    }

    fn quoted_kv(line: &str) -> Option<(String, String)> {
        let mut parts = line.split('"');
        let _before_key = parts.next()?;
        let key = parts.next()?.trim();
        let _between = parts.next()?;
        let value = parts.next()?.trim();
        if key.is_empty() {
            return None;
        }
        Some((key.to_string(), value.to_string()))
    }

    fn process_candidate_keys(process: &sysinfo::Process) -> Vec<String> {
        let mut keys = Vec::new();
        let mut seen = HashSet::new();

        if let Some(exe_name) = process
            .exe()
            .and_then(|exe| exe.file_stem().or_else(|| exe.file_name()))
            .map(|name| name.to_string_lossy().to_string())
        {
            for key in Self::exec_candidate_keys(&exe_name) {
                if seen.insert(key.clone()) {
                    keys.push(key);
                }
            }
        }

        if !process.cmd().is_empty() {
            let cmdline = process
                .cmd()
                .iter()
                .map(|part| part.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ");
            for key in Self::exec_candidate_keys(&cmdline) {
                if seen.insert(key.clone()) {
                    keys.push(key);
                }
            }

            for arg in process.cmd() {
                let arg = arg.to_string_lossy();
                if !Self::is_exec_like_arg(arg.as_ref()) {
                    continue;
                }
                for key in Self::exec_candidate_keys(arg.as_ref()) {
                    if seen.insert(key.clone()) {
                        keys.push(key);
                    }
                }
            }
        }

        if let Some(cmd0) = process.cmd().first() {
            let cmd0 = cmd0.to_string_lossy();
            for key in Self::exec_candidate_keys(cmd0.as_ref()) {
                if seen.insert(key.clone()) {
                    keys.push(key);
                }
            }
        }

        if keys.is_empty() {
            let process_name = process.name().to_string_lossy();
            for key in Self::exec_candidate_keys(process_name.as_ref()) {
                if seen.insert(key.clone()) {
                    keys.push(key);
                }
            }
        }

        keys
    }

    fn exec_candidate_keys(value: &str) -> Vec<String> {
        let token = Self::extract_match_token(value).unwrap_or_else(|| value.trim().to_string());
        let token = token.trim_matches('"').trim_matches('\'');
        let token = token.strip_suffix(".desktop").unwrap_or(token);
        let token = Path::new(token)
            .file_stem()
            .or_else(|| Path::new(token).file_name())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| token.to_string());

        let Some(normalized) = Self::normalize_exec_key(&token) else {
            return Vec::new();
        };
        if normalized.is_empty() {
            return Vec::new();
        }

        let mut out = vec![normalized.clone()];
        let mut alias = normalized;

        for suffix in ["-stable", "-beta", "-dev", "-bin"] {
            if alias.ends_with(suffix) {
                alias = alias.trim_end_matches(suffix).to_string();
            }
        }
        for suffix in ["-browser", "-desktop", "-applet"] {
            if alias.ends_with(suffix) {
                alias = alias.trim_end_matches(suffix).to_string();
            }
        }

        if !alias.is_empty() && !out.iter().any(|v| v == &alias) {
            out.push(alias.clone());
        }

        out
    }

    fn exec_primary_keys(value: &str) -> Vec<String> {
        let token = Self::extract_match_token(value).unwrap_or_else(|| value.trim().to_string());
        let token = token.trim_matches('"').trim_matches('\'');
        let token = token.strip_suffix(".desktop").unwrap_or(token);
        let token = Path::new(token)
            .file_stem()
            .or_else(|| Path::new(token).file_name())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| token.to_string());

        Self::normalize_exec_key(&token).into_iter().collect()
    }

    fn normalize_exec_key(value: &str) -> Option<String> {
        let normalized = value
            .trim()
            .replace([' ', '_', '.'], "-")
            .to_lowercase()
            .trim_matches('-')
            .to_string();

        if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        }
    }

    fn is_exec_like_arg(arg: &str) -> bool {
        if arg.starts_with('-') || arg.contains('=') || arg.len() < 3 {
            return false;
        }
        if !arg.chars().any(|c| c.is_ascii_alphabetic()) {
            return false;
        }
        arg.contains('/') || arg.contains('-') || arg.contains('.')
    }

    fn extract_match_token(value: &str) -> Option<String> {
        let tokens: Vec<&str> = value.split_whitespace().collect();
        if tokens.is_empty() {
            return None;
        }

        let command_stem = |token: &str| {
            Path::new(token)
                .file_name()
                .map(|part| part.to_string_lossy().to_lowercase())
                .unwrap_or_else(|| token.to_lowercase())
        };

        let mut index = 0;
        if command_stem(tokens[index]) == "env" {
            index += 1;
            while index < tokens.len() {
                let token = tokens[index];
                if token.contains('=') || token.starts_with('-') {
                    index += 1;
                } else {
                    break;
                }
            }
            if index >= tokens.len() {
                return None;
            }
        }

        if command_stem(tokens[index]) == "flatpak" {
            let mut idx = index + 1;
            if idx < tokens.len() && command_stem(tokens[idx]) == "run" {
                idx += 1;
                while idx < tokens.len() {
                    let flag = tokens[idx];
                    if !flag.starts_with('-') {
                        break;
                    }
                    idx += 1;

                    // Common flatpak run flags that take a separate value.
                    if matches!(
                        flag,
                        "--arch" | "--branch" | "--command" | "--file-forwarding"
                    ) && idx < tokens.len()
                        && !tokens[idx].starts_with('-')
                    {
                        idx += 1;
                    }
                }
                if idx < tokens.len() {
                    return Some(tokens[idx].to_string());
                }
            }
        }

        if matches!(
            command_stem(tokens[index]).as_str(),
            "steam" | "gtk-launch" | "xdg-open" | "sh" | "bash" | "zsh" | "fish"
        ) {
            return None;
        }

        Some(tokens[index].to_string())
    }

    fn toggle_sort(&mut self, column: SortColumn) {
        if self.sort_state.column == column {
            self.sort_state.direction = match self.sort_state.direction {
                SortDirection::Asc => SortDirection::Desc,
                SortDirection::Desc => SortDirection::Asc,
            };
        } else {
            self.sort_state = SortState {
                column,
                direction: Self::default_direction(column),
            };
        }
        self.sort_process_entries();
    }

    fn sort_process_entries(&mut self) {
        self.process_entries.sort_by(|a, b| {
            let primary = match self.sort_state.column {
                SortColumn::Name => a
                    .name
                    .to_lowercase()
                    .cmp(&b.name.to_lowercase())
                    .then_with(|| a.name.cmp(&b.name)),
                SortColumn::Cpu => a
                    .cpu_percent
                    .partial_cmp(&b.cpu_percent)
                    .unwrap_or(Ordering::Equal),
                SortColumn::Pid => a.pid.cmp(&b.pid),
                SortColumn::Ram => a.rss_bytes.cmp(&b.rss_bytes),
                SortColumn::Threads => a.threads.cmp(&b.threads),
            };

            let primary = match self.sort_state.direction {
                SortDirection::Asc => primary,
                SortDirection::Desc => primary.reverse(),
            };

            primary
                .then_with(|| b.rss_bytes.cmp(&a.rss_bytes))
                .then_with(|| {
                    b.cpu_percent
                        .partial_cmp(&a.cpu_percent)
                        .unwrap_or(Ordering::Equal)
                })
                .then_with(|| a.pid.cmp(&b.pid))
        });
    }

    fn default_direction(column: SortColumn) -> SortDirection {
        match column {
            SortColumn::Name => SortDirection::Asc,
            SortColumn::Cpu | SortColumn::Pid | SortColumn::Ram | SortColumn::Threads => {
                SortDirection::Desc
            }
        }
    }

    fn header_button_content<'a>(
        &self,
        label: &'a str,
        column: SortColumn,
    ) -> Element<'a, Message> {
        let mut row = widget::row::with_capacity(2)
            .push(widget::text(label))
            .align_y(Alignment::Center)
            .spacing(6);

        if self.sort_state.column == column {
            let arrow_icon_name = match self.sort_state.direction {
                SortDirection::Asc => "pan-up-symbolic",
                SortDirection::Desc => "pan-down-symbolic",
            };
            row = row.push(
                widget::icon::from_name(arrow_icon_name)
                    .icon()
                    .size(14)
                    .class(theme::Svg::custom(|_| cosmic::iced_widget::svg::Style {
                        color: Some(Color::WHITE),
                    })),
            );
        }

        widget::container(row)
            .width(Length::Fill)
            .align_x(Horizontal::Center)
            .into()
    }

    fn is_program_process(
        process: &sysinfo::Process,
        current_user_id: Option<&sysinfo::Uid>,
    ) -> bool {
        if let Some(uid) = current_user_id {
            if process.user_id() != Some(uid) {
                return false;
            }
        }

        let Some(exe) = process.exe() else {
            return false;
        };

        if exe.as_os_str().is_empty() {
            return false;
        }

        let name = process.name().to_string_lossy();
        if name.trim().is_empty() || name.starts_with('[') {
            return false;
        }

        if Self::is_background_component_process(process) {
            return false;
        }

        true
    }

    fn is_background_component_process(process: &sysinfo::Process) -> bool {
        if let Some(exe_name) = process
            .exe()
            .and_then(|exe| exe.file_stem().or_else(|| exe.file_name()))
        {
            if Self::looks_like_background_component(exe_name.to_string_lossy().as_ref()) {
                return true;
            }
        }

        if let Some(cmd0) = process.cmd().first() {
            let cmd0 = cmd0.to_string_lossy();
            let cmd0_name = Path::new(cmd0.as_ref())
                .file_stem()
                .or_else(|| Path::new(cmd0.as_ref()).file_name())
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| cmd0.to_string());

            if Self::looks_like_background_component(&cmd0_name) {
                return true;
            }
        }

        Self::looks_like_background_component(process.name().to_string_lossy().as_ref())
    }

    fn looks_like_background_component(token: &str) -> bool {
        let token = token.trim().to_ascii_lowercase();
        if token.is_empty() {
            return false;
        }

        token.contains("daemon")
            || token.contains("applet")
            || token.contains("helper")
            || token.contains("service")
    }

    fn is_excluded_app_id(app_id: &str) -> bool {
        app_id.contains("cosmicapplet")
            || app_id.contains("cosmic-applet")
            || app_id.contains("cosmic-panel-button")
            || app_id.contains("cosmic-status-area")
            || app_id.contains("cosmic-notifications")
            || app_id.contains("cosmic-osd")
            || app_id.contains("cosmic-workspaces")
            || app_id.contains("cosmic-launcher")
            || app_id.contains("cosmic-greeter")
            || app_id.contains("xdg-desktop-portal")
            || app_id.contains("daemon")
    }

    fn debug_enabled() -> bool {
        env::var("COSMIC_TM_DEBUG")
            .ok()
            .map(|value| {
                let normalized = value.trim().to_ascii_lowercase();
                matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
            })
            .unwrap_or(false)
    }

    fn debug_log(message: &str) {
        eprintln!("{message}");
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(DEBUG_LOG_PATH)
        {
            let _ = writeln!(file, "{message}");
        }
    }

    fn format_rss(bytes: u64) -> String {
        let gib = bytes as f64 / 1024.0 / 1024.0 / 1024.0;
        if gib >= 1.0 {
            format!("{gib:.1}GB")
        } else {
            let mib = bytes as f64 / 1024.0 / 1024.0;
            format!("{mib:.1}MB")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppModel;

    #[test]
    fn extracts_steam_app_id_from_reaper_cmdline() {
        let value = "SteamLaunch AppId=1903340 -- proton waitforexitandrun";
        assert_eq!(
            AppModel::extract_steam_app_id(value),
            Some("1903340".to_string())
        );
    }

    #[test]
    fn extracts_steam_app_id_from_gameoverlay_flag() {
        let value = "gameoverlayui -pid 333322 -steampid 327614 -gameid 1903340";
        assert_eq!(
            AppModel::extract_steam_app_id(value),
            Some("1903340".to_string())
        );
    }

    #[test]
    fn extracts_steam_app_id_from_steam_app_token() {
        let value = "steam_app_730";
        assert_eq!(
            AppModel::extract_steam_app_id(value),
            Some("730".to_string())
        );
    }

    #[test]
    fn extracts_name_from_acf_line() {
        let content = r#"
"AppState"
{
    "appid"     "1903340"
    "name"      "Clair Obscur: Expedition 33"
}
"#;
        assert_eq!(
            AppModel::acf_value(content, "name"),
            Some("Clair Obscur: Expedition 33".to_string())
        );
    }

    #[test]
    fn extracts_library_roots_from_vdf_path_lines() {
        let vdf = r#"
"libraryfolders"
{
    "0"
    {
        "path"      "/home/exepta/.local/share/Steam"
    }
    "1"
    {
        "path"      "/run/media/exepta/Games/SteamLibrary"
    }
}
"#;
        let roots = AppModel::steam_library_roots_from_vdf(vdf);
        assert!(roots.iter().any(|p| p.ends_with("Steam")));
        assert!(roots.iter().any(|p| p.ends_with("SteamLibrary")));
    }
}

pub enum Page {
    Page1,
    Page2,
    Page3,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum ContextPage {
    #[default]
    About,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    About,
}

impl menu::action::MenuAction for MenuAction {
    type Message = Message;

    fn message(&self) -> Self::Message {
        match self {
            MenuAction::About => Message::ToggleContextPage(ContextPage::About),
        }
    }
}
