use eframe::egui::{self, Color32 as C, RichText, Vec2};
#[path = "ui/icons.rs"]
mod icons;
#[path = "ui/worker.rs"]
mod worker;
use hh_clicker_launcher::{
    config::{Config, Layout, APP_VERSION},
    platform,
    runtime::{Operation, Runtime, State, Status, DESCRIPTIONS, NAMES},
    setup::{self, Preparation},
    updates,
};
use std::{
    sync::mpsc::{self, Receiver, Sender},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const ORANGE: C = C::from_rgb(255, 91, 10);
const TEXT: C = C::from_rgb(32, 41, 57);
const MUTED: C = C::from_rgb(118, 129, 149);
const BORDER: C = C::from_rgb(223, 227, 232);
const BG: C = C::from_rgb(241, 244, 248);
const GREEN: C = C::from_rgb(40, 167, 69);
const RED: C = C::from_rgb(229, 72, 77);
const RELEASES: &str = "https://github.com/hh-clicker/.github/releases";
enum Action {
    Refresh,
    Tick,
    Start,
    Stop,
    Ports,
    Prepare(Preparation),
    Auth(&'static str),
    Close,
}

pub struct Launcher {
    qa_close_stage: Option<u8>,
    update_sender: Sender<updates::Update>,
    update_receiver: Receiver<updates::Update>,
    checking_updates: bool,
    cancellation: Arc<AtomicBool>,
    monitor_stop: Arc<AtomicBool>,
    observations: Receiver<(u64, Vec<hh_clicker_launcher::runtime::Component>)>,
    state: State,
    tx: Sender<Action>,
    rx: Receiver<State>,
    closed: Receiver<()>,
    layout: Layout,
    settings: bool,
    confirm: Option<Preparation>,
    close_failed: bool,
    offline: bool,
    quit_after: Option<Duration>,
    started: Instant,
    first_frame: bool,
    allow_exit: bool,
    opened_device_url: Option<String>,
    initial_navigation: bool,
    copied_code: bool,
    preview: Option<String>,
    preparation: Option<Preparation>,
    pending_stop: bool,
    chrome_notice: bool,
}

fn finish_environment_navigation(
    settings: &mut bool,
    preparation: &mut Option<Preparation>,
    previous_revision: u64,
    state: &State,
    offline: bool,
) -> bool {
    if state.environment_revision <= previous_revision || state.busy || state.error.is_some() {
        return false;
    }
    if matches!(preparation, Some(Preparation::Chrome)) {
        *settings = true;
        return true;
    }
    *preparation = None;
    if !offline && state.ready {
        *settings = false;
    }
    false
}

impl Launcher {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        layout: Layout,
        config: Config,
        offline: bool,
        settings: bool,
        quit_after: Option<Duration>,
        scale: Option<f32>,
        started: Instant,
        close_during_start: bool,
    ) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Light);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::light();
        style.visuals.override_text_color = Some(TEXT);
        style.visuals.panel_fill = BG;
        style.visuals.window_fill = C::WHITE;
        style.visuals.widgets.inactive.bg_fill = C::from_rgb(249, 250, 251);
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
        style.visuals.widgets.hovered.bg_fill = C::from_rgb(255, 240, 231);
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, ORANGE);
        style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
        style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
        style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
        style.visuals.selection.bg_fill = C::from_rgb(255, 228, 210);
        style.spacing.button_padding = egui::vec2(13.0, 9.0);
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.visuals.panel_fill = C::TRANSPARENT;
        style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::NONE;
        style.spacing.scroll.bar_width = 3.0;
        style.spacing.scroll.floating = false;
        style.spacing.scroll.bar_inner_margin = 0.0;
        style.spacing.scroll.bar_outer_margin = 0.0;
        for (kind, size) in [
            (egui::TextStyle::Body, 14.0),
            (egui::TextStyle::Button, 14.0),
            (egui::TextStyle::Small, 12.0),
            (egui::TextStyle::Heading, 22.0),
        ] {
            style
                .text_styles
                .insert(kind, egui::FontId::proportional(size));
        }
        cc.egui_ctx.set_style_of(egui::Theme::Light, style);
        if let Some(scale) = scale {
            cc.egui_ctx.set_zoom_factor(scale);
        }
        let state = State::new(config.clone());
        let (tx, actions) = mpsc::channel();
        let (states, rx) = mpsc::channel();
        let (done, closed) = mpsc::channel();
        let context = cc.egui_ctx.clone();
        let runtime = Runtime::new(layout.clone(), config, offline);
        let cancellation = runtime.cancellation.clone();
        let shared = Arc::new(Mutex::new(state.clone()));
        let shared_worker = shared.clone();
        let monitor_stop = Arc::new(AtomicBool::new(false));
        let stop = monitor_stop.clone();
        let (observation_sender, observations) = mpsc::channel();
        let monitor_ctx = cc.egui_ctx.clone();
        let monitor_layout = layout.clone();
        std::thread::Builder::new()
            .name("launcher-observer".into())
            .spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    let snapshot = shared.lock().unwrap_or_else(|p| p.into_inner()).clone();
                    if snapshot.busy && snapshot.operation != Operation::Stopping && !offline {
                        let mut observer =
                            Runtime::new(monitor_layout.clone(), snapshot.config.clone(), false);
                        if observer.observe().is_ok() {
                            let _ = observation_sender
                                .send((snapshot.revision, observer.state.components));
                            monitor_ctx.request_repaint();
                        }
                    }
                    for _ in 0..20 {
                        if stop.load(Ordering::Acquire) {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            })
            .expect("observer thread");
        std::thread::Builder::new()
            .name("launcher-runtime".into())
            .spawn(move || worker::run(runtime, actions, states, done, context, shared_worker))
            .expect("runtime worker");
        let (update_sender, update_receiver) = mpsc::channel();
        let mut app = Self {
            qa_close_stage: if close_during_start { Some(0) } else { None },
            update_sender,
            update_receiver,
            checking_updates: false,
            cancellation,
            monitor_stop,
            observations,
            state,
            tx,
            rx,
            closed,
            layout,
            settings,
            confirm: None,
            close_failed: false,
            offline,
            quit_after,
            started,
            first_frame: true,
            allow_exit: false,
            opened_device_url: None,
            initial_navigation: true,
            copied_code: false,
            preview: std::env::var("HH_RUST_UI_PREVIEW").ok().filter(|_| offline),
            preparation: None,
            pending_stop: false,
            chrome_notice: false,
        };
        if !offline {
            app.request_updates();
        }
        app
    }
    fn request_updates(&mut self) {
        if self.offline || self.checking_updates {
            return;
        }
        self.checking_updates = true;
        let tx = self.update_sender.clone();
        std::thread::spawn(move || {
            let result = updates::check().unwrap_or_else(|_| updates::Update {
                message: "Проверка обновлений недоступна. Повторите позже".into(),
                ..Default::default()
            });
            let _ = tx.send(result);
        });
    }
    fn send(&mut self, action: Action) {
        if self.preview.is_some() && !matches!(action, Action::Close) {
            return;
        }
        if self.state.busy && !matches!(action, Action::Stop | Action::Close) {
            return;
        }
        if let Action::Prepare(preparation) = &action {
            self.chrome_notice = false;
            self.preparation = Some(*preparation);
        }
        if matches!(action, Action::Refresh) && !self.chrome_notice {
            self.preparation = None;
        }
        if matches!(action, Action::Auth("/auth/login/start")) {
            self.opened_device_url = None;
        }
        let stopping = matches!(action, Action::Stop | Action::Close);
        if self.tx.send(action).is_ok() {
            self.state.busy = true;
            self.state.error = None;
            if stopping {
                self.pending_stop = true;
                self.state.mark_stopping();
            }
        }
    }
    fn link(&mut self, url: &str) {
        if self.preview.is_some() {
            return;
        }
        if let Err(e) = platform::open_url(url) {
            self.state.error = Some(e.to_string());
        }
    }
    fn close(&mut self) {
        if !self.state.closing {
            if self.state.busy {
                self.cancellation.store(true, Ordering::Release);
            }
            self.state.closing = true;
            self.send(Action::Close);
        }
    }
    fn suggested_preparation(&self) -> Option<(Preparation, &'static str, &'static str)> {
        let failed = |name: &str| {
            self.state
                .checks
                .iter()
                .any(|c| c.name == name && !c.passed)
        };
        if failed("Виртуализация") {
            return None;
        }
        if failed("WSL") {
            return Some((if self.state.checks.iter().any(|c|c.name=="WSL"&&c.message.contains("устарел")){Preparation::WslUpdate}else{Preparation::Wsl},"Подготовка WSL 2","Установите или обновите WSL без дистрибутива Linux. Может потребоваться перезагрузка."));
        }
        if failed("WSL 2") {
            return Some((
                Preparation::WslDefault,
                "Выбор WSL 2",
                "Сделайте WSL 2 версией по умолчанию.",
            ));
        }
        if failed("Docker Desktop") {
            return Some((Preparation::DockerInstall,"Подготовка Docker Desktop","Лицензия принимается отдельно перед установкой. Существующие данные Docker сохраняются."));
        }
        if failed("Docker Engine") {
            return Some((
                Preparation::DockerStart,
                "Docker Engine недоступен",
                "Запустите установленный Docker Desktop. Нужны Linux containers с backend WSL 2.",
            ));
        }
        if failed("Node.js") {
            return Some((
                Preparation::Node,
                "Подготовка Node.js",
                "Скачайте отдельный portable runtime; системный PATH не изменяется.",
            ));
        }
        if failed("Google Chrome") {
            return Some((
                Preparation::Chrome,
                "Подготовка Google Chrome",
                "Chrome требуется Browser Agent. Условия Google принимаются отдельно.",
            ));
        }
        if failed("Browser Agent") {
            return Some((
                Preparation::Browser,
                "Подготовка Browser Agent",
                "Подготовьте отсутствующие файлы и зависимости агента в каталоге этой копии.",
            ));
        }
        None
    }
}

impl eframe::App for Launcher {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(state) = self.rx.try_recv() {
            // Ignore already in-flight preflight/tick/start snapshots between the
            // click and the worker acknowledging this stop request.
            if self.pending_stop {
                if state.operation != Operation::Stopping {
                    continue;
                }
                self.pending_stop = false;
            }
            let update = self.state.update.clone();
            let closing = self.state.closing;
            let close_failed = state.closing && !state.busy && state.error.is_some();
            self.chrome_notice |= finish_environment_navigation(
                &mut self.settings,
                &mut self.preparation,
                self.state.environment_revision,
                &state,
                self.offline,
            );
            self.state = state;
            self.state.update = update;
            if closing {
                self.state.closing = true;
            }
            if close_failed {
                self.close_failed = true;
            }
            if self.initial_navigation && !self.state.checks.is_empty() {
                self.initial_navigation = false;
                if !self.offline && !self.state.ready {
                    self.settings = true;
                }
            }
            if !self.offline && self.state.auth.pending {
                if let Some(url) = self.state.auth.url.clone() {
                    if self.opened_device_url.as_ref() != Some(&url) {
                        self.opened_device_url = Some(url.clone());
                        self.link(&url);
                    }
                }
            }
        }
        while let Ok((revision, components)) = self.observations.try_recv() {
            self.state.apply_observation(revision, components);
        }
        while let Ok(update) = self.update_receiver.try_recv() {
            self.state.update = update;
            self.checking_updates = false;
        }
        if self.qa_close_stage == Some(0) && self.state.ready && !self.state.busy {
            self.qa_close_stage = Some(1);
            self.send(Action::Start);
        }
        if self.qa_close_stage == Some(1)
            && self.state.busy
            && self.state.message.contains("Docker-образы")
            && std::fs::metadata(self.layout.data.join("logs/compose.log"))
                .is_ok_and(|m| m.len() > 0)
        {
            self.qa_close_stage = Some(2);
            let _=std::fs::write(self.layout.data.join("qa-close-during-start.json"),serde_json::json!({"phase":self.state.message,"revision":self.state.revision,"log_bytes":std::fs::metadata(self.layout.data.join("logs/compose.log")).map(|m|m.len()).unwrap_or(0)}).to_string());
            self.close();
        }
        if self.closed.try_recv().is_ok() {
            self.allow_exit = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_exit {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close();
        }
        if self.quit_after.is_some_and(|d| self.started.elapsed() > d) {
            self.quit_after = None;
            self.close();
        }
        self.apply_preview(ctx);
        self.title(ctx);
        self.sidebar(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(BG).inner_margin(egui::Margin {
                left: 22,
                right: 22,
                top: 20,
                bottom: 16,
            }))
            .show(ctx, |ui| {
                if self.settings {
                    self.settings_page(ui);
                } else {
                    self.main_page(ui);
                }
            });
        if let Some(action) = self.confirm {
            egui::Window::new("Подготовка зависимости")
                .collapsible(false)
                .resizable(false)
                .fixed_size([480.0, 200.0])
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(action.description());
                    ui.add_space(15.0);
                    if ui
                        .button(
                            if matches!(action, Preparation::DockerInstall | Preparation::Chrome) {
                                "Принимаю и установить"
                            } else {
                                "Выполнить"
                            },
                        )
                        .clicked()
                    {
                        self.confirm = None;
                        self.send(Action::Prepare(action));
                    }
                    if ui.button("Отмена").clicked() {
                        self.confirm = None;
                    }
                });
        }
        if let Some(error) = self.state.error.clone().filter(|_| self.close_failed) {
            egui::Window::new("Требуется внимание").collapsible(false).resizable(false).default_width(510.0).anchor(egui::Align2::CENTER_CENTER,[0.0,0.0]).show(ctx,|ui|{
                egui::ScrollArea::vertical().max_height(240.0).show(ui,|ui|{ui.add(egui::Label::new(RichText::new(&error).color(RED)).wrap());});
                if ui.button("Скопировать сообщение").clicked(){ctx.copy_text(error);}
                if ui.button("Открыть настройки").clicked(){self.settings=true;self.state.error=None;}
                if ui.button("Закрыть сообщение").clicked(){self.state.error=None;}
                if self.close_failed || self.state.closing {ui.label("Не все сервисы остановлены. Можно повторить остановку или оставить контейнеры работающими и выйти.");if ui.button("Повторить остановку").clicked(){self.state.closing=false;self.close();}if ui.button("Выйти, оставив контейнеры").clicked(){self.allow_exit=true;ctx.send_viewport_cmd(egui::ViewportCommand::Close);}}
            });
        }
        if self.first_frame
            && ctx.screen_rect().width() < 2000.0
            && ctx.screen_rect().height() < 1500.0
        {
            self.first_frame = false;
            let report = serde_json::json!({"first_frame_ms":self.started.elapsed().as_millis(),"offline":self.offline,"pixels_per_point":ctx.pixels_per_point(),"size_points":[ctx.screen_rect().width(),ctx.screen_rect().height()]});
            let _ = std::fs::write(
                self.layout.data.join("startup-metrics.json"),
                report.to_string(),
            );
        }
        ctx.request_repaint_after(Duration::from_millis(if self.state.busy {
            100
        } else {
            500
        }));
    }
}
impl Drop for Launcher {
    fn drop(&mut self) {
        self.monitor_stop.store(true, Ordering::Release);
        self.cancellation.store(true, Ordering::Release);
    }
}
fn status_color(s: Status) -> C {
    match s {
        Status::Running => GREEN,
        Status::Failed => RED,
        Status::Starting => C::from_rgb(35, 132, 223),
        Status::Stopping => ORANGE,
        _ => MUTED,
    }
}

include!("ui/view.rs");
