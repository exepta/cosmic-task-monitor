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
const APP_ICON: &[u8] = include_bytes!(
    "../resources/icons/hicolor/scalable/apps/com.github.exepta.cosmic-task-monitor.svg"
);
const PROCESS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const PERFORMANCE_HISTORY_POINTS: usize = 60;
const CPU_ACCENT: Color = Color::from_rgb(155.0 / 255.0, 88.0 / 255.0, 180.0 / 255.0);
const RAM_ACCENT: Color = Color::from_rgb(126.0 / 255.0, 189.0 / 255.0, 195.0 / 255.0);
const GPU_ACCENT: Color = Color::from_rgb(231.0 / 255.0, 141.0 / 255.0, 56.0 / 255.0);
const NETWORK_ACCENT: Color = Color::from_rgb(81.0 / 255.0, 150.0 / 255.0, 214.0 / 255.0);
const DISK_ACCENT: Color = Color::from_rgb(197.0 / 255.0, 196.0 / 255.0, 67.0 / 255.0);

mod apps;
mod autostart;
mod process;
mod steam_helper;
mod system_stats;

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

fn section_toggle_button_style() -> theme::Button {
    theme::Button::Custom {
        active: Box::new(|_focused, _theme| {
            let mut style = widget::button::Style::new();
            style.border_width = 0.0;
            style.border_color = Color::TRANSPARENT;
            style.border_radius = 0.0.into();
            style
        }),
        hovered: Box::new(|_focused, theme| {
            let mut style = widget::button::Style::new();
            style.background = Some(Background::Color(
                theme.current_container().component.hover.into(),
            ));
            style.border_width = 0.0;
            style.border_color = Color::TRANSPARENT;
            style.border_radius = 0.0.into();
            style
        }),
        pressed: Box::new(|_focused, theme| {
            let mut style = widget::button::Style::new();
            style.background = Some(Background::Color(
                theme.current_container().component.hover.into(),
            ));
            style.border_width = 0.0;
            style.border_color = Color::TRANSPARENT;
            style.border_radius = 0.0.into();
            style
        }),
        disabled: Box::new(|_theme| {
            let mut style = widget::button::Style::new();
            style.border_width = 0.0;
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
    is_background: bool,
    icon_handle: Option<icon::Handle>,
    pid: u32,
    cpu_percent: f32,
    rss_bytes: u64,
    threads: u32,
}

#[derive(Debug, Clone)]
struct AutostartEntry {
    app_id: String,
    desktop_file_name: String,
    autostart_path: String,
    name: String,
    exec: String,
    is_background: bool,
    icon_handle: Option<icon::Handle>,
}

#[derive(Debug, Clone)]
struct AutostartAddOption {
    app_id: String,
    desktop_entry_id: Option<String>,
    name: String,
    exec: Option<String>,
    desktop_entry_path: Option<PathBuf>,
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
struct GpuRuntimeInfo {
    name: String,
    provider: String,
    driver: String,
    utilization_percent: Option<f32>,
    temperature_celsius: Option<f32>,
    vram_used_bytes: Option<u64>,
    vram_total_bytes: Option<u64>,
    current_clock_mhz: Option<u64>,
    max_clock_mhz: Option<u64>,
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
struct NetworkInterfaceInfo {
    name: String,
    is_wireless: bool,
    speed_mbps: Option<u64>,
    rx_bytes: u64,
    tx_bytes: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct NetworkIoSnapshot {
    rx_bytes: u64,
    tx_bytes: u64,
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

impl Default for GpuRuntimeInfo {
    fn default() -> Self {
        Self {
            name: "Unknown GPU".to_string(),
            provider: "Unknown".to_string(),
            driver: "Unknown".to_string(),
            utilization_percent: None,
            temperature_celsius: None,
            vram_used_bytes: None,
            vram_total_bytes: None,
            current_clock_mhz: None,
            max_clock_mhz: None,
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
    Gpu,
    Network(String),
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
    apps_desktop_expanded: bool,
    apps_background_expanded: bool,
    autostart_entries: Vec<AutostartEntry>,
    autostart_add_options: Vec<AutostartAddOption>,
    autostart_modal_open: bool,
    autostart_modal_selected_option: Option<usize>,
    autostart_desktop_expanded: bool,
    autostart_background_expanded: bool,
    performance_view_mode: PerformanceViewMode,
    cpu_usage_history_per_core: Vec<Vec<f32>>,
    ram_usage_history: Vec<f32>,
    gpu_usage_history: Vec<f32>,
    gpu_vram_usage_history: Vec<f32>,
    network_interfaces: Vec<NetworkInterfaceInfo>,
    network_rx_history: HashMap<String, Vec<f32>>,
    network_tx_history: HashMap<String, Vec<f32>>,
    network_previous_snapshots: HashMap<String, NetworkIoSnapshot>,
    disk_read_history: HashMap<String, Vec<f32>>,
    disk_write_history: HashMap<String, Vec<f32>>,
    disk_runtime_info: HashMap<String, DiskRuntimeInfo>,
    disk_previous_snapshots: HashMap<String, DiskIoSnapshot>,
    cpu_static_info: CpuStaticInfo,
    gpu_runtime_info: GpuRuntimeInfo,
    sort_state: SortState,
}

#[derive(Debug, Clone)]
pub enum Message {
    LaunchUrl(String),
    ToggleContextPage(ContextPage),
    UpdateConfig(Config),
    RefreshProcesses,
    SetAppsViewMode(AppsViewMode),
    ToggleAppsDesktopSection,
    ToggleAppsBackgroundSection,
    OpenAutostartModal,
    CloseAutostartModal,
    SelectAutostartModalOption(usize),
    ConfirmAutostartModal,
    ToggleAutostartDesktopSection,
    ToggleAutostartBackgroundSection,
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
            .text(fl!("nav-autostart"))
            .data::<Page>(Page::Page2)
            .icon(icon::from_name("system-run-symbolic"));

        nav.insert()
            .text(fl!("nav-performance"))
            .data::<Page>(Page::Page3)
            .icon(icon::from_name("utilities-system-monitor-symbolic"));

        let about = About::default()
            .name(fl!("app-title"))
            .icon(icon::from_svg_bytes(APP_ICON))
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
                .map(|context| {
                    Config::get_entry(&context).unwrap_or_else(|(_errors, config)| config)
                })
                .unwrap_or_default(),
            system: System::new_all(),
            disks: Disks::new_with_refreshed_list(),
            desktop_apps_by_exec: Self::load_desktop_app_map(),
            steam_apps_by_id: HashMap::new(),
            process_entries: Vec::new(),
            selected_process: None,
            apps_view_mode: AppsViewMode::List,
            apps_desktop_expanded: true,
            apps_background_expanded: false,
            autostart_entries: Vec::new(),
            autostart_add_options: Vec::new(),
            autostart_modal_open: false,
            autostart_modal_selected_option: None,
            autostart_desktop_expanded: true,
            autostart_background_expanded: false,
            performance_view_mode: PerformanceViewMode::Cpu,
            cpu_usage_history_per_core: Vec::new(),
            ram_usage_history: Vec::new(),
            gpu_usage_history: Vec::new(),
            gpu_vram_usage_history: Vec::new(),
            network_interfaces: Vec::new(),
            network_rx_history: HashMap::new(),
            network_tx_history: HashMap::new(),
            network_previous_snapshots: HashMap::new(),
            disk_read_history: HashMap::new(),
            disk_write_history: HashMap::new(),
            disk_runtime_info: HashMap::new(),
            disk_previous_snapshots: HashMap::new(),
            cpu_static_info: Self::read_cpu_static_info(),
            gpu_runtime_info: GpuRuntimeInfo::default(),
            sort_state: SortState {
                column: SortColumn::Ram,
                direction: SortDirection::Desc,
            },
        };

        app.refresh_autostart_state();
        let command = app.update_title();
        (app, command)
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

                let padded_content = widget::container(content).padding([0, 20, 0, 0]);
                context_drawer::context_drawer(padded_content, Message::CloseProcessMenu)
                    .title(title)
            }
        })
    }

    fn dialog(&self) -> Option<Element<'_, Self::Message>> {
        self.autostart_add_dialog()
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

    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<cosmic::Action<Self::Message>> {
        self.nav.activate(id);
        self.update_title()
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
            Message::ToggleAppsDesktopSection => {
                self.apps_desktop_expanded = !self.apps_desktop_expanded;
            }
            Message::ToggleAppsBackgroundSection => {
                self.apps_background_expanded = !self.apps_background_expanded;
            }
            Message::OpenAutostartModal => self.open_autostart_modal(),
            Message::CloseAutostartModal => self.autostart_modal_open = false,
            Message::SelectAutostartModalOption(index) => {
                self.autostart_modal_selected_option = Some(index);
            }
            Message::ConfirmAutostartModal => {
                self.confirm_autostart_modal();
            }
            Message::ToggleAutostartDesktopSection => {
                self.autostart_desktop_expanded = !self.autostart_desktop_expanded;
            }
            Message::ToggleAutostartBackgroundSection => {
                self.autostart_background_expanded = !self.autostart_background_expanded;
            }
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

    fn view(&self) -> Element<'_, Self::Message> {
        let space_s = theme::spacing().space_s;
        let content: Element<_> = match self.nav.active_data::<Page>().unwrap() {
            Page::Page1 => self.apps_view(space_s),
            Page::Page2 => self.autostart_view(space_s),
            Page::Page3 => self.performance_view(space_s),
        };

        widget::container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

impl AppModel {
    fn format_ghz(mhz: u64) -> String {
        format!("{:.2}", mhz as f32 / 1000.0).replace('.', ",")
    }

    fn format_rate_mib(rate_mib_s: f32) -> String {
        format!("{rate_mib_s:.2} MiB/s").replace('.', ",")
    }

    fn format_temp_c(temp_celsius: f32) -> String {
        format!("{temp_celsius:.1} °C").replace('.', ",")
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

    fn read_cpu_temperature_celsius() -> Option<f32> {
        let Ok(entries) = fs::read_dir("/sys/class/thermal") else {
            return None;
        };

        let mut fallback = None;
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if !name.starts_with("thermal_zone") {
                continue;
            }

            let raw_temp = fs::read_to_string(path.join("temp")).ok();
            let Some(raw_temp) = raw_temp else {
                continue;
            };
            let Some(temp_celsius) = Self::parse_temperature_celsius(&raw_temp) else {
                continue;
            };

            let zone_type = fs::read_to_string(path.join("type"))
                .unwrap_or_default()
                .to_ascii_lowercase();
            let is_cpu_zone = zone_type.contains("x86_pkg_temp")
                || zone_type.contains("cpu")
                || zone_type.contains("package")
                || zone_type.contains("tctl");

            if is_cpu_zone {
                return Some(temp_celsius);
            }
            if fallback.is_none() {
                fallback = Some(temp_celsius);
            }
        }

        fallback
    }

    fn read_gpu_runtime_info() -> GpuRuntimeInfo {
        Self::read_gpu_runtime_from_nvidia_smi()
            .or_else(Self::read_gpu_runtime_from_sysfs)
            .unwrap_or_default()
    }

    fn read_gpu_runtime_from_nvidia_smi() -> Option<GpuRuntimeInfo> {
        let output = Command::new("nvidia-smi")
            .args([
                "--query-gpu=name,utilization.gpu,memory.used,memory.total,clocks.current.graphics,clocks.max.graphics,temperature.gpu,driver_version",
                "--format=csv,noheader,nounits",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }

        let line = String::from_utf8_lossy(&output.stdout)
            .lines()
            .find(|line| !line.trim().is_empty())?
            .trim()
            .to_string();
        let columns = line
            .split(',')
            .map(|part| part.trim().to_string())
            .collect::<Vec<_>>();
        if columns.len() < 8 {
            return None;
        }

        let utilization_percent = columns[1]
            .parse::<f32>()
            .ok()
            .map(|value| value.clamp(0.0, 100.0));
        let vram_used_bytes = columns[2]
            .parse::<u64>()
            .ok()
            .map(|value| value * 1024 * 1024);
        let vram_total_bytes = columns[3]
            .parse::<u64>()
            .ok()
            .map(|value| value * 1024 * 1024);
        let current_clock_mhz = columns[4].parse::<u64>().ok();
        let max_clock_mhz = columns[5].parse::<u64>().ok();
        let temperature_celsius = columns[6]
            .parse::<f32>()
            .ok()
            .and_then(Self::parse_temperature_celsius_from_value);

        Some(GpuRuntimeInfo {
            name: Self::short_gpu_name(&columns[0], "NVIDIA"),
            provider: "NVIDIA".to_string(),
            driver: columns[7].clone(),
            utilization_percent,
            temperature_celsius,
            vram_used_bytes,
            vram_total_bytes,
            current_clock_mhz,
            max_clock_mhz,
        })
    }

    fn read_gpu_runtime_from_sysfs() -> Option<GpuRuntimeInfo> {
        let card_path = Self::primary_drm_card_path()?;
        let device_path = card_path.join("device");

        let vendor_raw = fs::read_to_string(device_path.join("vendor"))
            .ok()
            .map(|value| value.trim().to_ascii_lowercase());
        let provider = vendor_raw
            .as_deref()
            .map(Self::gpu_provider_from_vendor_id)
            .unwrap_or_else(|| "Unknown".to_string());
        let driver =
            Self::gpu_driver_from_device(&device_path).unwrap_or_else(|| "Unknown".to_string());
        let name = Self::gpu_name_from_device(&device_path, &provider)
            .unwrap_or_else(|| format!("{provider} GPU"));
        let utilization_percent = Self::gpu_busy_percent_from_device(&device_path);
        let temperature_celsius = Self::gpu_temperature_from_device(&device_path);
        let (vram_used_bytes, vram_total_bytes) = Self::gpu_vram_from_device(&device_path);
        let (current_clock_mhz, max_clock_mhz) = Self::gpu_clock_from_device(&device_path);

        Some(GpuRuntimeInfo {
            name: Self::short_gpu_name(&name, &provider),
            provider,
            driver,
            utilization_percent,
            temperature_celsius,
            vram_used_bytes,
            vram_total_bytes,
            current_clock_mhz,
            max_clock_mhz,
        })
    }

    fn primary_drm_card_path() -> Option<PathBuf> {
        let mut cards = fs::read_dir("/sys/class/drm")
            .ok()?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                if !name.starts_with("card") || name.contains('-') {
                    return None;
                }
                let path = entry.path();
                if !path.join("device").exists() {
                    return None;
                }
                Some(path)
            })
            .collect::<Vec<_>>();
        cards.sort();
        cards.into_iter().next()
    }

    fn gpu_provider_from_vendor_id(vendor_id: &str) -> String {
        match vendor_id.trim() {
            "0x10de" => "NVIDIA".to_string(),
            "0x1002" | "0x1022" => "AMD".to_string(),
            "0x8086" => "Intel".to_string(),
            _ => "Unknown".to_string(),
        }
    }

    fn gpu_driver_from_device(device_path: &Path) -> Option<String> {
        if let Ok(link) = fs::read_link(device_path.join("driver")) {
            if let Some(driver) = link.file_name().and_then(|value| value.to_str()) {
                if !driver.trim().is_empty() {
                    return Some(driver.to_string());
                }
            }
        }

        let uevent = fs::read_to_string(device_path.join("uevent")).ok()?;
        for line in uevent.lines() {
            if let Some((key, value)) = line.split_once('=') {
                if key == "DRIVER" && !value.trim().is_empty() {
                    return Some(value.trim().to_string());
                }
            }
        }
        None
    }

    fn gpu_name_from_device(device_path: &Path, provider: &str) -> Option<String> {
        let uevent = fs::read_to_string(device_path.join("uevent")).ok()?;
        let pci_slot = uevent
            .lines()
            .filter_map(|line| line.split_once('='))
            .find_map(|(key, value)| (key == "PCI_SLOT_NAME").then(|| value.trim().to_string()));

        if let Some(slot) = pci_slot {
            let output = Command::new("lspci")
                .args(["-s", slot.as_str()])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
                .ok()?;
            if output.status.success() {
                let line = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if !line.is_empty() {
                    if let Some((_, rest)) = line.split_once(": ") {
                        if let Some((_, name)) = rest.split_once(": ") {
                            if !name.trim().is_empty() {
                                return Some(name.trim().to_string());
                            }
                        }
                        return Some(rest.to_string());
                    }
                    return Some(line);
                }
            }
        }

        Some(format!("{provider} GPU"))
    }

    fn short_gpu_name(raw_name: &str, provider: &str) -> String {
        let mut name = raw_name.trim().to_string();
        if name.is_empty() {
            return format!("{provider} GPU");
        }

        // Strip common PCI noise first, then prefer the model in the final bracket pair.
        name = name
            .split(" (rev ")
            .next()
            .unwrap_or(name.as_str())
            .trim()
            .to_string();
        if let (Some(start), Some(end)) = (name.rfind('['), name.rfind(']')) {
            if end > start + 1 {
                let bracketed = name[start + 1..end].trim();
                if !bracketed.is_empty() {
                    name = bracketed.to_string();
                }
            }
        }

        name = name
            .replace("Advanced Micro Devices, Inc. [AMD/ATI]", "")
            .replace("NVIDIA Corporation", "NVIDIA")
            .replace("Intel Corporation", "Intel")
            .replace("(TM)", "")
            .replace("  ", " ")
            .trim()
            .to_string();

        if provider.eq_ignore_ascii_case("AMD") {
            if let Some(rest) = name.strip_prefix("Radeon ") {
                return format!("AMD {rest}").trim().to_string();
            }
            if !name.to_ascii_lowercase().starts_with("amd ") {
                return format!("AMD {name}").trim().to_string();
            }
        }

        name
    }

    fn gpu_busy_percent_from_device(device_path: &Path) -> Option<f32> {
        let raw = fs::read_to_string(device_path.join("gpu_busy_percent")).ok()?;
        let value = raw.trim().parse::<f32>().ok()?;
        Some(value.clamp(0.0, 100.0))
    }

    fn gpu_temperature_from_device(device_path: &Path) -> Option<f32> {
        let Ok(hwmon_entries) = fs::read_dir(device_path.join("hwmon")) else {
            return None;
        };

        for entry in hwmon_entries.flatten() {
            let raw = fs::read_to_string(entry.path().join("temp1_input")).ok();
            let Some(raw) = raw else {
                continue;
            };
            let Some(temp_celsius) = Self::parse_temperature_celsius(&raw) else {
                continue;
            };
            return Some(temp_celsius);
        }

        None
    }

    fn gpu_vram_from_device(device_path: &Path) -> (Option<u64>, Option<u64>) {
        let used = fs::read_to_string(device_path.join("mem_info_vram_used"))
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok());
        let total = fs::read_to_string(device_path.join("mem_info_vram_total"))
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok());
        (used, total)
    }

    fn gpu_clock_from_device(device_path: &Path) -> (Option<u64>, Option<u64>) {
        if let Ok(raw) = fs::read_to_string(device_path.join("pp_dpm_sclk")) {
            let mut current = None;
            let mut max = None;

            for line in raw.lines() {
                let mhz = Self::parse_mhz_from_dpm_line(line);
                let Some(mhz) = mhz else {
                    continue;
                };
                if line.contains('*') {
                    current = Some(mhz);
                }
                max = Some(max.map_or(mhz, |existing: u64| existing.max(mhz)));
            }

            if current.is_some() || max.is_some() {
                return (current.or(max), max);
            }
        }

        let Ok(hwmon_entries) = fs::read_dir(device_path.join("hwmon")) else {
            return (None, None);
        };

        for entry in hwmon_entries.flatten() {
            let freq_path = entry.path().join("freq1_input");
            let max_path = entry.path().join("freq1_max");
            let current_mhz = fs::read_to_string(freq_path)
                .ok()
                .and_then(|raw| raw.trim().parse::<u64>().ok())
                .map(|hz| hz / 1_000_000);
            let max_mhz = fs::read_to_string(max_path)
                .ok()
                .and_then(|raw| raw.trim().parse::<u64>().ok())
                .map(|hz| hz / 1_000_000);

            if current_mhz.is_some() || max_mhz.is_some() {
                return (current_mhz.or(max_mhz), max_mhz);
            }
        }

        (None, None)
    }

    fn parse_mhz_from_dpm_line(line: &str) -> Option<u64> {
        let lower = line.to_ascii_lowercase();
        let mhz_index = lower.find("mhz")?;
        let prefix = &line[..mhz_index];
        prefix
            .split(|ch: char| !ch.is_ascii_digit())
            .filter(|part| !part.is_empty())
            .next_back()
            .and_then(|part| part.parse::<u64>().ok())
    }

    fn parse_temperature_celsius(raw: &str) -> Option<f32> {
        let value = raw.trim().parse::<f32>().ok()?;
        Self::parse_temperature_celsius_from_value(value)
    }

    fn parse_temperature_celsius_from_value(value: f32) -> Option<f32> {
        if value <= 0.0 {
            return None;
        }
        if value > 1000.0 {
            Some(value / 1000.0)
        } else {
            Some(value)
        }
    }

    fn list_active_network_interfaces() -> Vec<NetworkInterfaceInfo> {
        let Ok(entries) = fs::read_dir("/sys/class/net") else {
            return Vec::new();
        };

        let mut interfaces = Vec::new();
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if name == "lo" {
                continue;
            }

            let path = entry.path();
            let operstate = fs::read_to_string(path.join("operstate")).unwrap_or_default();
            if operstate.trim() != "up" {
                continue;
            }

            let is_wireless = path.join("wireless").exists();
            let speed_mbps = Self::read_network_speed_mbps(&path);
            let rx_bytes = Self::read_network_counter(path.join("statistics/rx_bytes"));
            let tx_bytes = Self::read_network_counter(path.join("statistics/tx_bytes"));

            interfaces.push(NetworkInterfaceInfo {
                name,
                is_wireless,
                speed_mbps,
                rx_bytes,
                tx_bytes,
            });
        }

        interfaces.sort_by(|a, b| a.name.cmp(&b.name));
        interfaces
    }

    fn read_network_speed_mbps(path: &Path) -> Option<u64> {
        let raw = fs::read_to_string(path.join("speed")).ok()?;
        let value = raw.trim().parse::<i64>().ok()?;
        if value > 0 { Some(value as u64) } else { None }
    }

    fn read_network_counter(path: PathBuf) -> u64 {
        fs::read_to_string(path)
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .unwrap_or(0)
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
