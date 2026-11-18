// Presentation geometry follows LauncherScreen.kt. Runtime ownership stays in Runtime.
const SECONDARY: C = C::from_rgb(71, 84, 103);
const SURFACE: C = C::from_rgb(252, 253, 254); // white at .82 over #f1f4f8
const RULE: C = C::from_rgb(226, 231, 238);
const STOPPED: C = C::from_rgb(152, 162, 179);
const NEUTRAL: C = C::from_rgb(102, 112, 133);
const BLUE: C = C::from_rgb(35, 132, 223);
type Rect = egui::Rect;
pub const PREVIEW_CASES: &[&str] = &[
    "ready",
    "checking",
    "node",
    "wsl",
    "docker",
    "chrome",
    "ports",
    "starting",
    "stopping",
    "running",
    "signed-out",
    "authorizing",
    "auth-checking",
    "auth-failed",
    "failed",
    "partial",
    "update",
    "critical",
    "preparing",
    "prepared",
    "prepare-failed",
    "virtualization",
    "reboot",
];

fn box_at(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
}
fn font(size: f32, semibold: bool) -> egui::FontId {
    egui::FontId::new(
        size,
        if semibold {
            egui::FontFamily::Name("Inter Bold".into())
        } else {
            egui::FontFamily::Proportional
        },
    )
}
fn line(ui: &egui::Ui, rect: Rect, text: &str, size: f32, semibold: bool, color: C) {
    let mut job =
        egui::text::LayoutJob::simple_singleline(text.into(), font(size, semibold), color);
    job.wrap.max_width = rect.width();
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.fonts(|f| f.layout_job(job));
    ui.painter().galley(
        egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}
fn paragraph(
    ui: &egui::Ui,
    rect: Rect,
    text: &str,
    size: f32,
    height: f32,
    max_rows: usize,
    color: C,
) -> f32 {
    let mut job =
        egui::text::LayoutJob::simple(text.into(), font(size, false), color, rect.width());
    job.wrap.max_rows = max_rows;
    job.sections[0].format.line_height = Some(height);
    let galley = ui.fonts(|f| f.layout_job(job));
    let used = galley.size().y;
    ui.painter().galley(rect.min, galley, color);
    used
}
fn text_width(ui: &egui::Ui, text: &str, size: f32, semibold: bool) -> f32 {
    ui.fonts(|f| {
        f.layout_no_wrap(text.into(), font(size, semibold), C::WHITE)
            .size()
            .x
    })
}
use platform::local_checked_at;

fn auth_detail_lines(details: &str) -> Vec<String> {
    let mut limits = Vec::new();
    let mut resets = Vec::new();
    let mut checked = None;
    let mut extra = Vec::new();
    for line in details.lines() {
        if let Some(value) = line.strip_prefix("Проверено: ") {
            checked = Some(format!(
                "Проверено: {}",
                local_checked_at(value).unwrap_or_else(|| value.into())
            ));
        } else if let Some((limit, reset)) = line.split_once(" · сброс ") {
            limits.push(limit.to_owned());
            if let Some((label, _)) = limit.split_once(':') {
                resets.push(format!("{label}: {reset}"));
            }
        } else if line.contains(": осталось ") {
            limits.push(line.to_owned());
        } else {
            extra.push(line.to_owned());
        }
    }
    let mut result = Vec::new();
    if !limits.is_empty() {
        result.push(limits.join(" · "));
    }
    if let Some(checked) = checked {
        result.push(checked);
    }
    if !resets.is_empty() {
        result.push(format!("Сброс — {}", resets.join(" · ")));
    }
    result.extend(extra);
    result
}
fn svg(ui: &mut egui::Ui, rect: Rect, source: egui::ImageSource<'_>, color: C) {
    icons::paint(ui, rect, source, color);
}
fn rule(ui: &egui::Ui, x: f32, y: f32, w: f32) {
    ui.painter().rect_filled(box_at(x, y, w, 1.0), 0, RULE);
}
fn hit(ui: &mut egui::Ui, rect: Rect, id: impl std::hash::Hash, enabled: bool) -> egui::Response {
    let response = ui.interact(
        rect,
        ui.id().with(id),
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}
fn action_button(
    ui: &mut egui::Ui,
    rect: Rect,
    id: &str,
    label: &str,
    icon: Option<egui::ImageSource<'_>>,
    enabled: bool,
    primary: bool,
) -> bool {
    let response = hit(ui, rect, id, enabled);
    let fill = if !enabled {
        C::from_rgb(255, 162, 120)
    } else if response.hovered() {
        C::from_rgb(255, 106, 26)
    } else {
        ORANGE
    };
    ui.painter()
        .rect_filled(rect, if primary { 12 } else { 10 }, fill);
    let width = text_width(ui, label, 14.0, true) + if icon.is_some() { 30.0 } else { 0.0 };
    let x = rect.center().x - width / 2.0;
    if let Some(source) = icon {
        svg(
            ui,
            box_at(x, rect.center().y - 10.0, 20.0, 20.0),
            source,
            C::WHITE,
        );
    }
    line(
        ui,
        box_at(
            x + if primary { 30.0 } else { 0.0 },
            rect.top(),
            width,
            rect.height(),
        ),
        label,
        14.0,
        true,
        C::WHITE,
    );
    response.clicked() && enabled
}
fn text_button(ui: &mut egui::Ui, rect: Rect, id: &str, label: &str, enabled: bool) -> bool {
    let response = hit(ui, rect, id, enabled);
    if response.hovered() && enabled {
        ui.painter()
            .rect_filled(rect, 9, C::from_rgb(255, 240, 231));
    }
    line(
        ui,
        rect.shrink2(egui::vec2(13.0, 0.0)),
        label,
        14.0,
        true,
        if enabled { ORANGE } else { MUTED },
    );
    response.clicked() && enabled
}

impl Launcher {
    fn visual_enabled(&self) -> bool {
        !self.offline || self.preview.is_some()
    }
    fn visual_busy(&self) -> bool {
        self.state.busy
            || self.state.auth.pending
            || self.state.auth.message.starts_with("Проверяем авторизацию")
    }
    fn apply_preview(&mut self, ctx: &egui::Context) {
        let Some(mut name) = self.preview.clone() else {
            return;
        };
        if ctx.input(|i| i.key_pressed(egui::Key::F8)) {
            let index = PREVIEW_CASES.iter().position(|s| *s == name).unwrap_or(0);
            let next = if ctx.input(|i| i.modifiers.shift) {
                (index + PREVIEW_CASES.len() - 1) % PREVIEW_CASES.len()
            } else {
                (index + 1) % PREVIEW_CASES.len()
            };
            name = PREVIEW_CASES[next].into();
            self.preview = Some(name.clone());
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "HH Clicker — UI PREVIEW · {name} · F8"
        )));
        use hh_clicker_launcher::{
            auth::Auth,
            runtime::{Check, Component},
        };
        self.state.ready = true;
        self.state.busy = false;
        self.state.closing = false;
        self.state.error = None;
        self.state.runtime_info.clear();
        self.preparation = None;
        self.chrome_notice = false;
        self.state.update = Default::default();
        self.state.components = vec![
            Component {
                status: Status::Stopped,
                details: String::new()
            };
            5
        ];
        self.state.auth = Auth {
            message: "Сначала запустите Generation Agent".into(),
            ..Default::default()
        };
        self.state.message = "Все компоненты сейчас остановлены".into();
        self.state.checks = [
            (
                "Виртуализация",
                "Включена в BIOS/UEFI, процессор поддерживает SLAT",
            ),
            ("WSL", "WSL 3.0.5.0"),
            ("WSL 2", "WSL 2 по умолчанию"),
            ("Docker endpoint", "Локальный Docker Desktop"),
            ("Docker Desktop", "4.97.0"),
            ("Docker CLI", "Docker version 29.9.2, build example"),
            ("Docker Compose V2", "5.6.1"),
            ("Docker Engine", "Linux · WSL 2 · 29.9.2"),
            ("Node.js", "v24.18.0"),
            ("Google Chrome", "Google Chrome установлен"),
            ("Browser Agent", "Собранный агент и Playwright доступны"),
            ("Файлы приложения", "Полный portable payload найден"),
            ("Принадлежность ресурсов", "Ресурсы принадлежат этой копии"),
            (
                "Локальные порты",
                "Порты свободны или принадлежат этой установке",
            ),
        ]
        .into_iter()
        .map(|(name, message)| Check {
            name: name.into(),
            passed: true,
            message: message.into(),
            required: true,
        })
        .collect();
        if matches!(
            name.as_str(),
            "running" | "signed-out" | "authorizing" | "auth-checking" | "auth-failed"
        ) {
            for c in &mut self.state.components {
                c.status = Status::Running;
            }
            self.state.runtime_info = "gpt-6-luna · low · CLI 0.163.0".into();
            self.state.auth.message =
                "Войдите через ChatGPT, чтобы генерировать сопроводительные".into();
            match name.as_str() {
                "running" => {
                    self.state.auth.signed_in = true;
                    self.state.auth.message = "Авторизация через ChatGPT активна".into();
                    self.state.auth.details="5 часов: осталось 82% · неделя: осталось 64%\nПроверено: 21:15\nСброс — 5 часов: 23:00 · неделя: 23 ноября, 12:00".into();
                }
                "authorizing" => {
                    self.state.auth.pending = true;
                    self.state.auth.code = Some("ABCD-EFGH".into());
                    self.state.auth.url = Some("https://auth.openai.com/codex/device".into());
                }
                "auth-checking" => self.state.auth.message = "Проверяем авторизацию Codex…".into(),
                "auth-failed" => {
                    self.state.auth.message =
                        "Не удалось проверить авторизацию. Проверьте соединение".into()
                }
                _ => {}
            }
        }
        match name.as_str() {
            "preparing" | "prepared" | "prepare-failed" => {
                self.preparation = Some(Preparation::Node);
                self.state.message = "Node.js готов".into();
                if name != "prepared" {
                    self.state.ready = false;
                    self.state
                        .checks
                        .iter_mut()
                        .find(|c| c.name == "Node.js")
                        .unwrap()
                        .passed = false;
                }
                if name == "preparing" {
                    self.state.busy = true;
                    self.state.message = "Скачиваем Node.js · 24 из 37 МиБ".into();
                }
                if name == "prepare-failed" {
                    self.state.error = Some(
                        "Не удалось скачать Node.js. Проверьте соединение и повторите попытку."
                            .into(),
                    );
                }
            }
            "virtualization" => {
                self.state.ready = false;
                let c = self
                    .state
                    .checks
                    .iter_mut()
                    .find(|c| c.name == "Виртуализация")
                    .unwrap();
                c.passed = false;
                c.message =
                    "Включите Intel VT-x или AMD-V (SVM) в BIOS/UEFI и перезагрузите компьютер"
                        .into();
            }
            "reboot" => {
                self.state.ready = false;
                self.state.checks.push(Check {
                    name: "Перезагрузка Windows".into(),
                    passed: false,
                    message: "Перезагрузите компьютер и запустите лаунчер снова".into(),
                    required: true,
                });
            }
            "checking" => {
                self.state.checks.clear();
                self.state.ready = false;
            }
            "starting" => {
                self.state.busy = true;
                self.state.components[0].status = Status::Running;
                for c in &mut self.state.components[1..] {
                    c.status = Status::Starting;
                }
            }
            "stopping" => {
                self.state.busy = true;
                for c in &mut self.state.components {
                    c.status = Status::Stopping;
                }
            }
            "failed" => {
                self.state.components[0].status = Status::Running;
                self.state.components[1].status = Status::Failed;
                self.state.components[1].details="Backend не ответил на проверку готовности. Проверьте журнал запуска и повторите попытку.".into();
            }
            "partial" => {
                self.state.components[0].status = Status::Running;
                self.state.components[1].status = Status::Running;
            }
            "node" | "wsl" | "docker" | "chrome" | "ports" => {
                let (target, message) = match name.as_str() {
                    "node" => ("Node.js", "Совместимый Node.js не найден"),
                    "wsl" => ("WSL", "WSL 2 не установлен"),
                    "docker" => ("Docker Desktop", "Docker Desktop не найден"),
                    "chrome" => (
                        "Google Chrome",
                        "Не найден; лаунчер может скачать и установить официальную версию",
                    ),
                    _ => (
                        "Локальные порты",
                        "Backend: порт 8080 занят процессом example.exe (PID 1234)",
                    ),
                };
                self.state.ready = false;
                self.state.message = message.into();
                if let Some(c) = self.state.checks.iter_mut().find(|c| c.name == target) {
                    c.passed = false;
                    c.message = message.into();
                }
            }
            "update" | "critical" => {
                self.state.update.available = true;
                self.state.update.latest = "3.1.0".into();
                self.state.update.critical = name == "critical";
            }
            _ => {}
        }
    }
    fn title(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("title")
            .exact_height(38.0)
            .frame(egui::Frame::NONE.fill(C::from_rgb(248, 250, 252)))
            .show(ctx, |ui| {
                let r = ui.max_rect();
                svg(
                    ui,
                    box_at(r.left() + 13.0, r.top() + 8.0, 22.0, 22.0),
                    egui::include_image!("../../assets/brand_icon.svg"),
                    C::WHITE,
                );
                line(
                    ui,
                    box_at(r.left() + 44.0, r.top(), 300.0, 38.0),
                    "HH Clicker Launcher · Rust portable",
                    12.0,
                    false,
                    TEXT,
                );
                let drag = ui.interact(
                    box_at(r.left(), r.top(), r.width() - 88.0, 38.0),
                    ui.id().with("drag"),
                    egui::Sense::drag(),
                );
                if drag.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                for (offset, close) in [(88.0, false), (44.0, true)] {
                    let rect = box_at(r.right() - offset, r.top(), 44.0, 38.0);
                    let response = hit(ui, rect, if close { "close" } else { "minimize" }, true);
                    if response.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            0,
                            if close {
                                C::from_rgb(255, 232, 232)
                            } else {
                                C::from_rgb(233, 237, 242)
                            },
                        );
                    }
                    let color = if response.hovered() && close {
                        C::from_rgb(198, 40, 40)
                    } else {
                        SECONDARY
                    };
                    let c = rect.center();
                    let stroke = egui::Stroke::new(1.25_f32, color);
                    if close {
                        ui.painter().line_segment(
                            [c + egui::vec2(-4.0, -4.0), c + egui::vec2(4.0, 4.0)],
                            stroke,
                        );
                        ui.painter().line_segment(
                            [c + egui::vec2(4.0, -4.0), c + egui::vec2(-4.0, 4.0)],
                            stroke,
                        );
                    } else {
                        ui.painter().line_segment(
                            [c + egui::vec2(-5.0, 0.0), c + egui::vec2(5.0, 0.0)],
                            stroke,
                        );
                    }
                    if response.clicked() {
                        if close {
                            self.close();
                        } else {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                    }
                }
            });
    }
    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .exact_width(205.0)
            .resizable(false)
            .frame(egui::Frame::NONE.fill(C::from_rgb(248, 250, 252)))
            .show(ctx, |ui| {
                let r = ui.max_rect();
                let attention = (!self.state.ready && !self.state.checks.is_empty())
                    || self.failed()
                    || self.state.auth.message.contains("Не удалось");
                for (index, label, icon) in [
                    (
                        0,
                        "Главная",
                        egui::include_image!("../../assets/icon_layout_dashboard.svg"),
                    ),
                    (
                        1,
                        "Настройки",
                        egui::include_image!("../../assets/icon_settings.svg"),
                    ),
                ] {
                    let rect = box_at(
                        r.left() + 20.0,
                        r.top() + 22.0 + index as f32 * 53.0,
                        165.0,
                        45.0,
                    );
                    let selected = self.settings == (index == 1);
                    let response = hit(ui, rect, index, true);
                    if selected || response.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            11,
                            if selected {
                                C::from_rgb(249, 234, 228)
                            } else {
                                C::from_rgb(238, 241, 245)
                            },
                        );
                    }
                    svg(
                        ui,
                        box_at(rect.left() + 13.0, rect.top() + 12.0, 21.0, 21.0),
                        icon,
                        if selected { ORANGE } else { NEUTRAL },
                    );
                    line(
                        ui,
                        box_at(rect.left() + 46.0, rect.top(), 105.0, 45.0),
                        label,
                        14.0,
                        selected,
                        if selected { C::BLACK } else { SECONDARY },
                    );
                    if index == 1 && attention {
                        let c = egui::pos2(rect.right() - 21.5, rect.center().y);
                        ui.painter().circle_filled(c, 8.5, RED);
                        ui.painter().text(
                            c,
                            egui::Align2::CENTER_CENTER,
                            "!",
                            font(11.0, true),
                            C::WHITE,
                        );
                    }
                    if response.clicked() {
                        self.settings = index == 1;
                    }
                }
                let update = self.state.update.clone();
                let bottom = r.bottom() - 22.0;
                let version_height = if update.available { 18.0 } else { 24.0 };
                let version_y = bottom - version_height - if update.available { 36.0 } else { 0.0 };
                line(
                    ui,
                    box_at(r.left() + 20.0, version_y, 141.0, version_height),
                    &format!("Версия {APP_VERSION}"),
                    12.0,
                    false,
                    MUTED,
                );
                // Preserve portable-only tools without adding controls to the reference screens.
                hit(
                    ui,
                    box_at(r.left() + 20.0, version_y, 141.0, version_height),
                    "version-info",
                    true,
                )
                .context_menu(|ui| {
                    if ui.button("Открыть данные").clicked() {
                        let _ = platform::open_folder(&self.layout.data);
                        ui.close_menu();
                    }
                });
                if update.available {
                    let rect = box_at(r.left() + 20.0, bottom - 30.0, 165.0, 30.0);
                    let response = hit(ui, rect, "update", self.visual_enabled());
                    let color = if update.critical { RED } else { ORANGE };
                    ui.painter().rect_filled(
                        rect,
                        7,
                        color.gamma_multiply(if response.hovered() { 0.14 } else { 0.08 }),
                    );
                    if update.critical {
                        let c = egui::pos2(rect.left() + 16.0, rect.center().y);
                        ui.painter().circle_filled(c, 8.0, color);
                        ui.painter().text(
                            c,
                            egui::Align2::CENTER_CENTER,
                            "!",
                            font(11.0, true),
                            C::WHITE,
                        );
                    } else {
                        svg(
                            ui,
                            box_at(rect.left() + 8.0, rect.top() + 7.0, 16.0, 16.0),
                            egui::include_image!("../../assets/icon_refresh.svg"),
                            color,
                        );
                    }
                    line(
                        ui,
                        box_at(rect.left() + 31.0, rect.top(), 126.0, 30.0),
                        &format!("Обновить до {}", update.latest),
                        11.0,
                        true,
                        color,
                    );
                    if response.clicked() {
                        self.link(RELEASES);
                    }
                } else {
                    let rect = box_at(r.right() - 44.0, version_y, 24.0, 24.0);
                    let response = hit(ui, rect, "release", self.visual_enabled());
                    if response.hovered() {
                        ui.painter()
                            .circle_filled(rect.center(), 12.0, C::from_rgb(238, 241, 245));
                    }
                    svg(
                        ui,
                        Rect::from_center_size(rect.center(), Vec2::splat(17.0)),
                        egui::include_image!("../../assets/icon_external_link.svg"),
                        MUTED,
                    );
                    if response.clicked() {
                        self.link(RELEASES);
                    }
                }
            });
    }
    fn checking(&self) -> bool {
        self.state.checks.is_empty()
    }
    fn stopping(&self) -> bool {
        self.state.closing
            || self
                .state
                .components
                .iter()
                .any(|c| c.status == Status::Stopping)
    }
    fn failed(&self) -> bool {
        self.state.error.is_some()
            || self
                .state
                .components
                .iter()
                .any(|c| c.status == Status::Failed)
    }
    fn summary(&self) -> (&'static str, String, C) {
        let count = self
            .state
            .components
            .iter()
            .filter(|c| c.status == Status::Running)
            .count();
        if self.checking() {
            return ("Проверяем окружение", "Проверка окружения".into(), BLUE);
        }
        if !self.state.ready {
            return (
                "Нужна настройка",
                "Окружение не готово".into(),
                C::from_rgb(245, 124, 0),
            );
        }
        if self.stopping() {
            return (
                "Останавливаем сервисы",
                "Остановка компонентов".into(),
                C::from_rgb(245, 124, 0),
            );
        }
        if self.state.busy
            && self
                .state
                .components
                .iter()
                .any(|c| c.status == Status::Starting)
        {
            return ("Запускаем сервисы", "Запуск компонентов".into(), BLUE);
        }
        if self.failed() {
            return ("Нужна проверка", "Есть ошибка".into(), RED);
        }
        if self.state.running() {
            return ("Сервисы работают", format!("{count} из 5 работают"), GREEN);
        }
        if count > 0 {
            return (
                "Часть сервисов работает",
                format!("{count} из 5 работают"),
                C::from_rgb(245, 124, 0),
            );
        }
        ("Готов к запуску", "Все остановлены".into(), STOPPED)
    }
    fn main_page(&mut self, ui: &mut egui::Ui) {
        let r = ui.max_rect();
        let hero = box_at(r.left(), r.top(), r.width(), 126.0);
        ui.painter().rect_filled(hero, 14, SURFACE);
        let (headline, summary, summary_color) = self.summary();
        let any_running = self
            .state
            .components
            .iter()
            .any(|c| c.status == Status::Running);
        let secondary = any_running && !self.visual_busy() && !self.failed();
        let supporting = if self.checking() {
            "Ищем Docker, Node.js, Chrome и конфигурацию".into()
        } else if self.stopping() {
            "Корректно завершаем Docker Compose project".into()
        } else if self.state.busy
            && self
                .state
                .components
                .iter()
                .any(|c| c.status == Status::Starting)
        {
            "Подготавливаем и запускаем Backend, Frontend и PostgreSQL".into()
        } else if self.failed() {
            self.state.error.clone().unwrap_or_else(|| {
                self.state
                    .components
                    .iter()
                    .find(|c| c.status == Status::Failed)
                    .map(|c| c.details.clone())
                    .unwrap_or_default()
            })
        } else if !self.state.ready {
            self.state.message.clone()
        } else if self.state.running() {
            if self.state.auth.signed_in {
                "Все компоненты доступны и готовы к работе".into()
            } else {
                self.state.auth.message.clone()
            }
        } else if any_running {
            format!(
                "{} из 5 компонентов уже работают",
                self.state
                    .components
                    .iter()
                    .filter(|c| c.status == Status::Running)
                    .count()
            )
        } else {
            "Все компоненты сейчас остановлены".into()
        };
        let x = hero.left() + 28.0;
        let width = hero.width() - 28.0 * 2.0 - 210.0 - 32.0;
        // Material typography: 14 + 5 + 36 + 2 + 22, vertically centred.
        let text_y = hero.center().y
            - 39.5
            - if text_width(ui, &supporting, 15.0, false) > width {
                11.0
            } else {
                0.0
            };
        line(
            ui,
            box_at(x, text_y, width, 14.0),
            "ЛОКАЛЬНОЕ ОКРУЖЕНИЕ",
            11.0,
            true,
            MUTED,
        );
        line(
            ui,
            box_at(x, text_y + 19.0, width, 36.0),
            headline,
            30.0,
            true,
            C::BLACK,
        );
        paragraph(
            ui,
            box_at(x, text_y + 57.0, width, 44.0),
            &supporting,
            15.0,
            22.0,
            2,
            SECONDARY,
        );
        hit(
            ui,
            box_at(x, text_y + 57.0, width, 44.0),
            "hero-reason",
            false,
        );
        let button = box_at(
            hero.right() - 28.0 - 210.0,
            hero.center().y - if secondary { 41.0 } else { 26.0 },
            210.0,
            52.0,
        );
        let checking = self.checking();
        let stopping = self.stopping();
        let label = if checking {
            "Проверяем…"
        } else if !self.state.ready {
            "Проверить снова"
        } else if stopping {
            "Остановка…"
        } else if self.state.busy
            && self
                .state
                .components
                .iter()
                .any(|c| c.status == Status::Starting)
        {
            "Запускаем…"
        } else if self.failed() {
            "Остановить"
        } else if self.state.running() {
            "Открыть HH Clicker"
        } else {
            "Запустить"
        };
        let icon = if !self.state.ready {
            egui::include_image!("../../assets/icon_refresh.svg")
        } else if self.state.running() {
            egui::include_image!("../../assets/icon_external_link.svg")
        } else if stopping {
            egui::include_image!("../../assets/icon_player_stop.svg")
        } else {
            egui::include_image!("../../assets/icon_player_play.svg")
        };
        if action_button(
            ui,
            button,
            "primary",
            label,
            Some(icon),
            !self.visual_busy() && !checking && self.visual_enabled(),
            true,
        ) {
            if !self.state.ready {
                self.settings = true;
                self.send(Action::Refresh);
            } else if self.failed() {
                self.send(Action::Stop);
            } else if self.state.running() {
                self.link(&self.state.config.url(2));
            } else {
                self.send(Action::Start);
            }
        }
        if secondary {
            let rect = box_at(button.left(), button.bottom() + 5.0, 210.0, 25.0);
            let response = hit(ui, rect, "stop", self.visual_enabled());
            let color = if response.hovered() {
                ORANGE
            } else {
                SECONDARY
            };
            let width = text_width(ui, "Остановить сервисы", 12.0, true) + 20.0;
            let x = rect.center().x - width / 2.0;
            svg(
                ui,
                box_at(x, rect.center().y - 7.0, 14.0, 14.0),
                egui::include_image!("../../assets/icon_player_stop.svg"),
                color,
            );
            line(
                ui,
                box_at(x + 20.0, rect.top(), width - 20.0, 25.0),
                "Остановить сервисы",
                12.0,
                true,
                color,
            );
            if response.clicked() {
                self.send(Action::Stop);
            }
        }
        let panel = Rect::from_min_max(egui::pos2(r.left(), hero.bottom() + 14.0), r.max);
        ui.painter().rect_filled(panel, 14, SURFACE);
        let inner = panel.shrink2(egui::vec2(22.0, 18.0));
        line(
            ui,
            box_at(inner.left(), inner.top(), 300.0, 27.0),
            "Компоненты",
            21.0,
            true,
            C::BLACK,
        );
        let sw = text_width(ui, &summary, 12.0, false);
        let sx = inner.right() - sw;
        ui.painter().circle_filled(
            egui::pos2(sx - 10.5, inner.top() + 13.5),
            3.5,
            summary_color,
        );
        line(
            ui,
            box_at(sx, inner.top(), sw, 27.0),
            &summary,
            12.0,
            false,
            MUTED,
        );
        let header_y = inner.top() + 39.0;
        rule(ui, inner.left(), header_y, inner.width());
        let row_x = inner.left() + 5.0;
        let row_width = inner.width() - 10.0;
        let col1 = (row_width - 135.0) * 1.2 / 2.35;
        let col2 = (row_width - 135.0) * 1.15 / 2.35;
        for (x, w, label) in [
            (row_x, col1, "КОМПОНЕНТ"),
            (row_x + col1, col2, "АДРЕС / РАСПОЛОЖЕНИЕ"),
            (row_x + col1 + col2, 135.0, "СОСТОЯНИЕ"),
        ] {
            line(
                ui,
                box_at(x, header_y + 1.0, w, 34.0),
                label,
                11.0,
                true,
                MUTED,
            );
        }
        let rows_y = header_y + 35.0;
        rule(ui, inner.left(), rows_y, inner.width());
        let row_height = (inner.bottom() - rows_y - 5.0) / 5.0;
        for i in 0..5 {
            let top = rows_y + 1.0 + i as f32 * (row_height + 1.0);
            let c = self.state.components[i].clone();
            let auth_warning = i == 3 && c.status == Status::Running && !self.state.auth.signed_in;
            let detail = if c.status == Status::Failed {
                Some((c.details.clone(), RED, 6.0))
            } else if i == 3 && c.status == Status::Running {
                let lines = auth_detail_lines(&self.state.auth.details);
                let text = if self.state.auth.signed_in {
                    let mut summary = lines
                        .first()
                        .filter(|s| s.contains(": осталось "))
                        .cloned()
                        .unwrap_or_default();
                    if let Some(checked) = lines.iter().find_map(|s| s.strip_prefix("Проверено: "))
                    {
                        if !summary.is_empty() {
                            summary.push_str(&format!(" · проверено {checked}"));
                        }
                    }
                    summary
                } else {
                    self.state.auth.message.clone()
                };
                if text.is_empty() {
                    None
                } else {
                    Some((text, MUTED, 0.0))
                }
            } else {
                None
            };
            let detail_height = detail.as_ref().map(|(_, _, gap)| 18.0 + gap).unwrap_or(0.0);
            let center_y = top + row_height / 2.0 - detail_height / 2.0;
            let icon = match i {
                0 | 1 => egui::include_image!("../../assets/icon_server.svg"),
                2 => egui::include_image!("../../assets/icon_browser.svg"),
                3 => egui::include_image!("../../assets/icon_sparkles.svg"),
                _ => egui::include_image!("../../assets/icon_world.svg"),
            };
            let rect = box_at(row_x, center_y - 19.0, 38.0, 38.0);
            ui.painter()
                .rect_filled(rect, 10, C::from_rgb(252, 238, 232));
            svg(ui, rect.shrink(9.0), icon, ORANGE);
            line(
                ui,
                box_at(row_x + 50.0, center_y - 20.0, col1 - 50.0, 22.0),
                NAMES[i],
                15.0,
                true,
                C::BLACK,
            );
            line(
                ui,
                box_at(row_x + 50.0, center_y + 2.0, col1 - 50.0, 18.0),
                DESCRIPTIONS[i],
                12.0,
                false,
                MUTED,
            );
            let location = match i {
                0 => "Внутри Docker".into(),
                1..=3 => self.state.config.url(i),
                _ => "Локальный процесс".into(),
            };
            line(
                ui,
                box_at(row_x + col1, center_y - 10.5, col2, 21.0),
                &location,
                14.0,
                false,
                SECONDARY,
            );
            let label = if auth_warning {
                if self.state.auth.pending {
                    "Вход"
                } else if self.state.auth.message.starts_with("Проверяем") {
                    "Проверка"
                } else if self.state.auth.message.contains("Не удалось") {
                    "Ошибка"
                } else {
                    "Авторизация"
                }
            } else {
                c.status.label()
            };
            let color = if auth_warning {
                ORANGE
            } else {
                match c.status {
                    Status::Stopped => STOPPED,
                    Status::Starting => BLUE,
                    Status::Stopping => C::from_rgb(245, 124, 0),
                    _ => status_color(c.status),
                }
            };
            let status_x = row_x + col1 + col2;
            ui.painter()
                .circle_filled(egui::pos2(status_x + 3.5, center_y), 3.5, color);
            let badge = box_at(
                status_x + 15.0,
                center_y - 14.0,
                text_width(ui, label, 12.0, false) + 20.0,
                28.0,
            );
            ui.painter()
                .rect_filled(badge, 7, C::from_rgb(243, 245, 248));
            line(
                ui,
                badge.shrink2(egui::vec2(10.0, 0.0)),
                label,
                12.0,
                false,
                SECONDARY,
            );
            if let Some((reason, color, gap)) = detail {
                let rect = box_at(row_x + 50.0, center_y + 20.0 + gap, row_width - 50.0, 36.0);
                paragraph(ui, rect, &reason, 12.0, 18.0, 2, color);
                hit(ui, rect, ("component-detail", i), false);
            }
            if i < 4 {
                rule(ui, inner.left(), top + row_height, inner.width());
            }
        }
    }

    fn display_checks(&self) -> Vec<hh_clicker_launcher::runtime::Check> {
        use hh_clicker_launcher::runtime::Check;
        let raw = &self.state.checks;
        if raw.is_empty() || raw.iter().any(|c| c.name == "Автономный просмотр") {
            return raw.clone();
        }
        let find = |name: &str| raw.iter().find(|c| c.name == name).cloned();
        let mut checks = Vec::new();
        if let Some(mut c) = find("Виртуализация") {
            c.name = "Аппаратная виртуализация".into();
            if c.passed {
                c.message = "Включена в BIOS/UEFI, процессор поддерживает SLAT".into();
            }
            checks.push(c);
        }
        if let Some(mut c) = find("WSL") {
            c.name = "WSL 2".into();
            if let Some(default) = find("WSL 2") {
                c.passed &= default.passed;
                c.message = format!(
                    "{}, {}",
                    c.message,
                    if default.passed {
                        "версия по умолчанию — WSL 2"
                    } else {
                        &default.message
                    }
                );
            }
            checks.push(c);
        }
        for name in [
            "Docker Desktop",
            "Docker CLI",
            "Docker Compose V2",
            "Docker Engine",
        ] {
            if let Some(mut c) = find(name) {
                if c.passed {
                    c.message = match name {
                        "Docker Desktop" => format!("Версия {}", c.message),
                        "Docker CLI" => format!(
                            "Docker {}",
                            c.message
                                .trim_start_matches("Docker version ")
                                .split(',')
                                .next()
                                .unwrap_or(&c.message)
                        ),
                        "Docker Compose V2" => format!("Compose {}", c.message.trim()),
                        "Docker Engine" => format!(
                            "Engine {}: Linux containers на WSL 2",
                            c.message.rsplit(' ').next().unwrap_or("")
                        ),
                        _ => c.message,
                    };
                }
                if name == "Docker Compose V2" {
                    c.name = "Docker Compose".into();
                }
                if name == "Docker Engine" {
                    if let Some(endpoint) = find("Docker endpoint") {
                        if !endpoint.passed {
                            c.passed = false;
                            c.message = endpoint.message;
                        }
                    }
                }
                checks.push(c);
            }
        }
        if let Some(mut c) = find("Файлы приложения") {
            c.name = "Рабочая папка".into();
            if c.passed {
                c.message = self.layout.payload.display().to_string();
            }
            checks.push(c);
        }
        for name in ["Node.js", "Google Chrome"] {
            if let Some(mut c) = find(name) {
                if c.passed {
                    c.message = if name == "Node.js" {
                        format!("Node.js {}", c.message.trim().trim_start_matches('v'))
                    } else {
                        "Google Chrome установлен".into()
                    };
                }
                checks.push(c);
            }
        }
        let config = self.layout.data.join("config.json");
        for name in ["Настройки инфраструктуры", "Настройки Generation Agent"]
        {
            checks.push(Check {
                name: name.into(),
                passed: config.is_file(),
                message: config.display().to_string(),
                required: true,
            });
        }
        if let Some(mut c) = find("Локальные порты") {
            if c.passed {
                c.message = format!(
                    "Frontend {}, Backend {}, Generation Agent {}",
                    self.state.config.frontend_port,
                    self.state.config.backend_port,
                    self.state.config.generation_port
                );
            }
            if let Some(owner) = find("Принадлежность ресурсов") {
                if !owner.passed {
                    c.passed = false;
                    c.message = owner.message;
                }
            }
            checks.push(c);
        }
        if let Some(c) = find("Browser Agent") {
            checks.push(c);
        }
        for c in raw {
            if c.name == "Перезагрузка Windows" {
                checks.push(c.clone());
            }
        }
        checks
    }
    fn settings_page(&mut self, ui: &mut egui::Ui) {
        let r = ui.max_rect();
        ui.painter().rect_filled(r, 14, SURFACE);
        let inner = r.shrink2(egui::vec2(28.0, 24.0));
        let x = inner.left();
        let w = inner.width();
        let checks = self.display_checks();
        let checking = self.checking();
        let title = if !checking && !self.state.ready {
            "Подготовка компьютера"
        } else {
            "Проверка окружения"
        };
        let reboot = self
            .state
            .checks
            .iter()
            .find(|c| c.name == "Перезагрузка Windows" && !c.passed)
            .cloned();
        let subtitle = if let Some(c) = &reboot {
            c.message.as_str()
        } else if self.state.ready || checking {
            "Все зависимости для локального запуска доступны"
        } else {
            "Выполните обязательные пункты и запустите проверку снова"
        };
        let refresh_label = if checking {
            "Проверяем…"
        } else {
            "Проверить снова"
        };
        let refresh_width = 28.0 + 18.0 + 8.0 + text_width(ui, refresh_label, 14.0, true);
        let mut refresh = box_at(
            inner.right() - refresh_width,
            inner.top() + 8.0,
            refresh_width,
            40.0,
        );
        let count = format!(
            "{} из {}",
            checks.iter().filter(|c| c.passed).count(),
            checks.len()
        );
        let cw = if checks.is_empty() {
            0.0
        } else {
            text_width(ui, &count, 12.0, false)
        };
        let text_w = w - refresh_width - cw - 24.0;
        let extra_header = if text_width(ui, subtitle, 14.0, false) > text_w {
            21.0
        } else {
            0.0
        };
        refresh = refresh.translate(egui::vec2(0.0, extra_header / 2.0));
        line(
            ui,
            box_at(x, inner.top(), text_w, 36.0),
            title,
            30.0,
            true,
            C::BLACK,
        );
        paragraph(
            ui,
            box_at(x, inner.top() + 39.0, text_w, 42.0),
            subtitle,
            14.0,
            21.0,
            2,
            SECONDARY,
        );
        if !checks.is_empty() {
            line(
                ui,
                box_at(refresh.left() - 12.0 - cw, refresh.top(), cw, 40.0),
                &count,
                12.0,
                false,
                MUTED,
            );
        }
        let enabled = !self.state.busy && !checking && self.visual_enabled();
        let response = hit(ui, refresh, "refresh", enabled);
        if !enabled || response.hovered() {
            ui.painter().rect_filled(
                refresh,
                10,
                if !enabled {
                    C::from_rgb(242, 244, 247)
                } else {
                    C::from_rgb(255, 240, 231)
                },
            );
        }
        let color = if enabled { ORANGE } else { MUTED };
        svg(
            ui,
            box_at(refresh.left() + 14.0, refresh.center().y - 9.0, 18.0, 18.0),
            egui::include_image!("../../assets/icon_refresh.svg"),
            color,
        );
        line(
            ui,
            box_at(
                refresh.left() + 40.0,
                refresh.top(),
                refresh.width() - 54.0,
                40.0,
            ),
            refresh_label,
            14.0,
            true,
            color,
        );
        if response.clicked() {
            self.send(Action::Refresh);
        }
        let mut y = inner.top()
            + 78.0
            + if text_width(ui, subtitle, 14.0, false) > text_w {
                21.0
            } else {
                0.0
            };
        rule(ui, x, y, w);
        y += 1.0;
        if let Some(c) = self.state.checks.iter().find(|c| {
            !c.passed
                && c.required
                && (c.name == "Виртуализация" || c.name == "Перезагрузка Windows")
        }) {
            let (title, description) = if c.name == "Виртуализация" {
                ("Сначала включите виртуализацию","Включите Intel VT-x или AMD-V (SVM) в BIOS/UEFI и запустите проверку снова. Лаунчер не меняет настройки BIOS")
            } else {
                ("Перезагрузите компьютер", c.message.as_str())
            };
            line(
                ui,
                box_at(x + 6.0, y + 13.0, w - 12.0, 21.0),
                title,
                14.0,
                true,
                C::from_rgb(245, 124, 0),
            );
            let h = paragraph(
                ui,
                box_at(x + 6.0, y + 33.0, w - 12.0, 54.0),
                description,
                12.0,
                18.0,
                3,
                MUTED,
            );
            y += 46.0 + h;
            rule(ui, x, y, w);
            y += 1.0;
        } else if let Some(action) = self
            .preparation
            .or_else(|| self.suggested_preparation().map(|v| v.0))
        {
            y = self.setup_section(ui, box_at(x, y, w, 160.0), Some(action));
            rule(ui, x, y, w);
            y += 1.0;
        } else if self
            .state
            .checks
            .iter()
            .any(|c| c.name == "Локальные порты" && !c.passed)
        {
            y = self.setup_section(ui, box_at(x, y, w, 130.0), None);
            rule(ui, x, y, w);
            y += 1.0;
        }
        if self.state.ready {
            y = self.auth_section(ui, box_at(x, y, w, 200.0));
            rule(ui, x, y, w);
            y += 1.0;
        }
        let scroll_rect = Rect::from_min_max(egui::pos2(x, y), inner.max);
        let mut scroll_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(scroll_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        scroll_ui.spacing_mut().item_spacing.y = 0.0;
        scroll_ui.spacing_mut().scroll.handle_min_length = 32.0;
        scroll_ui.visuals_mut().extreme_bg_color = C::TRANSPARENT;
        scroll_ui.visuals_mut().widgets.inactive.fg_stroke.color = MUTED.gamma_multiply(0.32);
        scroll_ui.visuals_mut().widgets.hovered.fg_stroke.color = ORANGE.gamma_multiply(0.62);
        scroll_ui.visuals_mut().widgets.active.fg_stroke.color = ORANGE.gamma_multiply(0.62);
        if checks.is_empty() {
            scroll_ui.painter().text(
                scroll_rect.center(),
                egui::Align2::CENTER_CENTER,
                "Проверяем Docker, Node.js, Chrome и файлы конфигурации…",
                font(14.0, false),
                MUTED,
            );
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("checks")
            .auto_shrink([false, false])
            .max_height(scroll_rect.height())
            .show(&mut scroll_ui, |ui| {
                for (index, c) in checks.iter().enumerate() {
                    let (row, _) =
                        ui.allocate_exact_size(egui::vec2(w - 11.0, 50.0), egui::Sense::hover());
                    let accent = if c.passed {
                        GREEN
                    } else if c.required {
                        RED
                    } else {
                        C::from_rgb(245, 124, 0)
                    };
                    let soft = if c.passed {
                        C::from_rgb(234, 248, 238)
                    } else if c.required {
                        C::from_rgb(255, 240, 240)
                    } else {
                        C::from_rgb(255, 242, 230)
                    };
                    let center = egui::pos2(row.left() + 21.0, row.center().y);
                    ui.painter().circle_filled(center, 15.0, soft);
                    ui.painter().text(
                        center,
                        egui::Align2::CENTER_CENTER,
                        if c.passed { "✓" } else { "!" },
                        font(14.0, true),
                        accent,
                    );
                    let label = if c.passed {
                        "Готово"
                    } else if c.required {
                        "Требуется"
                    } else {
                        "Можно пропустить"
                    };
                    let lw = text_width(ui, label, 12.0, true);
                    let tx = row.left() + 49.0;
                    let tw = row.right() - 6.0 - lw - 12.0 - tx;
                    line(
                        ui,
                        box_at(tx, row.center().y - 19.5, tw, 21.0),
                        &c.name,
                        14.0,
                        true,
                        C::BLACK,
                    );
                    if !c.required {
                        let offset = text_width(ui, &c.name, 14.0, true) + 8.0;
                        line(
                            ui,
                            box_at(
                                tx + offset,
                                row.center().y - 19.5,
                                (tw - offset).max(0.0),
                                21.0,
                            ),
                            "НЕОБЯЗАТЕЛЬНО",
                            11.0,
                            true,
                            MUTED,
                        );
                    }
                    line(
                        ui,
                        box_at(tx, row.center().y + 1.5, tw, 18.0),
                        &c.message,
                        12.0,
                        false,
                        MUTED,
                    );
                    hit(
                        ui,
                        box_at(tx, row.center().y + 1.5, tw, 18.0),
                        ("check-detail", index),
                        false,
                    );
                    line(
                        ui,
                        box_at(row.right() - 6.0 - lw, row.top(), lw, 50.0),
                        label,
                        12.0,
                        true,
                        accent,
                    );
                    if index + 1 < checks.len() {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(w - 11.0, 1.0), egui::Sense::hover());
                        rule(ui, rect.left(), rect.top(), rect.width());
                    }
                }
            });
    }
    fn auth_section(&mut self, ui: &mut egui::Ui, r: Rect) -> f32 {
        let a = self.state.auth.clone();
        let active = self.state.components[3].status == Status::Running;
        let failed = a.message.contains("Не удалось");
        let checking =
            active && !a.signed_in && !a.pending && !failed && a.message.contains("Проверяем");
        let (title, color) = if a.signed_in {
            ("Codex подключён", GREEN)
        } else if a.pending {
            ("Вход через ChatGPT", ORANGE)
        } else if failed {
            ("Не удалось проверить Codex", RED)
        } else if checking {
            ("Проверяем Codex", TEXT)
        } else {
            ("Авторизация Codex", TEXT)
        };
        let label = if a.signed_in {
            "Выйти из Codex"
        } else if a.pending {
            "Открыть страницу"
        } else if failed {
            "Повторить"
        } else if checking {
            "Проверяем…"
        } else {
            "Войти через ChatGPT"
        };
        let has_button = active || a.signed_in || a.pending;
        let cancel_width = if a.pending {
            8.0 + text_width(ui, "Отменить", 14.0, true) + 26.0
        } else {
            0.0
        };
        let bw = if has_button {
            text_width(ui, label, 14.0, true) + if failed { 26.0 } else { 40.0 }
        } else {
            0.0
        };
        let tx = r.left() + 6.0;
        let tw = r.width() - 12.0 - bw - cancel_width - if has_button { 16.0 } else { 0.0 };
        let details = auth_detail_lines(&a.details);
        let mut content_height = 36.0;
        if !self.state.runtime_info.is_empty() {
            content_height += 18.0;
        }
        if a.signed_in {
            content_height += details.len() as f32 * 18.0;
        }
        if a.pending && a.code.is_some() {
            content_height += 43.0;
        }
        let height = content_height + 28.0;
        let top = r.top() + 14.0;
        line(ui, box_at(tx, top, tw, 21.0), title, 14.0, true, color);
        let message = if a.pending && a.code.is_some() {
            "Введите код на открывшейся странице"
        } else if a.message == "Codex подключён · chatgpt" {
            "Выполнен вход через ChatGPT"
        } else if a.message == "Codex подключён · api_key" {
            "Выполнен вход с API-ключом"
        } else {
            &a.message
        };
        line(
            ui,
            box_at(tx, top + 18.0, tw, 18.0),
            message,
            12.0,
            false,
            MUTED,
        );
        hit(ui, box_at(tx, top + 18.0, tw, 18.0), "auth-message", false);
        let mut y = top + 36.0;
        if !self.state.runtime_info.is_empty() {
            line(
                ui,
                box_at(tx, y, tw, 18.0),
                &self.state.runtime_info,
                12.0,
                false,
                SECONDARY,
            );
            y += 18.0;
        }
        if a.signed_in {
            for detail in &details {
                line(
                    ui,
                    box_at(tx, y, tw, 18.0),
                    detail,
                    12.0,
                    false,
                    if detail.starts_with("Проверено") {
                        MUTED
                    } else {
                        SECONDARY
                    },
                );
                y += 18.0;
            }
        }
        if a.pending {
            if let Some(code) = a.code {
                y += 7.0;
                let codew = text_width(ui, &code, 14.0, true) + 20.0;
                let code_rect = box_at(tx, y, codew, 34.0);
                ui.painter()
                    .rect_filled(code_rect, 7, C::from_rgb(241, 243, 246));
                let mut code_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(code_rect.shrink2(egui::vec2(10.0, 7.0)))
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                code_ui.add(
                    egui::Label::new(RichText::new(&code).font(font(14.0, true)).color(TEXT))
                        .selectable(true),
                );
                let copy = if self.copied_code {
                    "Скопировано"
                } else {
                    "Копировать"
                };
                if text_button(
                    ui,
                    box_at(
                        tx + codew + 8.0,
                        y,
                        text_width(ui, copy, 14.0, true) + 26.0,
                        38.0,
                    ),
                    "copy-code",
                    copy,
                    true,
                ) {
                    ui.ctx().copy_text(code);
                    self.copied_code = true;
                }
            }
        }
        if has_button {
            let rect = box_at(
                r.right() - 6.0 - bw - cancel_width,
                r.top() + (height - 42.0) / 2.0,
                bw,
                42.0,
            );
            let enabled = !self.state.busy && !checking && self.visual_enabled();
            let clicked = if failed {
                text_button(ui, rect, "auth-action", label, enabled)
            } else {
                action_button(ui, rect, "auth-action", label, None, enabled, false)
            };
            if clicked {
                if a.signed_in {
                    self.send(Action::Auth("/auth/logout"));
                } else if a.pending {
                    if let Some(url) = a.url {
                        self.link(&url);
                    }
                } else if failed {
                    self.send(Action::Auth("/auth/status/refresh"));
                } else {
                    self.copied_code = false;
                    self.send(Action::Auth("/auth/login/start"));
                }
            }
            if a.pending
                && text_button(
                    ui,
                    box_at(rect.right() + 8.0, rect.top(), cancel_width - 8.0, 42.0),
                    "cancel-login",
                    "Отменить",
                    enabled,
                )
            {
                self.send(Action::Auth("/auth/login/cancel"));
            }
        }
        r.top() + height
    }
    fn setup_section(&mut self, ui: &mut egui::Ui, r: Rect, action: Option<Preparation>) -> f32 {
        let (title,description,label,notice)=match action {
            Some(Preparation::Node)=>("Скачать локальный Node.js","Системный Node.js отсутствует или не подходит. Лаунчер скачает portable runtime только для текущего пользователя, не изменяя PATH","Скачать Node.js",None),
            Some(Preparation::Chrome)=>("Установить Google Chrome","Лаунчер установит официальный Google Chrome для Browser Agent. После установки один раз откройте Chrome вручную и завершите первоначальную настройку.","Принять и установить Chrome",Some(("Скачивая Chrome, вы принимаете условия использования Google Chrome","https://www.google.com/chrome/terms/"))),
            Some(Preparation::DockerInstall)=>("Установить Docker Desktop","Лаунчер скачает официальный установщик, проверит его подпись и установит Docker Desktop для текущего пользователя с backend WSL 2 и Linux containers","Принять и установить Docker",Some(("Нажимая «Принять и установить», вы принимаете Docker Subscription Service Agreement","https://www.docker.com/legal/docker-subscription-service-agreement/"))),
            Some(Preparation::DockerStart)=>("Запустить Docker Desktop","Docker Desktop установлен, но не запущен. Лаунчер запустит его и дождётся готовности Engine","Запустить Docker",None),
            Some(Preparation::WslDefault)=>("Включить режим WSL 2","Версия по умолчанию должна быть WSL 2. Лаунчер переключит её; нужны права администратора","Включить WSL 2",None),
            Some(Preparation::WslUpdate)=>("Обновить WSL","Установленная версия WSL слишком старая. Лаунчер обновит её; нужны права администратора и перезагрузка","Обновить WSL",None),
            Some(Preparation::Wsl)=>("Установить WSL 2","Лаунчер включит компоненты Windows и установит WSL 2 без дистрибутива Linux. Нужны права администратора и перезагрузка","Установить WSL 2",None),
            Some(Preparation::Browser)=>("Подготовка Browser Agent","Подготовьте отсутствующие файлы и зависимости агента в каталоге этой копии.","Подготовить",None),
            None=>("Заняты локальные порты","","Выбрать свободные порты",None),
        };
        let own_preparation = self.preparation.is_some();
        let awaiting_chrome = self.chrome_notice && matches!(action, Some(Preparation::Chrome));
        let success = own_preparation && !self.state.busy && self.state.error.is_none();
        let failed = own_preparation && self.state.error.is_some();
        let (preparing_title, failed_title, ready_title) = match action {
            Some(Preparation::Node) => (
                "Подготовка Node.js",
                "Не удалось подготовить Node.js",
                "Node.js готов",
            ),
            Some(Preparation::DockerInstall | Preparation::DockerStart) => (
                "Подготовка Docker Desktop",
                "Не удалось подготовить Docker",
                "Docker Desktop готов",
            ),
            Some(Preparation::Chrome) => (
                "Подготовка Google Chrome",
                "Не удалось установить Google Chrome",
                "Google Chrome готов",
            ),
            Some(Preparation::Browser) => (
                "Подготовка Browser Agent",
                "Не удалось подготовить Browser Agent",
                "Browser Agent готов",
            ),
            _ => (
                "Подготовка WSL 2",
                "Не удалось подготовить WSL 2",
                "WSL 2 настроен",
            ),
        };
        let title: String = if awaiting_chrome {
            "Google Chrome установлен".into()
        } else if own_preparation {
            if self.state.busy {
                preparing_title.into()
            } else if failed {
                failed_title.into()
            } else {
                ready_title.into()
            }
        } else {
            title.into()
        };
        let description = if awaiting_chrome {
            "Откройте Chrome вручную и завершите первоначальную настройку, включая выбор поисковой системы, если он появится. Затем нажмите «Продолжить»."
        } else if own_preparation {
            self.state.error.as_deref().unwrap_or(&self.state.message)
        } else if action.is_none() {
            self.state
                .checks
                .iter()
                .find(|c| c.name == "Локальные порты")
                .map(|c| c.message.as_str())
                .unwrap_or(description)
        } else if self.state.busy {
            &self.state.message
        } else {
            description
        };
        let label = if awaiting_chrome {
            "Продолжить"
        } else if self.state.busy {
            "Подготовка…"
        } else if failed {
            "Повторить"
        } else {
            label
        };
        let bw = if success && !awaiting_chrome {
            0.0
        } else {
            text_width(ui, label, 14.0, true) + 40.0
        };
        let tx = r.left() + 6.0;
        let tw = r.width() - 12.0 - bw - 16.0;
        let top = r.top() + 15.0;
        line(
            ui,
            box_at(tx, top - 2.0, tw, 21.0),
            &title,
            14.0,
            true,
            if action.is_none() || failed {
                RED
            } else if success {
                GREEN
            } else {
                TEXT
            },
        );
        let dh = paragraph(
            ui,
            box_at(
                tx,
                top + 18.0,
                tw,
                if awaiting_chrome { 72.0 } else { 54.0 },
            ),
            description,
            12.0,
            18.0,
            if awaiting_chrome { 4 } else { 3 },
            MUTED,
        );
        let mut height = 19.0 + dh;
        if let Some((text, url)) = notice.filter(|_| !own_preparation) {
            let nh = paragraph(
                ui,
                box_at(tx, top + height + 4.0, tw, 36.0),
                text,
                12.0,
                18.0,
                2,
                SECONDARY,
            );
            height += 4.0 + nh + 1.0;
            let rect = box_at(tx, top + height, tw, 18.0);
            line(ui, rect, url, 12.0, true, ORANGE);
            if hit(ui, rect, "terms", self.visual_enabled()).clicked() {
                self.link(url);
            }
            height += 18.0;
        }
        let height = height + 27.0;
        if (!success || awaiting_chrome)
            && action_button(
                ui,
                box_at(
                    r.right() - 6.0 - bw,
                    r.top() + (height - 42.0) / 2.0,
                    bw,
                    42.0,
                ),
                "prepare",
                label,
                None,
                !self.state.busy && self.visual_enabled(),
                false,
            )
        {
            if awaiting_chrome {
                self.chrome_notice = false;
                self.preparation = None;
                if self.state.ready {
                    self.settings = false;
                }
            } else if let Some(action) = action {
                self.send(Action::Prepare(action));
            } else {
                self.send(Action::Ports);
            }
        }
        r.top() + height
    }
}

#[cfg(test)]
mod presentation_tests {
    #[test]
    fn chrome_setup_notice_stays_until_explicit_continue() {
        use super::*;
        let layout = Layout::new(
            std::env::temp_dir().join(uuid::Uuid::new_v4().to_string()),
            None,
        );
        let mut state = State::new(layout.load().unwrap());
        let mut settings = true;
        let mut preparation = Some(Preparation::Chrome);
        state.environment_revision = 1;
        state.ready = true;
        state.error = Some("installer failed".into());
        let mut notice =
            finish_environment_navigation(&mut settings, &mut preparation, 0, &state, false);
        assert!(!notice && settings);
        state.error = None;
        notice |= finish_environment_navigation(&mut settings, &mut preparation, 0, &state, false);
        assert!(notice && settings && matches!(preparation, Some(Preparation::Chrome)));
        state.message = "Периодическая проверка перезаписала сообщение".into();
        notice |= finish_environment_navigation(&mut settings, &mut preparation, 1, &state, false);
        assert!(notice && settings && matches!(preparation, Some(Preparation::Chrome)));
        state.environment_revision = 2; // Even an explicit refresh must not dismiss the notice.
        notice |= finish_environment_navigation(&mut settings, &mut preparation, 1, &state, false);
        assert!(notice && settings && matches!(preparation, Some(Preparation::Chrome)));
        std::fs::remove_dir_all(layout.root).unwrap();
    }
    #[test]
    fn completed_setup_and_refresh_advance_without_codex_login() {
        use super::*;
        let layout = Layout::new(
            std::env::temp_dir().join(uuid::Uuid::new_v4().to_string()),
            None,
        );
        let mut state = State::new(layout.load().unwrap());
        let mut settings = true;
        let mut preparation = Some(Preparation::Node);
        state.busy = true;
        state.environment_revision = 1;
        finish_environment_navigation(&mut settings, &mut preparation, 0, &state, false);
        assert!(preparation.is_some()); // A progress snapshot cannot end preparation.
        state.busy = false;
        state.ready = false;
        finish_environment_navigation(&mut settings, &mut preparation, 0, &state, false);
        assert!(settings);
        assert!(preparation.is_none()); // Next missing dependency is now selectable.
        preparation = Some(Preparation::Browser);
        state.ready = true;
        state.auth.signed_in = false;
        state.error = Some("download failed".into());
        finish_environment_navigation(&mut settings, &mut preparation, 0, &state, false);
        assert!(settings && preparation.is_some());
        state.error = None;
        finish_environment_navigation(&mut settings, &mut preparation, 1, &state, false);
        assert!(settings); // An unrelated Tick must preserve the user's navigation.
        state.environment_revision = 2;
        finish_environment_navigation(&mut settings, &mut preparation, 1, &state, false);
        assert!(!settings && preparation.is_none());
        assert!(!state.auth.signed_in);
        std::fs::remove_dir_all(layout.root).unwrap();
    }
    use super::*;
    #[test]
    fn fractional_font_sizes_are_distinct_and_cache_order_independent() {
        for dpi in [1.0, 1.25, 1.5] {
            let create_fonts = || {
                let mut definitions = egui::FontDefinitions::empty();
                definitions.font_data.insert(
                    "Inter".into(),
                    egui::FontData::from_static(include_bytes!("../../assets/inter_regular.ttf"))
                        .into(),
                );
                definitions
                    .families
                    .insert(egui::FontFamily::Proportional, vec!["Inter".into()]);
                egui::epaint::Fonts::new(dpi, 2048, definitions)
            };
            let fonts = create_fonts();
            let width = fonts.glyph_width(&font(14.0, false), 'M');
            let larger = fonts.glyph_width(&font(14.1, false), 'M');
            assert!(
                larger > width + 0.03,
                "fractional size collapsed at DPI {dpi}"
            );
            let reversed = create_fonts();
            assert_eq!(larger, reversed.glyph_width(&font(14.1, false), 'M'));
            assert_eq!(width, reversed.glyph_width(&font(14.0, false), 'M'));
        }
        // This also verifies Cargo uses our local renderer patch, not registry epaint.
        let coverage = egui::epaint::FontImage {
            size: [1, 1],
            pixels: vec![0.5],
        };
        assert_eq!(coverage.srgba_pixels(None).next().unwrap().a(), 128);
    }
    #[test]
    fn codex_limits_keep_usage_check_and_reset_separate() {
        let lines=auth_detail_lines("5 часов: осталось 82% · сброс 17.11 23:00\nНеделя: осталось 64% · сброс 23.11 12:00\nПроверено: 2026-11-17T18:15:00.123Z");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "5 часов: осталось 82% · Неделя: осталось 64%");
        assert_eq!(
            lines[1],
            format!(
                "Проверено: {}",
                local_checked_at("2026-11-17T18:15:00Z").unwrap()
            )
        );
        assert_eq!(
            lines[2],
            "Сброс — 5 часов: 17.11 23:00 · Неделя: 23.11 12:00"
        );
        assert!(!lines.iter().any(|s| s.contains('T') || s.contains('Z')));
        assert!(local_checked_at("invalid").is_none());
        assert!(local_checked_at("2026-99-17T18:15:00Z").is_none());
    }
}
