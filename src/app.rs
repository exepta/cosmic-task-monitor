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
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, Signal, System, UpdateKind};

const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const APP_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/apps/icon.svg");
const PROCESS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

mod process;

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

fn table_row_button_style() -> theme::Button {
    theme::Button::Custom {
        active: Box::new(|_focused, _theme| widget::button::Style::new()),
        hovered: Box::new(|_focused, theme| {
            let mut style = widget::button::Style::new();
            style.background = Some(Background::Color(
                theme.current_container().component.hover.into(),
            ));
            style
        }),
        pressed: Box::new(|_focused, theme| {
            let mut style = widget::button::Style::new();
            style.background = Some(Background::Color(
                theme.current_container().component.hover.into(),
            ));
            style
        }),
        disabled: Box::new(|_theme| widget::button::Style::new()),
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
    gpu_percent: f32,
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
    Gpu,
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
    selected_process: Option<SelectedProcess>,
    gpu_engine_ns_by_pid: HashMap<u32, u64>,
    last_gpu_usage_by_pid: HashMap<u32, f32>,
    last_gpu_sample_at: Option<Instant>,
    sort_state: SortState,
}

#[derive(Debug, Clone)]
pub enum Message {
    LaunchUrl(String),
    ToggleContextPage(ContextPage),
    UpdateConfig(Config),
    RefreshProcesses,
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
            desktop_apps_by_exec: Self::load_desktop_app_map(),
            steam_apps_by_id: HashMap::new(),
            process_entries: Vec::new(),
            selected_process: None,
            gpu_engine_ns_by_pid: HashMap::new(),
            last_gpu_usage_by_pid: HashMap::new(),
            last_gpu_sample_at: None,
            sort_state: SortState {
                column: SortColumn::Ram,
                direction: SortDirection::Desc,
            },
        };

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

                let table_header =
                    widget::row::with_capacity(6)
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
                                    self.header_button_content(fl!("table-gpu"), SortColumn::Gpu),
                                )
                                .on_press(Message::ToggleSort(SortColumn::Gpu))
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
                            widget::button::custom(
                                widget::row::with_capacity(6)
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
                                        widget::container(widget::text(format!(
                                            "{:.1}%",
                                            process.gpu_percent
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
                    .push(widget::text::title3(fl!("nav-performance")))
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
}

impl menu::action::MenuAction for MenuAction {
    type Message = Message;

    fn message(&self) -> Self::Message {
        match self {
            MenuAction::About => Message::ToggleContextPage(ContextPage::About),
        }
    }
}
