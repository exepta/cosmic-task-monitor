// SPDX-License-Identifier: MPL-2.0

use crate::config::Config;
use crate::fl;
use cosmic::app::context_drawer;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::desktop::{self, IconSourceExt};
use cosmic::iced::alignment::Horizontal;
use cosmic::iced::{Alignment, Background, Border, Color, Length, Subscription};
use cosmic::theme;
use cosmic::widget::{self, about::About, icon, menu, nav_bar};
use cosmic::{iced_futures, prelude::*};
use futures_util::SinkExt;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use sysinfo::{Disks, Pid, ProcessRefreshKind, ProcessesToUpdate, Signal, System, UpdateKind};

const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const APP_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/apps/icon.svg");
const PROCESS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const PERFORMANCE_HISTORY_POINTS: usize = 60;
const CPU_ACCENT: Color = Color::from_rgb(155.0 / 255.0, 88.0 / 255.0, 180.0 / 255.0);
const RAM_ACCENT: Color = Color::from_rgb(126.0 / 255.0, 189.0 / 255.0, 195.0 / 255.0);
const DISK_ACCENT: Color = Color::from_rgb(197.0 / 255.0, 196.0 / 255.0, 67.0 / 255.0);

mod process;

fn table_cell_style(theme: &Theme) -> widget::container::Style {
    widget::container::Style {
        border: Border {
            color: theme.cosmic().bg_divider().into(),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

fn table_row_button_style() -> theme::Button {
    theme::Button::Custom {
        active: Box::new(|_focused, _theme| {
            let mut style = widget::button::Style::new();
            style.border_width = 1.0;
            style.border_color = Color::TRANSPARENT;
            style.border_radius = 0.0.into();
            style
        }),
        hovered: Box::new(|_focused, theme| {
            let mut style = widget::button::Style::new();
            style.background = Some(Background::Color(
                theme.current_container().component.hover.into(),
            ));
            style.border_width = 1.0;
            style.border_color = theme.cosmic().accent_color().into();
            style.border_radius = 0.0.into();
            style
        }),
        pressed: Box::new(|_focused, theme| {
            let mut style = widget::button::Style::new();
            style.background = Some(Background::Color(
                theme.current_container().component.hover.into(),
            ));
            style.border_width = 1.0;
            style.border_color = theme.cosmic().accent_color().into();
            style.border_radius = 0.0.into();
            style
        }),
        disabled: Box::new(|_theme| {
            let mut style = widget::button::Style::new();
            style.border_width = 1.0;
            style.border_color = Color::TRANSPARENT;
            style.border_radius = 0.0.into();
            style
        }),
    }
}

#[derive(Debug, Clone)]
struct ProcessEntry {
    app_id: String,
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
    desktop_entry_id: Option<String>,
    desktop_entry_path: Option<PathBuf>,
    exec_command: Option<String>,
}

#[derive(Clone)]
struct SteamAppMeta {
    name: String,
    icon_handle: Option<icon::Handle>,
}

#[derive(Debug, Clone)]
struct SelectedProcess {
    app_id: String,
    display_name: String,
    pid: u32,
}

#[derive(Debug, Clone)]
struct CpuStaticInfo {
    sockets: String,
    virtualization: String,
    l1_cache: String,
    l2_cache: String,
    l3_cache: String,
}

#[derive(Debug, Clone)]
struct DiskGroupInfo {
    name: String,
    total_bytes: u64,
    used_bytes: u64,
    kind_label: String,
    partitions: Vec<String>,
    is_mounted: bool,
    is_system_disk: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct DiskRuntimeInfo {
    active_time_percent: f32,
    avg_response_ms: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct DiskIoSnapshot {
    reads_completed: u64,
    writes_completed: u64,
    io_time_ms: u64,
    weighted_io_time_ms: u64,
}

#[derive(Debug, Clone)]
struct DiskBlockEntry {
    name: String,
    block_type: String,
    mountpoint: String,
}

impl Default for CpuStaticInfo {
    fn default() -> Self {
        Self {
            sockets: "N/A".to_string(),
            virtualization: "N/A".to_string(),
            l1_cache: "N/A".to_string(),
            l2_cache: "N/A".to_string(),
            l3_cache: "N/A".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
enum LaunchCandidate {
    SteamUri(String),
    GtkLaunch(String),
    GioLaunch(PathBuf),
    DesktopExec(String),
    Command { program: String, args: Vec<String> },
    Executable(PathBuf),
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
pub enum AppsViewMode {
    List,
    Tile,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum PerformanceViewMode {
    Cpu,
    Ram,
    Disk(String),
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
    disks: Disks,
    desktop_apps_by_exec: HashMap<String, DesktopAppMeta>,
    steam_apps_by_id: HashMap<String, SteamAppMeta>,
    process_entries: Vec<ProcessEntry>,
    selected_process: Option<SelectedProcess>,
    apps_view_mode: AppsViewMode,
    performance_view_mode: PerformanceViewMode,
    cpu_usage_history_per_core: Vec<Vec<f32>>,
    ram_usage_history: Vec<f32>,
    disk_read_history: HashMap<String, Vec<f32>>,
    disk_write_history: HashMap<String, Vec<f32>>,
    disk_runtime_info: HashMap<String, DiskRuntimeInfo>,
    disk_previous_snapshots: HashMap<String, DiskIoSnapshot>,
    cpu_static_info: CpuStaticInfo,
    sort_state: SortState,
}

#[derive(Debug, Clone)]
pub enum Message {
    LaunchUrl(String),
    ToggleContextPage(ContextPage),
    UpdateConfig(Config),
    RefreshProcesses,
    SetAppsViewMode(AppsViewMode),
    SetPerformanceViewMode(PerformanceViewMode),
    MountDisk(String),
    UnmountDisk(String),
    ToggleSort(SortColumn),
    OpenProcessMenu {
        app_id: String,
        display_name: String,
        pid: u32,
    },
    CloseProcessMenu,
    RestartSelectedApplication,
    FocusSelectedApplication,
    StopSelectedApplication,
    KillSelectedApplication,
    OpenSelectedApplicationPath,
    CopySelectedApplicationInfo,
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "com.github.exepta.cosmic-task-monitor";

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
            .text(fl!("nav-apps"))
            .data::<Page>(Page::Page1)
            .icon(icon::from_name("applications-other-symbolic"))
            .activate();

        nav.insert()
            .text(fl!("nav-performance"))
            .data::<Page>(Page::Page2)
            .icon(icon::from_name("utilities-system-monitor-symbolic"));

        nav.insert()
            .text(fl!("nav-autostart"))
            .data::<Page>(Page::Page3)
            .icon(icon::from_name("system-run-symbolic"));

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
            disks: Disks::new_with_refreshed_list(),
            desktop_apps_by_exec: Self::load_desktop_app_map(),
            steam_apps_by_id: HashMap::new(),
            process_entries: Vec::new(),
            selected_process: None,
            apps_view_mode: AppsViewMode::List,
            performance_view_mode: PerformanceViewMode::Cpu,
            cpu_usage_history_per_core: Vec::new(),
            ram_usage_history: Vec::new(),
            disk_read_history: HashMap::new(),
            disk_write_history: HashMap::new(),
            disk_runtime_info: HashMap::new(),
            disk_previous_snapshots: HashMap::new(),
            cpu_static_info: Self::read_cpu_static_info(),
            sort_state: SortState {
                column: SortColumn::Ram,
                direction: SortDirection::Desc,
            },
        };

        let command = app.update_title();
        (app, command)
    }

    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        let menu_bar = menu::bar(vec![
            menu::Tree::with_children(
                menu::root(fl!("view")).apply(Element::from),
                menu::items(
                    &self.key_binds,
                    vec![
                        menu::Item::CheckBox(
                            fl!("list"),
                            None,
                            self.apps_view_mode == AppsViewMode::List,
                            MenuAction::ViewList,
                        ),
                        menu::Item::CheckBox(
                            fl!("tile"),
                            None,
                            self.apps_view_mode == AppsViewMode::Tile,
                            MenuAction::ViewTile,
                        ),
                    ],
                ),
            ),
            menu::Tree::with_children(
                menu::root(fl!("settings")).apply(Element::from),
                menu::items(
                    &self.key_binds,
                    vec![menu::Item::Button(fl!("about"), None, MenuAction::About)],
                ),
            ),
            menu::Tree::with_children(
                menu::root(fl!("help")).apply(Element::from),
                menu::items(
                    &self.key_binds,
                    vec![menu::Item::Button(fl!("about"), None, MenuAction::About)],
                ),
            ),
        ]);

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
            ContextPage::ProcessActions => {
                let title = self
                    .selected_process
                    .as_ref()
                    .map(|entry| entry.display_name.clone())
                    .unwrap_or_else(|| fl!("process-actions-title"));

                let button_height = Length::Fixed(38.0);
                let content: Element<'_, Message> =
                    if let Some(selected) = self.selected_process.as_ref() {
                        widget::column::with_capacity(8)
                            .push(widget::text(fl!("process-pid", pid = selected.pid)))
                            .push(
                                widget::button::standard(fl!("process-action-restart"))
                                    .class(theme::Button::Standard)
                                    .on_press(Message::RestartSelectedApplication)
                                    .width(Length::Fill)
                                    .height(button_height),
                            )
                            .push(
                                widget::button::standard(fl!("process-action-focus"))
                                    .on_press(Message::FocusSelectedApplication)
                                    .width(Length::Fill)
                                    .height(button_height),
                            )
                            .push(
                                widget::button::standard(fl!("process-action-stop"))
                                    .on_press(Message::StopSelectedApplication)
                                    .width(Length::Fill)
                                    .height(button_height),
                            )
                            .push(
                                widget::button::destructive(fl!("process-action-kill"))
                                    .on_press(Message::KillSelectedApplication)
                                    .width(Length::Fill)
                                    .height(button_height),
                            )
                            .push(
                                widget::button::standard(fl!("process-action-open-path"))
                                    .on_press(Message::OpenSelectedApplicationPath)
                                    .width(Length::Fill)
                                    .height(button_height),
                            )
                            .push(
                                widget::button::standard(fl!("process-action-copy-info"))
                                    .on_press(Message::CopySelectedApplicationInfo)
                                    .width(Length::Fill)
                                    .height(button_height),
                            )
                            .spacing(8)
                            .width(Length::Fill)
                            .into()
                    } else {
                        widget::text(fl!("process-none-selected")).into()
                    };

                context_drawer::context_drawer(content, Message::CloseProcessMenu).title(title)
            }
        })
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let content: Element<_> = match self.nav.active_data::<Page>().unwrap() {
            Page::Page1 => {
                let header = widget::row::with_capacity(1)
                    .push(widget::text::title2(fl!(
                        "apps-title",
                        count = self.process_entries.len()
                    )))
                    .align_y(Alignment::Center)
                    .spacing(space_s);

                let sort_controls =
                    widget::row::with_capacity(5)
                        .push(
                            widget::container(
                                widget::button::custom(
                                    self.header_button_content(fl!("table-name"), SortColumn::Name),
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
                                    self.header_button_content(fl!("table-cpu"), SortColumn::Cpu),
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
                                    self.header_button_content(fl!("table-pid"), SortColumn::Pid),
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
                                    self.header_button_content(fl!("table-ram"), SortColumn::Ram),
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
                                widget::button::custom(self.header_button_content(
                                    fl!("table-threads"),
                                    SortColumn::Threads,
                                ))
                                .on_press(Message::ToggleSort(SortColumn::Threads))
                                .width(Length::Fill),
                            )
                            .padding(10)
                            .class(theme::Container::custom(table_cell_style))
                            .width(Length::FillPortion(2)),
                        )
                        .spacing(0);

                let list_rows = self.process_entries.iter().fold(
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
                            widget::button::custom(
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
                                        widget::container(widget::text(
                                            process.threads.to_string(),
                                        ))
                                        .padding(10)
                                        .class(theme::Container::custom(table_cell_style))
                                        .width(Length::FillPortion(2)),
                                    )
                                    .spacing(0)
                                    .width(Length::Fill),
                            )
                            .on_press(Message::OpenProcessMenu {
                                app_id: process.app_id.clone(),
                                display_name: process.display_name.clone(),
                                pid: process.pid,
                            })
                            .padding(0)
                            .class(table_row_button_style())
                            .width(Length::Fill),
                        )
                    },
                );

                let page_content: Element<'_, Message> = match self.apps_view_mode {
                    AppsViewMode::List => widget::column::with_capacity(3)
                        .push(header)
                        .push(sort_controls)
                        .push(widget::scrollable(list_rows).height(Length::Fill))
                        .spacing(space_s)
                        .height(Length::Fill)
                        .into(),
                    AppsViewMode::Tile => {
                        let tile_grid = widget::responsive(move |size| {
                            let spacing = space_s as f32;
                            let min_tile_width = 320.0;
                            let tile_columns = (((size.width + spacing)
                                / (min_tile_width + spacing))
                                .floor() as usize)
                                .clamp(1, 4);

                            let mut tile_rows = widget::column::with_capacity(
                                (self.process_entries.len() + tile_columns - 1) / tile_columns,
                            )
                            .spacing(space_s)
                            .width(Length::Fill);

                            for chunk in self.process_entries.chunks(tile_columns) {
                                let mut tile_row = widget::row::with_capacity(tile_columns)
                                    .spacing(space_s)
                                    .width(Length::Fill);

                                for process in chunk {
                                    let icon_content: Element<'_, Message> =
                                        if let Some(icon_handle) = process.icon_handle.as_ref() {
                                            widget::icon::icon(icon_handle.clone()).size(56).into()
                                        } else {
                                            widget::container(widget::text(""))
                                                .width(Length::Fixed(56.0))
                                                .into()
                                        };

                                    let details = widget::column::with_capacity(5)
                                        .push(widget::text(process.display_name.as_str()).size(20))
                                        .push(
                                            widget::text(format!(
                                                "{}: {}",
                                                fl!("table-pid"),
                                                process.pid
                                            ))
                                            .size(12),
                                        )
                                        .push(
                                            widget::text(format!(
                                                "{}: {:.1}%",
                                                fl!("table-cpu"),
                                                process.cpu_percent
                                            ))
                                            .size(12),
                                        )
                                        .push(
                                            widget::text(format!(
                                                "{}: {}",
                                                fl!("table-ram"),
                                                Self::format_rss(process.rss_bytes)
                                            ))
                                            .size(12),
                                        )
                                        .push(
                                            widget::text(format!(
                                                "{}: {}",
                                                fl!("table-threads"),
                                                process.threads
                                            ))
                                            .size(12),
                                        )
                                        .spacing(6)
                                        .width(Length::Fill);

                                    let tile_content = widget::container(
                                        widget::row::with_capacity(2)
                                            .push(
                                                widget::container(icon_content)
                                                    .center_x(Length::Fixed(56.0)),
                                            )
                                            .push(details)
                                            .spacing(25)
                                            .align_y(Alignment::Center)
                                            .width(Length::Fill),
                                    )
                                    .padding(12)
                                    .class(theme::Container::custom(table_cell_style))
                                    .width(Length::Fill);

                                    let tile_button = widget::button::custom(tile_content)
                                        .on_press(Message::OpenProcessMenu {
                                            app_id: process.app_id.clone(),
                                            display_name: process.display_name.clone(),
                                            pid: process.pid,
                                        })
                                        .padding(0)
                                        .class(table_row_button_style())
                                        .width(Length::Fill);

                                    tile_row = tile_row.push(
                                        widget::container(tile_button)
                                            .width(Length::FillPortion(1)),
                                    );
                                }

                                for _ in chunk.len()..tile_columns {
                                    tile_row = tile_row.push(
                                        widget::container(widget::text(""))
                                            .width(Length::FillPortion(1))
                                            .height(Length::Shrink),
                                    );
                                }

                                tile_rows = tile_rows.push(tile_row);
                            }

                            widget::scrollable(tile_rows).height(Length::Fill).into()
                        });

                        widget::column::with_capacity(3)
                            .push(header)
                            .push(sort_controls)
                            .push(tile_grid)
                            .spacing(space_s)
                            .height(Length::Fill)
                            .into()
                    }
                };

                page_content
            }

            Page::Page2 => self.performance_view(space_s),

            Page::Page3 => {
                let header = widget::row::with_capacity(2)
                    .push(widget::text::title1(fl!("welcome")))
                    .push(widget::text::title3(fl!("nav-autostart")))
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
            Message::SetAppsViewMode(mode) => self.apps_view_mode = mode,
            Message::SetPerformanceViewMode(mode) => self.performance_view_mode = mode,
            Message::MountDisk(disk_name) => {
                self.mount_disk(&disk_name);
                self.refresh_processes();
            }
            Message::UnmountDisk(disk_name) => {
                let is_system_disk = self
                    .collect_disk_groups()
                    .into_iter()
                    .find(|disk| disk.name == disk_name)
                    .is_some_and(|disk| disk.is_system_disk);
                if !is_system_disk {
                    self.unmount_disk(&disk_name);
                    self.refresh_processes();
                }
            }
            Message::ToggleSort(column) => self.toggle_sort(column),
            Message::OpenProcessMenu {
                app_id,
                display_name,
                pid,
            } => {
                self.selected_process = Some(SelectedProcess {
                    app_id,
                    display_name,
                    pid,
                });
                self.context_page = ContextPage::ProcessActions;
                self.core.window.show_context = true;
            }
            Message::CloseProcessMenu => {
                self.core.window.show_context = false;
                if self.context_page == ContextPage::ProcessActions {
                    self.selected_process = None;
                }
            }
            Message::RestartSelectedApplication => {
                self.restart_selected_application();
                self.core.window.show_context = false;
            }
            Message::FocusSelectedApplication => {
                self.focus_selected_application();
                self.core.window.show_context = false;
            }
            Message::StopSelectedApplication => {
                self.signal_selected_application(Signal::Term);
                self.core.window.show_context = false;
            }
            Message::KillSelectedApplication => {
                self.signal_selected_application(Signal::Kill);
                self.core.window.show_context = false;
            }
            Message::OpenSelectedApplicationPath => {
                self.open_selected_application_path();
                self.core.window.show_context = false;
            }
            Message::CopySelectedApplicationInfo => {
                self.copy_selected_application_info();
                self.core.window.show_context = false;
            }
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
    fn performance_view(&self, space_s: u16) -> Element<'_, Message> {
        let cpu_usage = self.system.global_cpu_usage().clamp(0.0, 100.0);
        let avg_freq_mhz = if self.system.cpus().is_empty() {
            0_u64
        } else {
            self.system
                .cpus()
                .iter()
                .map(|cpu| cpu.frequency())
                .sum::<u64>()
                / self.system.cpus().len() as u64
        };
        let current_speed_mhz = Self::read_current_cpu_speed_mhz().unwrap_or(avg_freq_mhz);
        let total_memory = self.system.total_memory();
        let used_memory = self.system.used_memory().min(total_memory);
        let ram_usage = if total_memory > 0 {
            (used_memory as f32 / total_memory as f32 * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };

        let cpu_card = self.performance_selector_card(
            fl!("table-cpu"),
            format!("{cpu_usage:.1}%"),
            Some(format!("{} GHz", Self::format_ghz(current_speed_mhz))),
            CPU_ACCENT,
            self.performance_view_mode == PerformanceViewMode::Cpu,
            Some(Message::SetPerformanceViewMode(PerformanceViewMode::Cpu)),
        );
        let ram_card = self.performance_selector_card(
            fl!("table-ram"),
            format!(
                "{} / {} ({ram_usage:.0}%)",
                Self::format_rss(used_memory),
                Self::format_rss(total_memory)
            ),
            None,
            RAM_ACCENT,
            self.performance_view_mode == PerformanceViewMode::Ram,
            Some(Message::SetPerformanceViewMode(PerformanceViewMode::Ram)),
        );

        let mut grouped_disks = self.collect_disk_groups();
        grouped_disks.sort_by(|a, b| a.name.cmp(&b.name));

        let mut sidebar = widget::column::with_capacity(3 + grouped_disks.len())
            .push(widget::text::title2(fl!("nav-performance")))
            .push(cpu_card)
            .push(ram_card)
            .spacing(space_s);

        for disk in &grouped_disks {
            let usage = if disk.total_bytes > 0 {
                (disk.used_bytes as f32 / disk.total_bytes as f32 * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            };
            let mode = PerformanceViewMode::Disk(disk.name.clone());
            let is_selected = self.performance_view_mode == mode;

            sidebar = sidebar.push(self.disk_selector_card(
                format!("Disk {}", disk.name),
                disk.kind_label.clone(),
                format!(
                    "{} / {} ({usage:.0}%)",
                    Self::format_rss(disk.used_bytes),
                    Self::format_rss(disk.total_bytes)
                ),
                disk.is_mounted,
                is_selected,
                Some(Message::SetPerformanceViewMode(mode)),
            ));
        }

        let sidebar = sidebar.width(Length::Fill);

        let detail: Element<'_, Message> = match &self.performance_view_mode {
            PerformanceViewMode::Cpu => self.cpu_detail_panel(cpu_usage, space_s),
            PerformanceViewMode::Ram => {
                self.ram_detail_panel(used_memory, total_memory, ram_usage, space_s)
            }
            PerformanceViewMode::Disk(selected_disk) => {
                if let Some(disk) = grouped_disks
                    .iter()
                    .find(|disk| &disk.name == selected_disk)
                {
                    self.disk_detail_panel(
                        disk.name.as_str(),
                        disk.total_bytes,
                        disk.used_bytes,
                        disk.kind_label.clone(),
                        disk.is_mounted,
                        disk.is_system_disk,
                        &disk.partitions,
                        space_s,
                    )
                } else if let Some(disk) = grouped_disks.first() {
                    self.disk_detail_panel(
                        disk.name.as_str(),
                        disk.total_bytes,
                        disk.used_bytes,
                        disk.kind_label.clone(),
                        disk.is_mounted,
                        disk.is_system_disk,
                        &disk.partitions,
                        space_s,
                    )
                } else {
                    widget::container(widget::text("No disks found"))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                }
            }
        };

        widget::row::with_capacity(2)
            .push(
                widget::container(widget::scrollable(sidebar).height(Length::Fill))
                    .width(Length::Fixed(280.0))
                    .height(Length::Fill),
            )
            .push(widget::container(detail).width(Length::Fill))
            .spacing(space_s)
            .height(Length::Fill)
            .into()
    }

    fn performance_selector_card(
        &self,
        title: String,
        value: String,
        value_suffix: Option<String>,
        accent: Color,
        is_selected: bool,
        on_press: Option<Message>,
    ) -> widget::Button<'_, Message> {
        let value_row: Element<'_, Message> = if let Some(suffix) = value_suffix {
            widget::row::with_capacity(2)
                .push(widget::text(value).size(14))
                .push(widget::text(suffix).size(14))
                .spacing(15)
                .into()
        } else {
            widget::text(value).size(14).into()
        };

        let mut button = widget::button::custom(
            widget::row::with_capacity(2)
                .push(
                    widget::container(widget::text(""))
                        .class(theme::Container::custom(move |_theme| {
                            widget::container::Style {
                                background: Some(Background::Color(accent)),
                                border: Border {
                                    color: Color::TRANSPARENT,
                                    width: 0.0,
                                    radius: 0.0.into(),
                                },
                                ..Default::default()
                            }
                        }))
                        .width(Length::Fixed(4.0))
                        .height(Length::Fill),
                )
                .push(
                    widget::column::with_capacity(2)
                        .push(widget::text(title).size(20))
                        .push(value_row)
                        .spacing(4)
                        .width(Length::Fill),
                )
                .spacing(12)
                .width(Length::Fill)
                .align_y(Alignment::Center),
        )
        .class(theme::Button::Custom {
            active: Box::new(move |_focused, theme| {
                let mut style = widget::button::Style::new();
                if is_selected {
                    style.background = Some(Background::Color(
                        theme.current_container().component.hover.into(),
                    ));
                    style.border_color = accent;
                } else {
                    style.border_color = theme.cosmic().bg_divider().into();
                }
                style.border_width = 1.0;
                style.border_radius = 10.0.into();
                style
            }),
            hovered: Box::new(move |_focused, _theme| {
                let mut style = widget::button::Style::new();
                style.background = Some(Background::Color(Color { a: 0.08, ..accent }));
                style.border_width = 1.0;
                style.border_color = accent;
                style.border_radius = 10.0.into();
                style
            }),
            pressed: Box::new(move |_focused, _theme| {
                let mut style = widget::button::Style::new();
                style.background = Some(Background::Color(Color { a: 0.16, ..accent }));
                style.border_width = 1.0;
                style.border_color = accent;
                style.border_radius = 10.0.into();
                style
            }),
            disabled: Box::new(move |_theme| {
                let mut style = widget::button::Style::new();
                style.border_width = 1.0;
                style.border_color = accent;
                style.border_radius = 10.0.into();
                style
            }),
        })
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fixed(110.0));

        if let Some(message) = on_press {
            button = button.on_press(message);
        }

        button
    }

    fn disk_selector_card(
        &self,
        title: String,
        disk_kind: String,
        usage_text: String,
        is_mounted: bool,
        is_selected: bool,
        on_press: Option<Message>,
    ) -> widget::Button<'_, Message> {
        let mut title_row = widget::row::with_capacity(3)
            .push(widget::text(title).size(18))
            .push(widget::horizontal_space())
            .width(Length::Fill)
            .align_y(Alignment::Center);

        if is_mounted {
            title_row = title_row.push(
                widget::icon::from_name("emblem-ok-symbolic")
                    .icon()
                    .size(14)
                    .class(theme::Svg::custom(|_| cosmic::iced_widget::svg::Style {
                        color: Some(DISK_ACCENT),
                    })),
            );
        }

        let mut button = widget::button::custom(
            widget::row::with_capacity(2)
                .push(
                    widget::container(widget::text(""))
                        .class(theme::Container::custom(move |_theme| {
                            widget::container::Style {
                                background: Some(Background::Color(DISK_ACCENT)),
                                border: Border {
                                    color: Color::TRANSPARENT,
                                    width: 0.0,
                                    radius: 0.0.into(),
                                },
                                ..Default::default()
                            }
                        }))
                        .width(Length::Fixed(4.0))
                        .height(Length::Fill),
                )
                .push(
                    widget::column::with_capacity(3)
                        .push(title_row)
                        .push(widget::text(disk_kind).size(13))
                        .push(widget::text(usage_text).size(13))
                        .spacing(4)
                        .width(Length::Fill),
                )
                .spacing(12)
                .width(Length::Fill)
                .align_y(Alignment::Center),
        )
        .class(theme::Button::Custom {
            active: Box::new(move |_focused, theme| {
                let mut style = widget::button::Style::new();
                if is_selected {
                    style.background = Some(Background::Color(
                        theme.current_container().component.hover.into(),
                    ));
                    style.border_color = DISK_ACCENT;
                } else {
                    style.background = Some(Background::Color(
                        theme.current_container().component.base.into(),
                    ));
                    style.border_color = theme.cosmic().bg_divider().into();
                }
                style.border_width = 1.0;
                style.border_radius = 10.0.into();
                style
            }),
            hovered: Box::new(move |_focused, _theme| {
                let mut style = widget::button::Style::new();
                style.background = Some(Background::Color(Color {
                    a: 0.08,
                    ..DISK_ACCENT
                }));
                style.border_width = 1.0;
                style.border_color = DISK_ACCENT;
                style.border_radius = 10.0.into();
                style
            }),
            pressed: Box::new(move |_focused, _theme| {
                let mut style = widget::button::Style::new();
                style.background = Some(Background::Color(Color {
                    a: 0.16,
                    ..DISK_ACCENT
                }));
                style.border_width = 1.0;
                style.border_color = DISK_ACCENT;
                style.border_radius = 10.0.into();
                style
            }),
            disabled: Box::new(move |_theme| {
                let mut style = widget::button::Style::new();
                style.border_width = 1.0;
                style.border_color = DISK_ACCENT;
                style.border_radius = 10.0.into();
                style
            }),
        })
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fixed(115.0));

        if let Some(message) = on_press {
            button = button.on_press(message);
        }

        button
    }

    fn disk_detail_panel(
        &self,
        disk_name: &str,
        total: u64,
        used: u64,
        kind_label: String,
        is_mounted: bool,
        is_system_disk: bool,
        partitions: &[String],
        space_s: u16,
    ) -> Element<'_, Message> {
        let read_history = self
            .disk_read_history
            .get(disk_name)
            .cloned()
            .unwrap_or_default();
        let write_history = self
            .disk_write_history
            .get(disk_name)
            .cloned()
            .unwrap_or_default();
        let read_now = *read_history.last().unwrap_or(&0.0);
        let write_now = *write_history.last().unwrap_or(&0.0);
        let runtime_info = self
            .disk_runtime_info
            .get(disk_name)
            .copied()
            .unwrap_or_default();
        let usage = if total > 0 {
            (used as f32 / total as f32 * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };
        let used_portion = usage.round().clamp(0.0, 100.0) as u16;
        let free_portion = 100_u16.saturating_sub(used_portion);

        let mut partition_tiles = widget::row::with_capacity(partitions.len().max(1))
            .spacing(8)
            .width(Length::Fill);
        for partition in partitions.iter().cloned() {
            partition_tiles = partition_tiles.push(
                widget::container(widget::text(partition).size(13))
                    .padding([4, 10])
                    .class(theme::Container::custom(move |_theme| {
                        widget::container::Style {
                            background: Some(Background::Color(Color {
                                a: 0.18,
                                ..DISK_ACCENT
                            })),
                            border: Border {
                                color: DISK_ACCENT,
                                width: 1.0,
                                radius: 6.0.into(),
                            },
                            ..Default::default()
                        }
                    })),
            );
        }

        let io_stats = widget::row::with_capacity(2)
            .push(
                widget::column::with_capacity(2)
                    .push(widget::text("Lesen").size(14))
                    .push(
                        widget::text(Self::format_rate_mib(read_now))
                            .size(24)
                            .class(theme::Text::Color(DISK_ACCENT)),
                    )
                    .spacing(2)
                    .width(Length::FillPortion(1)),
            )
            .push(
                widget::column::with_capacity(2)
                    .push(widget::text("Schreiben").size(14))
                    .push(
                        widget::text(Self::format_rate_mib(write_now))
                            .size(24)
                            .class(theme::Text::Color(DISK_ACCENT)),
                    )
                    .spacing(2)
                    .width(Length::FillPortion(1)),
            )
            .spacing(24)
            .width(Length::Fill);

        let extra_stats = widget::column::with_capacity(4)
            .push(widget::text(format!(
                "Systemdatenträger: {}",
                if is_system_disk { "Ja" } else { "Nein" }
            )))
            .push(widget::text(format!("Type: {kind_label}")))
            .push(widget::text(format!(
                "Aktive Zeit: {:.1}%",
                runtime_info.active_time_percent
            )))
            .push(widget::text(format!(
                "Antwortzeit (Durchschnitt): {:.1} ms",
                runtime_info.avg_response_ms
            )))
            .spacing(6)
            .width(Length::Fill);

        let usage_bar = widget::container(
            widget::row::with_capacity(2)
                .push(
                    widget::container(widget::text(""))
                        .class(theme::Container::custom(move |_theme| {
                            widget::container::Style {
                                background: Some(Background::Color(Color {
                                    a: 0.55,
                                    ..DISK_ACCENT
                                })),
                                ..Default::default()
                            }
                        }))
                        .height(Length::Fill)
                        .width(Length::FillPortion(used_portion.max(1))),
                )
                .push(widget::horizontal_space().width(Length::FillPortion(free_portion.max(1))))
                .height(Length::Fill)
                .width(Length::Fill),
        )
        .padding(2)
        .class(theme::Container::custom(move |_theme| {
            widget::container::Style {
                border: Border {
                    color: DISK_ACCENT,
                    width: 2.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            }
        }))
        .height(Length::Fixed(150.0))
        .width(Length::Fill);

        let usage_labels = widget::row::with_capacity(2)
            .push(
                widget::column::with_capacity(2)
                    .push(widget::text("Momentan belegt").size(13))
                    .push(
                        widget::text(Self::format_rss(used))
                            .size(20)
                            .class(theme::Text::Color(DISK_ACCENT)),
                    )
                    .spacing(4)
                    .width(Length::FillPortion(1)),
            )
            .push(widget::horizontal_space())
            .push(
                widget::column::with_capacity(2)
                    .push(widget::text("Maximal").size(13))
                    .push(
                        widget::text(Self::format_rss(total))
                            .size(20)
                            .class(theme::Text::Color(DISK_ACCENT)),
                    )
                    .align_x(Horizontal::Right)
                    .spacing(4)
                    .width(Length::FillPortion(1)),
            )
            .align_y(Alignment::End)
            .width(Length::Fill);

        let disk_actions: Element<'_, Message> = if is_mounted {
            if is_system_disk {
                widget::container(widget::text(
                    "Systemdatenträger kann nicht ausgehängt werden",
                ))
                .width(Length::Fill)
                .into()
            } else {
                widget::button::standard("Unmount")
                    .class(theme::Button::Suggested)
                    .on_press(Message::UnmountDisk(disk_name.to_string()))
                    .into()
            }
        } else {
            widget::button::standard("Mounten")
                .class(theme::Button::Suggested)
                .on_press(Message::MountDisk(disk_name.to_string()))
                .into()
        };

        let panel = widget::column::with_capacity(8)
            .push(
                widget::row::with_capacity(3)
                    .push(widget::text::title1(format!("Disk {disk_name}")))
                    .push(widget::horizontal_space())
                    .push(
                        widget::text(if is_mounted {
                            kind_label.clone()
                        } else {
                            format!("{kind_label} • Unmounted")
                        })
                        .size(14)
                        .class(theme::Text::Color(DISK_ACCENT)),
                    )
                    .align_y(Alignment::Center)
                    .width(Length::Fill),
            )
            .push(usage_bar)
            .push(usage_labels)
            .push(self.sparkline_solid(&read_history, DISK_ACCENT, 130.0))
            .push(self.sparkline_solid(
                &write_history,
                Color::from_rgb(158.0 / 255.0, 158.0 / 255.0, 54.0 / 255.0),
                130.0,
            ))
            .push(io_stats)
            .push(extra_stats)
            .push(widget::text("Partitionen").size(14))
            .push(partition_tiles)
            .push(widget::Space::with_height(Length::Fill))
            .push(widget::container(disk_actions).width(Length::Shrink))
            .height(Length::Fill)
            .spacing(space_s);

        widget::container(panel)
            .padding(18)
            .class(theme::Container::custom(|theme| widget::container::Style {
                background: Some(Background::Color(
                    theme.current_container().component.base.into(),
                )),
                border: Border {
                    color: DISK_ACCENT,
                    width: 1.0,
                    radius: 12.0.into(),
                },
                ..Default::default()
            }))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn cpu_detail_panel(&self, cpu_usage: f32, space_s: u16) -> Element<'_, Message> {
        let cores = self.system.cpus();
        let cpu_brand = cores.first().map_or("CPU", |cpu| cpu.brand());
        let avg_freq_mhz = if cores.is_empty() {
            0_u64
        } else {
            cores.iter().map(|cpu| cpu.frequency()).sum::<u64>() / cores.len() as u64
        };
        let current_speed_mhz = Self::read_current_cpu_speed_mhz().unwrap_or(avg_freq_mhz);
        let base_freq_mhz = cores.iter().map(|cpu| cpu.frequency()).max().unwrap_or(0);
        let process_count = self.system.processes().len();
        let thread_count = self
            .system
            .processes()
            .values()
            .map(|process| process.tasks().map_or(1_usize, |tasks| tasks.len().max(1)))
            .sum::<usize>();
        let logical_cores = cores.len();
        let uptime = Self::format_uptime(System::uptime());

        let core_grid = widget::responsive(move |size| {
            let min_tile_width = 200.0;
            let min_tile_height = 150.0;
            let max_tile_height = 260.0;
            let spacing = space_s as f32;
            let columns = (((size.width + spacing) / (min_tile_width + spacing)).floor() as usize)
                .clamp(1, 6);
            let row_count = (self.cpu_usage_history_per_core.len() + columns - 1) / columns;
            let available_height =
                (size.height - spacing * row_count.saturating_sub(1) as f32).max(min_tile_height);
            let tile_height = if row_count > 0 {
                (available_height / row_count as f32).clamp(min_tile_height, max_tile_height)
            } else {
                min_tile_height
            };
            let graph_height = (tile_height - 72.0).clamp(70.0, 160.0);

            let mut rows = widget::column::with_capacity(
                (self.cpu_usage_history_per_core.len() + columns - 1) / columns,
            )
            .spacing(space_s)
            .width(Length::Fill);

            for (row_index, chunk) in self.cpu_usage_history_per_core.chunks(columns).enumerate() {
                let mut row = widget::row::with_capacity(columns)
                    .spacing(space_s)
                    .width(Length::Fill);
                let base_index = row_index * columns;

                for (offset, history) in chunk.iter().enumerate() {
                    let index = base_index + offset;
                    let current_usage = cores.get(index).map_or(0.0, |core| core.cpu_usage());

                    let card = widget::container(
                        widget::column::with_capacity(3)
                            .push(widget::text(format!("Core {}", index + 1)).size(14))
                            .push(
                                widget::text(format!("{current_usage:.1}%"))
                                    .size(16)
                                    .class(theme::Text::Color(CPU_ACCENT)),
                            )
                            .push(self.sparkline(history, CPU_ACCENT, graph_height))
                            .spacing(6)
                            .width(Length::Fill),
                    )
                    .padding(10)
                    .class(theme::Container::custom(|theme| widget::container::Style {
                        background: Some(Background::Color(
                            theme.current_container().component.base.into(),
                        )),
                        border: Border {
                            color: theme.cosmic().bg_divider().into(),
                            width: 1.0,
                            radius: 8.0.into(),
                        },
                        ..Default::default()
                    }))
                    .width(Length::FillPortion(1))
                    .height(Length::Fixed(tile_height));

                    row = row.push(card);
                }

                rows = rows.push(row);
            }

            widget::scrollable(rows).height(Length::Fill).into()
        });

        let stat_block = |label: String, value: String, accent: bool| {
            let mut value_text = widget::text(value).size(26);
            if accent {
                value_text = value_text.class(theme::Text::Color(CPU_ACCENT));
            }

            widget::column::with_capacity(2)
                .push(widget::text(label).size(14))
                .push(value_text)
                .spacing(2)
                .width(Length::Fill)
        };

        let stats_row_1 = widget::row::with_capacity(2)
            .push(
                widget::container(stat_block(
                    "Last".to_string(),
                    format!("{cpu_usage:.0}%"),
                    true,
                ))
                .width(Length::Fixed(130.0)),
            )
            .push(
                widget::container(stat_block(
                    "Speed".to_string(),
                    format!("{} GHz", Self::format_ghz(current_speed_mhz)),
                    false,
                ))
                .width(Length::Fixed(130.0)),
            )
            .spacing(20)
            .width(Length::Shrink);

        let stats_row_2 = widget::row::with_capacity(2)
            .push(
                widget::container(stat_block(
                    "Processes".to_string(),
                    process_count.to_string(),
                    false,
                ))
                .width(Length::Fixed(130.0)),
            )
            .push(
                widget::container(stat_block(
                    "Threads".to_string(),
                    thread_count.to_string(),
                    false,
                ))
                .width(Length::Fixed(130.0)),
            )
            .spacing(20)
            .width(Length::Shrink);

        let stats_row_3 = widget::row::with_capacity(1)
            .push(
                widget::container(stat_block("Uptime".to_string(), uptime, false))
                    .width(Length::Fixed(130.0)),
            )
            .width(Length::Shrink);

        let stats_col_1 = widget::column::with_capacity(3)
            .push(stats_row_1)
            .push(stats_row_2)
            .push(stats_row_3)
            .spacing(8)
            .width(Length::Fixed(280.0));

        let right_line = |label: &str, value: String| {
            widget::row::with_capacity(2)
                .push(
                    widget::text(format!("{label}:"))
                        .size(16)
                        .width(Length::Fixed(120.0)),
                )
                .push(widget::text(value).size(16))
                .spacing(10)
                .width(Length::Shrink)
        };

        let stats_col_2 = widget::column::with_capacity(3)
            .push(right_line(
                "Base speed",
                format!("{} GHz", Self::format_ghz(base_freq_mhz)),
            ))
            .push(right_line("Cores", logical_cores.to_string()))
            .push(right_line(
                "Virtualization",
                self.cpu_static_info.virtualization.clone(),
            ))
            .push(right_line(
                "L1 Cache",
                self.cpu_static_info.l1_cache.clone(),
            ))
            .push(right_line(
                "L2 Cache",
                self.cpu_static_info.l2_cache.clone(),
            ))
            .push(right_line(
                "L3 Cache",
                self.cpu_static_info.l3_cache.clone(),
            ))
            .spacing(6)
            .width(Length::Fixed(340.0));

        let stats = widget::row::with_capacity(2)
            .push(stats_col_1)
            .push(stats_col_2)
            .spacing(35)
            .width(Length::Shrink);

        let panel = widget::column::with_capacity(6)
            .push(
                widget::row::with_capacity(3)
                    .push(widget::text::title1("CPU"))
                    .push(widget::horizontal_space())
                    .push(
                        widget::text(cpu_brand)
                            .size(14)
                            .class(theme::Text::Color(CPU_ACCENT)),
                    )
                    .align_y(Alignment::Center)
                    .width(Length::Fill),
            )
            .push(widget::text("% Auslastung uber 60 Sekunden").size(14))
            .push(core_grid)
            .push(widget::Space::with_height(Length::Fixed(50.0)))
            .push(widget::container(stats).width(Length::Shrink))
            .spacing(space_s);

        widget::container(panel)
            .padding(18)
            .class(theme::Container::custom(|theme| widget::container::Style {
                background: Some(Background::Color(
                    theme.current_container().component.base.into(),
                )),
                border: Border {
                    color: CPU_ACCENT,
                    width: 1.0,
                    radius: 12.0.into(),
                },
                ..Default::default()
            }))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn ram_detail_panel(
        &self,
        used_memory: u64,
        total_memory: u64,
        _ram_usage: f32,
        space_s: u16,
    ) -> Element<'_, Message> {
        let available_memory = self.system.available_memory();
        let cached_memory = self.system.free_memory();
        let used_swap = self.system.used_swap();
        let total_swap = self.system.total_swap();

        let stat_block = |label: String, value: String, accent: bool| {
            let mut value_text = widget::text(value).size(26);
            if accent {
                value_text = value_text.class(theme::Text::Color(RAM_ACCENT));
            }

            widget::column::with_capacity(2)
                .push(widget::text(label).size(14))
                .push(value_text)
                .spacing(2)
                .width(Length::Fill)
        };

        let stats_row_1 = widget::row::with_capacity(2)
            .push(
                widget::container(stat_block(
                    "In use".to_string(),
                    Self::format_rss(used_memory),
                    true,
                ))
                .width(Length::Fixed(180.0)),
            )
            .push(
                widget::container(stat_block(
                    "Available".to_string(),
                    Self::format_rss(available_memory),
                    false,
                ))
                .width(Length::Fixed(180.0)),
            )
            .spacing(20)
            .width(Length::Shrink);

        let stats_row_2 = widget::row::with_capacity(2)
            .push(
                widget::container(stat_block(
                    "Cached".to_string(),
                    Self::format_rss(cached_memory),
                    false,
                ))
                .width(Length::Fixed(180.0)),
            )
            .push(
                widget::container(stat_block(
                    "Swap used".to_string(),
                    if total_swap > 0 {
                        format!(
                            "{} / {}",
                            Self::format_rss(used_swap),
                            Self::format_rss(total_swap)
                        )
                    } else {
                        "N/A".to_string()
                    },
                    false,
                ))
                .width(Length::Fixed(180.0)),
            )
            .spacing(20)
            .width(Length::Shrink);

        let stats_col_1 = widget::column::with_capacity(2)
            .push(stats_row_1)
            .push(stats_row_2)
            .spacing(8)
            .width(Length::Fixed(380.0));

        let stats = widget::row::with_capacity(1)
            .push(stats_col_1)
            .width(Length::Shrink);

        let panel = widget::column::with_capacity(7)
            .push(
                widget::row::with_capacity(3)
                    .push(widget::text::title1("Memory"))
                    .push(widget::horizontal_space())
                    .push(
                        widget::text(Self::format_rss(total_memory))
                            .size(16)
                            .class(theme::Text::Color(RAM_ACCENT)),
                    )
                    .align_y(Alignment::Center)
                    .width(Length::Fill),
            )
            .push(widget::text("Speicherauslastung").size(14))
            .push(self.sparkline_solid(&self.ram_usage_history, RAM_ACCENT, 240.0))
            .push(
                widget::row::with_capacity(3)
                    .push(
                        widget::column::with_capacity(2)
                            .push(widget::text("Momentan").size(14))
                            .push(
                                widget::text(Self::format_rss(used_memory))
                                    .size(20)
                                    .class(theme::Text::Color(RAM_ACCENT)),
                            )
                            .spacing(2),
                    )
                    .push(widget::horizontal_space())
                    .push(
                        widget::column::with_capacity(2)
                            .push(widget::text("Maximal").size(14))
                            .push(
                                widget::text(Self::format_rss(total_memory))
                                    .size(20)
                                    .class(theme::Text::Color(RAM_ACCENT)),
                            )
                            .spacing(2)
                            .align_x(Horizontal::Right),
                    )
                    .align_y(Alignment::End)
                    .width(Length::Fill),
            )
            .push(widget::Space::with_height(Length::Fixed(50.0)))
            .push(widget::container(stats).width(Length::Shrink))
            .spacing(space_s);

        widget::container(panel)
            .padding(18)
            .class(theme::Container::custom(|theme| widget::container::Style {
                background: Some(Background::Color(
                    theme.current_container().component.base.into(),
                )),
                border: Border {
                    color: RAM_ACCENT,
                    width: 1.0,
                    radius: 12.0.into(),
                },
                ..Default::default()
            }))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn sparkline_solid(&self, samples: &[f32], accent: Color, height: f32) -> Element<'_, Message> {
        let mut bars = widget::row::with_capacity(samples.len().max(1))
            .spacing(0)
            .height(Length::Fixed(height))
            .width(Length::Fill)
            .align_y(Alignment::End);

        if samples.is_empty() {
            bars = bars.push(
                widget::container(widget::text(""))
                    .width(Length::Fill)
                    .height(Length::Fill),
            );
        } else {
            for sample in samples {
                let clamped = sample.clamp(0.0, 100.0);
                let bar_height = ((clamped / 100.0) * height).max(1.0);
                let top_space = (height - bar_height).max(0.0);
                bars = bars.push(
                    widget::container(
                        widget::column::with_capacity(2)
                            .push(widget::Space::with_height(Length::Fixed(top_space)))
                            .push(
                                widget::container(widget::text(""))
                                    .class(theme::Container::custom(move |_theme| {
                                        widget::container::Style {
                                            background: Some(Background::Color(Color {
                                                a: 0.78,
                                                ..accent
                                            })),
                                            ..Default::default()
                                        }
                                    }))
                                    .height(Length::Fixed(bar_height))
                                    .width(Length::Fill),
                            )
                            .spacing(0),
                    )
                    .width(Length::FillPortion(1))
                    .height(Length::Fill),
                );
            }
        }

        widget::container(bars)
            .padding(0)
            .class(theme::Container::custom(|theme| widget::container::Style {
                background: Some(Background::Color(
                    theme.current_container().component.base.into(),
                )),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            }))
            .width(Length::Fill)
            .height(Length::Fixed(height))
            .into()
    }

    fn sparkline(&self, samples: &[f32], accent: Color, height: f32) -> Element<'_, Message> {
        let mut bars = widget::row::with_capacity(samples.len().max(1))
            .spacing(1)
            .height(Length::Fixed(height))
            .width(Length::Fill)
            .align_y(Alignment::End);

        if samples.is_empty() {
            bars = bars.push(
                widget::container(widget::text(""))
                    .width(Length::Fill)
                    .height(Length::Fill),
            );
        } else {
            for sample in samples {
                let clamped = sample.clamp(0.0, 100.0);
                let bar_height = ((clamped / 100.0) * height).max(1.0);
                let top_space = (height - bar_height).max(0.0);
                bars = bars.push(
                    widget::container(
                        widget::column::with_capacity(2)
                            .push(widget::Space::with_height(Length::Fixed(top_space)))
                            .push(
                                widget::container(widget::text(""))
                                    .class(theme::Container::custom(move |_theme| {
                                        widget::container::Style {
                                            background: Some(Background::Color(Color {
                                                a: 0.75,
                                                ..accent
                                            })),
                                            ..Default::default()
                                        }
                                    }))
                                    .height(Length::Fixed(bar_height))
                                    .width(Length::Fill),
                            )
                            .spacing(0),
                    )
                    .width(Length::FillPortion(1))
                    .height(Length::Fill),
                );
            }
        }

        widget::container(bars)
            .padding(8)
            .class(theme::Container::custom(|theme| widget::container::Style {
                background: Some(Background::Color(
                    theme.current_container().component.base.into(),
                )),
                border: Border {
                    color: theme.cosmic().bg_divider().into(),
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            }))
            .width(Length::Fill)
            .height(Length::Fixed(height + 16.0))
            .into()
    }

    fn format_ghz(mhz: u64) -> String {
        format!("{:.2}", mhz as f32 / 1000.0).replace('.', ",")
    }

    fn format_rate_mib(rate_mib_s: f32) -> String {
        format!("{rate_mib_s:.2} MiB/s").replace('.', ",")
    }

    fn format_uptime(total_seconds: u64) -> String {
        let days = total_seconds / 86_400;
        let hours = (total_seconds % 86_400) / 3_600;
        let minutes = (total_seconds % 3_600) / 60;
        let seconds = total_seconds % 60;
        format!("{days}:{hours:02}:{minutes:02}:{seconds:02}")
    }

    fn read_cpu_static_info() -> CpuStaticInfo {
        let mut info = CpuStaticInfo::default();
        let Ok(output) = Command::new("lscpu").stdout(Stdio::piped()).output() else {
            return info;
        };
        if !output.status.success() {
            return info;
        }

        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let Some((raw_key, raw_value)) = line.split_once(':') else {
                continue;
            };
            let key = raw_key.trim().to_ascii_lowercase();
            let value = raw_value.trim();
            if value.is_empty() {
                continue;
            }

            let cleaned = value.split(" (").next().unwrap_or(value).trim().to_string();
            match key.as_str() {
                "socket(s)" => info.sockets = cleaned,
                "virtualization" => info.virtualization = format!("Enabled ({cleaned})"),
                "virtualization type" if info.virtualization == "N/A" => {
                    info.virtualization = cleaned;
                }
                "l1 cache" => info.l1_cache = cleaned,
                "l1d cache" if info.l1_cache == "N/A" => info.l1_cache = cleaned,
                "l2 cache" => info.l2_cache = cleaned,
                "l3 cache" => info.l3_cache = cleaned,
                _ => {}
            }
        }

        if info.virtualization == "N/A" {
            if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
                if cpuinfo.contains(" vmx ")
                    || cpuinfo.contains("\nflags\t:") && cpuinfo.contains(" vmx")
                {
                    info.virtualization = "Enabled (VT-x)".to_string();
                } else if cpuinfo.contains(" svm ")
                    || cpuinfo.contains("\nflags\t:") && cpuinfo.contains(" svm")
                {
                    info.virtualization = "Enabled (AMD-V)".to_string();
                } else if cpuinfo.contains("flags") {
                    info.virtualization = "Disabled".to_string();
                }
            }
        }

        info
    }

    fn read_current_cpu_speed_mhz() -> Option<u64> {
        let Ok(entries) = fs::read_dir("/sys/devices/system/cpu") else {
            return None;
        };

        let mut total_mhz = 0_u64;
        let mut count = 0_u64;

        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !name.starts_with("cpu")
                || name.len() <= 3
                || !name[3..].chars().all(|c| c.is_ascii_digit())
            {
                continue;
            }

            let base = entry.path().join("cpufreq");
            let freq_paths = [base.join("scaling_cur_freq"), base.join("cpuinfo_cur_freq")];
            for path in freq_paths {
                let Ok(raw) = fs::read_to_string(&path) else {
                    continue;
                };
                let Ok(value) = raw.trim().parse::<u64>() else {
                    continue;
                };
                if value == 0 {
                    continue;
                }

                // cpufreq values are typically in kHz, convert to MHz.
                total_mhz += value / 1000;
                count += 1;
                break;
            }
        }

        if count == 0 {
            None
        } else {
            Some(total_mhz / count)
        }
    }

    fn disk_device_key(partition_name: &str) -> String {
        let partition_name = Self::normalize_block_name(partition_name);
        // Linux partition naming: sda1 -> sda, nvme0n1p2 -> nvme0n1, mmcblk0p1 -> mmcblk0.
        if let Some(idx) = partition_name.rfind('p') {
            let suffix = &partition_name[idx + 1..];
            if !suffix.is_empty()
                && suffix.chars().all(|c| c.is_ascii_digit())
                && partition_name[..idx].chars().any(|c| c.is_ascii_digit())
            {
                return partition_name[..idx].to_string();
            }
        }

        // Classic disk names where trailing digits indicate partitions.
        let is_classic_partition = (partition_name.starts_with("sd")
            || partition_name.starts_with("vd")
            || partition_name.starts_with("xvd")
            || partition_name.starts_with("hd"))
            && partition_name.chars().any(|c| c.is_ascii_alphabetic())
            && partition_name.chars().any(|c| c.is_ascii_digit());

        if is_classic_partition {
            partition_name
                .trim_end_matches(|c: char| c.is_ascii_digit())
                .to_string()
        } else {
            partition_name.to_string()
        }
    }

    fn normalize_block_name(name: &str) -> &str {
        name.trim()
            .trim_start_matches("/dev/")
            .trim_start_matches("./")
    }

    fn mount_disk(&self, disk_name: &str) {
        let entries = self.disk_block_entries(disk_name);
        let mut mounted_any = false;

        for entry in entries
            .iter()
            .filter(|entry| entry.block_type == "part" && entry.mountpoint.trim().is_empty())
        {
            if Self::run_udisksctl("mount", &entry.name) {
                mounted_any = true;
                break;
            }
        }

        if !mounted_any {
            for entry in entries
                .iter()
                .filter(|entry| entry.block_type == "disk" && entry.mountpoint.trim().is_empty())
            {
                if Self::run_udisksctl("mount", &entry.name) {
                    mounted_any = true;
                    break;
                }
            }
        }

        if !mounted_any {
            eprintln!("no mountable block device found for disk {disk_name}");
        }
    }

    fn unmount_disk(&self, disk_name: &str) {
        let entries = self.disk_block_entries(disk_name);
        let mut unmounted_any = false;

        for entry in entries.iter().filter(|entry| {
            entry.block_type == "part"
                && !entry.mountpoint.trim().is_empty()
                && !entry.mountpoint.trim().starts_with('[')
        }) {
            if Self::run_udisksctl("unmount", &entry.name) {
                unmounted_any = true;
            }
        }

        if !unmounted_any {
            for entry in entries.iter().filter(|entry| {
                entry.block_type == "disk"
                    && !entry.mountpoint.trim().is_empty()
                    && !entry.mountpoint.trim().starts_with('[')
            }) {
                if Self::run_udisksctl("unmount", &entry.name) {
                    unmounted_any = true;
                }
            }
        }

        if !unmounted_any {
            eprintln!("no mounted block device found for disk {disk_name}");
        }
    }

    fn run_udisksctl(action: &str, block_name: &str) -> bool {
        let device = format!("/dev/{}", Self::canonical_block_name(block_name));
        match Command::new("udisksctl")
            .args([action, "-b", device.as_str()])
            .status()
        {
            Ok(status) => status.success(),
            Err(err) => {
                eprintln!("failed to run udisksctl {action} for {device}: {err}");
                false
            }
        }
    }

    fn disk_block_entries(&self, disk_name: &str) -> Vec<DiskBlockEntry> {
        let mut parent_by_name: HashMap<String, String> = HashMap::new();
        let mut raw_rows: Vec<DiskBlockEntry> = Vec::new();

        let Ok(output) = Command::new("lsblk")
            .args(["-P", "-o", "NAME,TYPE,PKNAME,MOUNTPOINT"])
            .stdout(Stdio::piped())
            .output()
        else {
            return Vec::new();
        };
        if !output.status.success() {
            return Vec::new();
        }

        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let fields = Self::parse_lsblk_pairs(line);
            let name = fields.get("NAME").cloned().unwrap_or_default();
            let block_type = fields.get("TYPE").cloned().unwrap_or_default();
            let pkname = fields.get("PKNAME").cloned().unwrap_or_default();
            let mountpoint = fields.get("MOUNTPOINT").cloned().unwrap_or_default();

            if name.is_empty() {
                continue;
            }

            if !pkname.is_empty() {
                parent_by_name.insert(name.clone(), pkname);
            }

            raw_rows.push(DiskBlockEntry {
                name,
                block_type,
                mountpoint,
            });
        }

        raw_rows
            .into_iter()
            .filter(|entry| entry.block_type == "part" || entry.block_type == "disk")
            .filter(|entry| {
                let ancestor = Self::resolve_disk_ancestor(&entry.name, &parent_by_name);
                Self::disk_device_key(&ancestor) == disk_name
            })
            .collect()
    }

    fn collect_disk_groups(&self) -> Vec<DiskGroupInfo> {
        #[derive(Default)]
        struct TempDisk {
            total_bytes: u64,
            kind_label: String,
            partitions: Vec<String>,
            is_mounted: bool,
            is_system_disk: bool,
        }

        let mut mounted_usage: HashMap<String, (u64, u64)> = HashMap::new();
        for disk in self.disks.list() {
            let partition_name = disk.name().to_string_lossy().to_string();
            let key = Self::disk_device_key(&partition_name);
            let total = disk.total_space();
            let used = total.saturating_sub(disk.available_space());
            let entry = mounted_usage.entry(key).or_insert((0, 0));
            // A disk can appear multiple times (bind mounts/subvolumes), so avoid summing duplicates.
            entry.0 = entry.0.max(total);
            entry.1 = entry.1.max(used);
        }

        let mut by_disk: HashMap<String, TempDisk> = HashMap::new();
        let mut parent_by_name: HashMap<String, String> = HashMap::new();
        let mut root_mount_devices: Vec<String> = Vec::new();
        if let Ok(output) = Command::new("lsblk")
            .args([
                "-b",
                "-P",
                "-o",
                "NAME,TYPE,PKNAME,SIZE,LABEL,MOUNTPOINT,ROTA,TRAN",
            ])
            .stdout(Stdio::piped())
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                for line in text.lines() {
                    let fields = Self::parse_lsblk_pairs(line);
                    let name = fields.get("NAME").cloned().unwrap_or_default();
                    let ty = fields.get("TYPE").cloned().unwrap_or_default();
                    let pkname = fields.get("PKNAME").cloned().unwrap_or_default();
                    let size = fields
                        .get("SIZE")
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(0);
                    let label = fields.get("LABEL").cloned().unwrap_or_default();
                    let mountpoint = fields.get("MOUNTPOINT").cloned().unwrap_or_default();
                    let rota = fields.get("ROTA").cloned().unwrap_or_default();
                    let transport = fields.get("TRAN").cloned().unwrap_or_default();

                    if !pkname.is_empty() {
                        parent_by_name.insert(name.clone(), pkname.clone());
                    }
                    if mountpoint == "/" {
                        root_mount_devices.push(name.clone());
                    }

                    if ty == "disk" {
                        if name.starts_with("loop")
                            || name.starts_with("ram")
                            || name.starts_with("zram")
                            || name.starts_with("dm-")
                        {
                            continue;
                        }
                        let entry = by_disk.entry(name.clone()).or_default();
                        entry.total_bytes = size;
                        entry.kind_label = Self::disk_type_label(&name, &rota, &transport);
                        if !mountpoint.is_empty() {
                            entry.is_mounted = true;
                        }
                        if mountpoint == "/" {
                            entry.is_system_disk = true;
                        }
                    } else if ty == "part" {
                        let parent = if !pkname.is_empty() {
                            Self::disk_device_key(&pkname)
                        } else {
                            Self::disk_device_key(&name)
                        };
                        let entry = by_disk.entry(parent.clone()).or_default();
                        let partition_display =
                            Self::partition_display_name(&name, &label, &mountpoint);
                        if !entry.partitions.iter().any(|existing| {
                            existing.ends_with(&format!("({name})")) || existing == &name
                        }) {
                            entry.partitions.push(partition_display);
                        }
                        if !mountpoint.is_empty() {
                            entry.is_mounted = true;
                        }
                        if mountpoint == "/" {
                            entry.is_system_disk = true;
                        }
                    } else if !mountpoint.is_empty() {
                        let resolved = Self::resolve_disk_ancestor(&name, &parent_by_name);
                        let parent = Self::disk_device_key(&resolved);
                        let entry = by_disk.entry(parent).or_default();
                        entry.is_mounted = true;
                    }
                }
            }
        }

        for root_name in root_mount_devices {
            let resolved = Self::resolve_disk_ancestor(&root_name, &parent_by_name);
            let key = Self::disk_device_key(&resolved);
            by_disk.entry(key).or_default().is_system_disk = true;
        }
        for source in Self::findmnt_sources_for_system_disk() {
            let resolved = Self::resolve_disk_ancestor(&source, &parent_by_name);
            let key = Self::disk_device_key(&resolved);
            by_disk.entry(key).or_default().is_system_disk = true;
        }

        let groups = by_disk
            .into_iter()
            .map(|(name, mut temp)| {
                let (mounted_total, mounted_used) = mounted_usage.remove(&name).unwrap_or((0, 0));
                // Prefer mounted totals when available; otherwise keep lsblk disk size
                // so unmounted primary disks are still visible in the sidebar.
                if mounted_total > 0 {
                    temp.total_bytes = mounted_total;
                }
                if temp.kind_label.is_empty() {
                    temp.kind_label = Self::disk_type_label(&name, "", "");
                }
                if temp.partitions.is_empty() {
                    temp.partitions.push(name.clone());
                } else {
                    temp.partitions.sort();
                }
                DiskGroupInfo {
                    name,
                    total_bytes: temp.total_bytes,
                    used_bytes: if temp.is_mounted && mounted_total > 0 {
                        mounted_used
                    } else {
                        0
                    },
                    kind_label: temp.kind_label,
                    partitions: temp.partitions,
                    is_mounted: temp.is_mounted,
                    is_system_disk: temp.is_system_disk,
                }
            })
            .filter(|disk| disk.total_bytes > 0)
            .collect::<Vec<_>>();

        groups
    }

    fn parse_lsblk_pairs(line: &str) -> HashMap<String, String> {
        let mut out = HashMap::new();
        let mut i = 0usize;
        let bytes = line.as_bytes();
        while i < bytes.len() {
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }

            let key_start = i;
            while i < bytes.len() && bytes[i] != b'=' {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }
            let key = line[key_start..i].trim().to_string();
            i += 1;

            let value = if i < bytes.len() && bytes[i] == b'"' {
                i += 1;
                let value_start = i;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += 1;
                }
                let v = line[value_start..i].to_string();
                if i < bytes.len() && bytes[i] == b'"' {
                    i += 1;
                }
                v
            } else {
                let value_start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                line[value_start..i].to_string()
            };

            out.insert(key, value);
        }
        out
    }

    fn resolve_disk_ancestor(name: &str, parent_by_name: &HashMap<String, String>) -> String {
        let mut current = Self::canonical_block_name(name);
        let mut guard = 0usize;
        while let Some(parent) = parent_by_name.get(&current) {
            current = parent.clone();
            guard += 1;
            if guard > 32 {
                break;
            }
        }
        current
    }

    fn canonical_block_name(name: &str) -> String {
        let value = Self::normalize_block_name(name);
        value.trim_start_matches("mapper/").to_string()
    }

    fn findmnt_sources_for_system_disk() -> Vec<String> {
        let mut out = Vec::new();
        for target in ["/", "/home", "/boot", "/boot/efi"] {
            let Ok(result) = Command::new("findmnt")
                .args(["-n", "-o", "SOURCE", target])
                .stdout(Stdio::piped())
                .output()
            else {
                continue;
            };
            if !result.status.success() {
                continue;
            }
            let source = String::from_utf8_lossy(&result.stdout).trim().to_string();
            if source.is_empty() || source == "overlay" || source == "none" {
                continue;
            }
            let canonical = Self::canonical_block_name(&source);
            if !canonical.is_empty() && !out.iter().any(|value| value == &canonical) {
                out.push(canonical);
            }
        }
        out
    }

    fn partition_display_name(name: &str, label: &str, mountpoint: &str) -> String {
        let clean_label = label.trim();
        if !clean_label.is_empty() {
            return format!("{clean_label} ({name})");
        }
        let clean_mount = mountpoint.trim();
        if !clean_mount.is_empty() && !clean_mount.starts_with('[') {
            let fallback = Path::new(clean_mount)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(clean_mount);
            if !fallback.trim().is_empty() {
                return format!("{fallback} ({name})");
            }
        }
        name.to_string()
    }

    fn disk_kind_from_name_and_rota(name: &str, rota: &str) -> String {
        match rota {
            "0" => return "SSD".to_string(),
            "1" => return "HDD".to_string(),
            _ => {}
        }

        if name.starts_with("nvme") {
            return "SSD".to_string();
        }

        let path = format!("/sys/block/{name}/queue/rotational");
        if let Ok(value) = fs::read_to_string(path) {
            match value.trim() {
                "0" => return "SSD".to_string(),
                "1" => return "HDD".to_string(),
                _ => {}
            }
        }

        "Unknown".to_string()
    }

    fn disk_type_label(name: &str, rota: &str, transport: &str) -> String {
        let kind = Self::disk_kind_from_name_and_rota(name, rota);
        let bus = Self::disk_bus_from_name_and_transport(name, transport);
        if bus == "Unknown" {
            kind
        } else {
            format!("{kind} ({bus})")
        }
    }

    fn disk_bus_from_name_and_transport(name: &str, transport: &str) -> String {
        let transport = transport.trim().to_ascii_lowercase();
        if !transport.is_empty() {
            return match transport.as_str() {
                "nvme" => "NVMe".to_string(),
                "sata" | "ata" => "SATA".to_string(),
                "sas" => "SAS".to_string(),
                "usb" => "USB".to_string(),
                "scsi" => "SCSI".to_string(),
                other => other.to_ascii_uppercase(),
            };
        }

        if name.starts_with("nvme") {
            return "NVMe".to_string();
        }

        let subsystem_path = format!("/sys/block/{name}/device/subsystem");
        if let Ok(link) = fs::read_link(subsystem_path) {
            if let Some(raw) = link.file_name().and_then(|value| value.to_str()) {
                return match raw {
                    "nvme" => "NVMe".to_string(),
                    "ata" => "SATA".to_string(),
                    "scsi" => "SCSI".to_string(),
                    other => other.to_ascii_uppercase(),
                };
            }
        }

        "Unknown".to_string()
    }

    fn list_primary_disks() -> Vec<String> {
        let Ok(entries) = fs::read_dir("/sys/block") else {
            return Vec::new();
        };

        entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| {
                !(name.starts_with("loop")
                    || name.starts_with("ram")
                    || name.starts_with("zram")
                    || name.starts_with("dm-"))
            })
            .collect()
    }

    fn read_disk_io_snapshot(disk_name: &str) -> Option<DiskIoSnapshot> {
        let path = format!("/sys/block/{disk_name}/stat");
        let raw = fs::read_to_string(path).ok()?;
        let numbers = raw
            .split_whitespace()
            .filter_map(|token| token.parse::<u64>().ok())
            .collect::<Vec<_>>();

        if numbers.len() < 11 {
            return None;
        }

        Some(DiskIoSnapshot {
            reads_completed: numbers[0],
            writes_completed: numbers[4],
            io_time_ms: numbers[9],
            weighted_io_time_ms: numbers[10],
        })
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
    ProcessActions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    About,
    ViewList,
    ViewTile,
}

impl menu::action::MenuAction for MenuAction {
    type Message = Message;

    fn message(&self) -> Self::Message {
        match self {
            MenuAction::About => Message::ToggleContextPage(ContextPage::About),
            MenuAction::ViewList => Message::SetAppsViewMode(AppsViewMode::List),
            MenuAction::ViewTile => Message::SetAppsViewMode(AppsViewMode::Tile),
        }
    }
}
