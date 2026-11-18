use crate::{
    auth::{self, Auth},
    config::{self, Config, Layout},
    process::{self, ManagedChild},
    updates::Update,
};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::PathBuf, process::Command, time::Duration};

pub const NAMES: [&str; 5] = [
    "PostgreSQL",
    "Backend",
    "Frontend",
    "Generation Agent",
    "Browser Agent",
];
pub const DESCRIPTIONS: [&str; 5] = [
    "База данных",
    "API и бизнес-логика",
    "Веб-интерфейс",
    "Генерация сопроводительных",
    "Автоматизация браузера",
];
const SERVICES: [&str; 4] = ["postgres", "backend", "frontend", "generation-agent"];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
    Unknown,
}
impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stopped => "Остановлен",
            Self::Starting => "Запускается",
            Self::Running => "Работает",
            Self::Stopping => "Остановка",
            Self::Failed => "Ошибка",
            Self::Unknown => "Нет связи",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Component {
    pub status: Status,
    pub details: String,
}
#[derive(Clone, Debug)]
pub struct Check {
    pub name: String,
    pub passed: bool,
    pub message: String,
    pub required: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Operation {
    #[default]
    Idle,
    Starting,
    Stopping,
    Other,
}
#[derive(Clone)]
pub struct State {
    pub revision: u64,
    pub components: Vec<Component>,
    pub checks: Vec<Check>,
    pub busy: bool,
    pub message: String,
    pub error: Option<String>,
    pub auth: Auth,
    pub runtime_info: String,
    pub update: Update,
    pub config: Config,
    pub ready: bool,
    pub closing: bool,
    pub operation: Operation,
    pub environment_revision: u64,
}
impl State {
    pub fn new(config: Config) -> Self {
        Self {
            revision: 0,
            components: (0..5)
                .map(|_| Component {
                    status: Status::Unknown,
                    details: "Состояние ещё не проверено".into(),
                })
                .collect(),
            checks: vec![],
            busy: false,
            message: "Проверяем окружение…".into(),
            error: None,
            auth: Auth {
                message: "Сначала запустите Generation Agent".into(),
                ..Default::default()
            },
            runtime_info: String::new(),
            update: Update::default(),
            config,
            ready: false,
            closing: false,
            operation: Operation::Idle,
            environment_revision: 0,
        }
    }
    pub fn running(&self) -> bool {
        self.components.iter().all(|c| c.status == Status::Running)
    }
    pub fn any_active(&self) -> bool {
        self.components.iter().any(|c| {
            matches!(
                c.status,
                Status::Running | Status::Starting | Status::Unknown
            )
        })
    }
    pub fn mark_stopping(&mut self) {
        self.operation = Operation::Stopping;
        for component in &mut self.components {
            if component.status != Status::Stopped {
                component.status = Status::Stopping;
            }
        }
    }
    pub fn apply_observation(&mut self, revision: u64, components: Vec<Component>) {
        // Stop owns its progression. A read-only inspection can still see live
        // containers while Compose is shutting them down; that is not a restart.
        if !self.busy || self.revision != revision || self.operation == Operation::Stopping {
            return;
        }
        for (i, observed) in components.into_iter().take(4).enumerate() {
            if self.components[i].status == Status::Starting
                && matches!(observed.status, Status::Stopped | Status::Failed)
            {
                continue;
            }
            self.components[i] = observed;
        }
    }
}

pub fn owned(container: &Value, instance: &str, project: &str, service: &str) -> bool {
    let l = &container["Config"]["Labels"];
    l["com.hh-clicker.instance-id"].as_str() == Some(instance)
        && l["com.docker.compose.project"].as_str() == Some(project)
        && l["com.docker.compose.service"].as_str() == Some(service)
        && l["com.hh-clicker.profile"].as_str() == Some("rust-portable")
}
pub fn container_status(c: &Value, health_required: bool) -> Component {
    let running = c["State"]["Running"].as_bool() == Some(true);
    let health = c["State"]["Health"]["Status"].as_str();
    let (status, details) = if !running {
        (
            Status::Stopped,
            format!(
                "Контейнер {}",
                c["State"]["Status"].as_str().unwrap_or("остановлен")
            ),
        )
    } else {
        match health {
            Some("healthy") => (Status::Running, "Health check пройден".into()),
            Some("starting") => (Status::Starting, "Ожидаем health check".into()),
            Some("unhealthy") => (
                Status::Failed,
                "Контейнер работает, health check не пройден".into(),
            ),
            _ if health_required => (Status::Failed, "Нет подтверждения готовности".into()),
            _ => (Status::Running, "Контейнер работает".into()),
        }
    };
    Component { status, details }
}

type Executor = std::sync::Arc<dyn Fn(Command, Duration) -> Result<String> + Send + Sync>;
struct PreflightSnapshot {
    docker: PathBuf,
    containers: BTreeMap<bool, std::result::Result<Vec<Value>, String>>,
}
pub struct Runtime {
    pub cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub layout: Layout,
    pub state: State,
    pub browser: Option<ManagedChild>,
    pub offline: bool,
    login_failures: u32,
    observed_managed_resources: bool,
    executor: Executor,
    preflight_snapshot: std::cell::RefCell<Option<PreflightSnapshot>>,
}
impl Runtime {
    pub fn prepare_configuration(&self) -> Result<()> {
        self.layout.write_env(&self.state.config)?;
        self.write_compose(false)?;
        self.write_compose(true)?;
        self.isolation_summary()?;
        Ok(())
    }
    pub fn isolation_summary(&self) -> Result<Value> {
        let mut projects = Vec::new();
        for generation in [false, true] {
            let file = self.layout.data.join(if generation {
                "generation.compose.json"
            } else {
                "infra.compose.json"
            });
            let document: Value = serde_json::from_slice(&fs::read(&file)?)?;
            let mut services = serde_json::Map::new();
            for (name, service) in document["services"]
                .as_object()
                .context("services missing")?
            {
                if service["labels"]["com.hh-clicker.instance-id"].as_str()
                    != Some(&self.state.config.instance.to_string())
                {
                    bail!("wrong owner");
                }
                if let Some(volumes) = service["volumes"].as_array() {
                    for volume in volumes {
                        if volume["type"] != "bind"
                            || !PathBuf::from(volume["source"].as_str().context("mount source")?)
                                .starts_with(&self.layout.data)
                        {
                            bail!("mount outside isolated data");
                        }
                    }
                }
                if let Some(ports) = service["ports"].as_array() {
                    for port in ports {
                        if !port.as_str().is_some_and(|p| p.starts_with("127.0.0.1:")) {
                            bail!("non-loopback port");
                        }
                    }
                }
                services.insert(name.clone(),json!({"ports":service["ports"],"mounts":service["volumes"],"build":service["build"]}));
            }
            projects.push(json!({"project":self.layout.project(&self.state.config,generation),"services":services}));
        }
        Ok(
            json!({"instance":self.state.config.instance,"data":self.layout.data,"payload":self.layout.payload,"projects":projects}),
        )
    }
    pub fn new(layout: Layout, config: Config, offline: bool) -> Self {
        let cancellation = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let control = cancellation.clone();
        Self {
            cancellation,
            layout,
            state: State::new(config),
            browser: None,
            offline,
            login_failures: 0,
            observed_managed_resources: false,
            executor: std::sync::Arc::new(move |c, t| process::run_cancellable(c, t, &control)),
            preflight_snapshot: std::cell::RefCell::new(None),
        }
    }
    fn execute(&self, mut c: Command, timeout: Duration) -> Result<String> {
        process::redact_output(&mut c, &[self.state.config.postgres_password.clone()]);
        let args: Vec<_> = c
            .get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        if args.first().is_some_and(|s| s == "compose")
            && args.iter().any(|s| s == "up" || s == "down")
        {
            c.env(
                "HH_RUST_COMMAND_LOG",
                self.layout.data.join("logs/compose.log"),
            );
        }
        (self.executor)(c, timeout)
    }
    fn check_cancelled(&self) -> Result<()> {
        if self.cancellation.load(std::sync::atomic::Ordering::Acquire) {
            bail!("Запуск отменён");
        }
        Ok(())
    }
    fn docker(&self) -> PathBuf {
        if let Some(snapshot) = self.preflight_snapshot.borrow().as_ref() {
            return snapshot.docker.clone();
        }
        crate::preflight::docker_cli().unwrap_or_else(|| PathBuf::from("docker.exe"))
    }
    fn docker_query(&self, args: &[&str]) -> Result<String> {
        let mut c = process::docker_command(self.docker());
        c.args(args);
        self.execute(c, Duration::from_secs(12))
    }
    fn require_local_docker(&self) -> Result<()> {
        let explicit_context = std::env::var_os("DOCKER_CONTEXT").is_some_and(|v| !v.is_empty());
        let host = if let Some(host) =
            std::env::var_os("DOCKER_HOST").filter(|v| !v.is_empty() && !explicit_context)
        {
            host.to_string_lossy().into_owned()
        } else {
            self.docker_query(&[
                "context",
                "inspect",
                "--format",
                "{{.Endpoints.docker.Host}}",
            ])?
        };
        if !crate::preflight::local_endpoint(&host) {
            bail!(
                "Управление удалённым Docker endpoint запрещено. Выберите локальный Docker Desktop"
            );
        }
        Ok(())
    }
    fn containers(&self, generation: bool) -> Result<Vec<Value>> {
        if let Some(cached) = self
            .preflight_snapshot
            .borrow()
            .as_ref()
            .and_then(|s| s.containers.get(&generation))
        {
            return cached.clone().map_err(anyhow::Error::msg);
        }
        let result = self.read_containers(generation);
        if let Some(snapshot) = self.preflight_snapshot.borrow_mut().as_mut() {
            snapshot.containers.insert(
                generation,
                result
                    .as_ref()
                    .map(Clone::clone)
                    .map_err(|e| format!("{e:#}")),
            );
        }
        result
    }
    fn read_containers(&self, generation: bool) -> Result<Vec<Value>> {
        let project = self.layout.project(&self.state.config, generation);
        let filter = format!("label=com.docker.compose.project={project}");
        let ids = self.docker_query(&["ps", "-aq", "--filter", &filter])?;
        if ids.trim().is_empty() {
            return Ok(vec![]);
        }
        let mut c = process::docker_command(self.docker());
        c.arg("inspect").args(ids.split_whitespace());
        let list: Vec<Value> = serde_json::from_str(&self.execute(c, Duration::from_secs(15))?)?;
        for value in &list {
            let service = value["Config"]["Labels"]["com.docker.compose.service"]
                .as_str()
                .unwrap_or("");
            let allowed = if generation {
                service == "generation-agent"
            } else {
                ["postgres", "backend", "frontend"].contains(&service)
            };
            if !allowed
                || !owned(
                    value,
                    &self.state.config.instance.to_string(),
                    &project,
                    service,
                )
            {
                bail!("В проекте {project} найден чужой контейнер. Управление запрещено");
            }
            if service != "postgres" {
                let (key, expected) = match service {
                    "backend" => ("8080/tcp", self.state.config.backend_port),
                    "frontend" => ("80/tcp", self.state.config.frontend_port),
                    _ => ("17374/tcp", self.state.config.generation_port),
                };
                let port = expected.to_string();
                if !value["HostConfig"]["PortBindings"][key]
                    .as_array()
                    .is_some_and(|bindings| {
                        bindings.len() == 1
                            && bindings[0]["HostPort"].as_str() == Some(&port)
                            && bindings[0]["HostIp"].as_str() == Some("127.0.0.1")
                    })
                {
                    bail!("Порты контейнера {service} не соответствуют этой portable-копии. Управление запрещено");
                }
            }
        }
        Ok(list)
    }
    fn ensure_volumes_owned(&self, generation: bool) -> Result<()> {
        // Networks also have project identity; refuse to adopt a manually-created collision.
        let project = self.layout.project(&self.state.config, generation);
        let filter = format!("label=com.docker.compose.project={project}");
        let ids = self.docker_query(&["network", "ls", "-q", "--filter", &filter])?;
        if !ids.is_empty() {
            let mut c = process::docker_command(self.docker());
            c.args(["network", "inspect"]).args(ids.split_whitespace());
            let networks: Vec<Value> =
                serde_json::from_str(&self.execute(c, Duration::from_secs(12))?)?;
            for network in networks {
                if network["Labels"]["com.hh-clicker.instance-id"].as_str()
                    != Some(&self.state.config.instance.to_string())
                {
                    bail!("Сеть проекта {project} не принадлежит эксперименту");
                }
            }
        }
        Ok(())
    }
    pub fn observe(&mut self) -> Result<()> {
        if self.offline {
            self.state.message =
                "Автономный просмотр: команды окружения и сервисов отключены".into();
            return Ok(());
        }
        for generation in [false, true] {
            let indices: Vec<usize> = if generation { vec![3] } else { vec![0, 1, 2] };
            match self.containers(generation) {
                Ok(containers) => {
                    self.observed_managed_resources |= !containers.is_empty();
                    for i in indices {
                        self.state.components[i] = containers
                            .iter()
                            .find(|v| {
                                v["Config"]["Labels"]["com.docker.compose.service"].as_str()
                                    == Some(SERVICES[i])
                            })
                            .map(|v| container_status(v, i != 2))
                            .unwrap_or(Component {
                                status: Status::Stopped,
                                details: "Контейнер не запущен".into(),
                            });
                        if i > 0 && self.state.components[i].status == Status::Running {
                            let path = match i {
                                1 => "/actuator/health",
                                2 => "/",
                                _ => "/health",
                            };
                            if !self.http_ready(i, path) {
                                self.state.components[i] = Component {
                                    status: Status::Failed,
                                    details: "Контейнер работает, HTTP не подтвердил готовность"
                                        .into(),
                                };
                            }
                        }
                    }
                }
                Err(e) => {
                    for i in indices {
                        self.state.components[i] = Component {
                            status: Status::Unknown,
                            details: e.to_string(),
                        };
                    }
                }
            }
        }
        if self.state.components[0].status != Status::Running
            && self.state.components[1].status == Status::Running
        {
            self.state.components[1] = Component {
                status: Status::Failed,
                details: "Backend зависит от здоровой PostgreSQL".into(),
            };
        }
        let browser_alive = self.browser.as_mut().is_some_and(ManagedChild::alive);
        self.state.components[4] = if browser_alive {
            match self.heartbeat() {
                Ok(true) => Component {
                    status: Status::Running,
                    details: "Backend подтвердил heartbeat этого агента".into(),
                },
                _ => Component {
                    status: Status::Failed,
                    details: "Процесс работает, но его heartbeat не подтверждён".into(),
                },
            }
        } else {
            Component {
                status: Status::Stopped,
                details: "Локальный процесс остановлен".into(),
            }
        };
        if self.state.components[3].status == Status::Running {
            if self.state.auth.pending {
                self.poll_login();
            } else {
                self.state.auth = auth::json(
                    &format!("{}/auth/status", self.state.config.url(3)),
                    false,
                    3,
                )
                .and_then(|v| auth::account(&v))
                .unwrap_or_else(|_| Auth {
                    message: "Не удалось подтвердить статус Codex. Проверьте соединение".into(),
                    ..Default::default()
                });
            }
            if let Ok(v) = auth::json(&format!("{}/health", self.state.config.url(3)), false, 3) {
                self.state.runtime_info = format!(
                    "{} · {} · CLI {}",
                    v["model"].as_str().unwrap_or("—"),
                    v["reasoningEffort"].as_str().unwrap_or("—"),
                    v["cliVersion"].as_str().unwrap_or("—")
                );
            }
        } else {
            self.state.auth = Auth {
                message: "Сначала запустите Generation Agent".into(),
                ..Default::default()
            };
            self.state.runtime_info.clear();
        }
        if !self.state.busy {
            if !self.state.ready && !self.state.checks.is_empty() {
                self.state.message = self
                    .state
                    .checks
                    .iter()
                    .find(|c| !c.passed && c.required)
                    .map(|c| c.message.clone())
                    .unwrap_or_else(|| "Повторите проверку окружения".into());
            } else if self.state.components[..4]
                .iter()
                .any(|c| c.status == Status::Unknown)
            {
                self.state.ready = false;
                self.state.message =
                    "Нет подтверждения состояния Docker. Повторите проверку окружения".into();
            } else if self.state.running() {
                self.state.message = if self.state.auth.signed_in {
                    "Все компоненты доступны и готовы к работе".into()
                } else {
                    self.state.auth.message.clone()
                };
            } else if self
                .state
                .components
                .iter()
                .any(|c| c.status == Status::Running)
            {
                self.state.message = format!(
                    "{} из 5 компонентов работают",
                    self.state
                        .components
                        .iter()
                        .filter(|c| c.status == Status::Running)
                        .count()
                );
            } else {
                self.state.message = "Все компоненты сейчас остановлены".into();
            }
        }
        Ok(())
    }
    fn http_ready(&self, i: usize, path: &str) -> bool {
        let url = format!("{}{path}", self.state.config.url(i));
        if i == 2 {
            auth::agent(3).get(&url).call().is_ok()
        } else {
            auth::json(&url, false, 3).is_ok_and(|v| {
                if i == 1 {
                    v["status"] == "UP"
                } else {
                    v["ok"] == true
                }
            })
        }
    }
    fn heartbeat(&self) -> Result<bool> {
        let id = self
            .browser
            .as_ref()
            .context("Browser Agent не запущен")?
            .agent_id
            .as_str();
        let v = auth::json(
            &format!("{}/api/v1/browser-agent/status", self.state.config.url(1)),
            false,
            3,
        )?;
        Ok(v["agentId"].as_str() == Some(id)
            && v["status"].as_str().is_some_and(|s| s != "OFFLINE"))
    }
    pub fn preflight(&mut self) -> Result<()> {
        let docker = self.docker();
        *self.preflight_snapshot.borrow_mut() = Some(PreflightSnapshot {
            docker,
            containers: BTreeMap::new(),
        });
        let result = self.preflight_inner();
        self.preflight_snapshot.borrow_mut().take();
        result
    }
    fn preflight_inner(&mut self) -> Result<()> {
        if self.offline {
            self.state.checks = vec![Check {
                name: "Автономный просмотр".into(),
                passed: false,
                message: "Проверки компьютера отключены параметром --offline. Данные изолированы"
                    .into(),
                required: true,
            }];
            self.state.ready = false;
            return Ok(());
        }
        self.observe()?;
        let mut checks = crate::preflight::probe(&self.layout);
        let ownership = [false, true]
            .into_iter()
            .try_for_each(|generation| self.containers(generation).map(|_| ()));
        checks.push(Check{name:"Принадлежность ресурсов".into(),passed:ownership.is_ok(),message:ownership.err().map(|e|e.to_string()).unwrap_or_else(||"Контейнеры принадлежат этой portable-копии; чужие ресурсы не принимаются под управление".into()),required:true});
        checks.push(Check {
            name: "Файлы приложения".into(),
            passed: self.layout.check_payload().is_ok(),
            message: self
                .layout
                .check_payload()
                .err()
                .map(|e| e.to_string())
                .unwrap_or_else(|| "Полный portable payload найден".into()),
            required: true,
        });
        let mut conflicts = vec![];
        let owned_services: Vec<String> = [false, true]
            .into_iter()
            .flat_map(|generation| self.containers(generation).unwrap_or_default())
            .filter(|v| v["State"]["Running"] == true)
            .filter_map(|v| {
                v["Config"]["Labels"]["com.docker.compose.service"]
                    .as_str()
                    .map(str::to_owned)
            })
            .collect();
        for (i, p) in [
            (1, self.state.config.backend_port),
            (2, self.state.config.frontend_port),
            (3, self.state.config.generation_port),
        ] {
            if !owned_services.iter().any(|s| s == SERVICES[i]) && !config::available(p) {
                let owner = crate::platform::port_owner(p)
                    .unwrap_or_else(|| "владелец не определён".into());
                conflicts.push(format!("{}: {p} — {owner}", NAMES[i]));
            }
        }
        checks.push(Check {
            name: "Локальные порты".into(),
            passed: conflicts.is_empty(),
            message: if conflicts.is_empty() {
                "Порты свободны или принадлежат этой установке".into()
            } else {
                format!(
                    "Заняты: {}. Выберите свободные порты; чужие процессы не останавливаются",
                    conflicts.join(", ")
                )
            },
            required: true,
        });
        self.state.ready = checks.iter().all(|c| c.passed || !c.required);
        self.state.message = if self.state.ready {
            "Все зависимости для локального запуска доступны"
        } else {
            "Откройте настройки и выполните обязательные пункты"
        }
        .into();
        self.state.checks = checks;
        Ok(())
    }
    pub fn free_ports(&mut self) -> Result<()> {
        if self.offline {
            bail!("Действие отключено в автономном просмотре");
        }
        self.observe()?;
        // Do not rewrite live service endpoints (even if their HTTP health is failing).
        for generation in [false, true] {
            if self
                .containers(generation)?
                .iter()
                .any(|v| v["State"]["Running"] == true)
            {
                bail!("Сначала остановите сервисы этой portable-копии");
            }
        }
        if self.browser.as_mut().is_some_and(ManagedChild::alive) {
            bail!("Сначала остановите Browser Agent");
        }
        let mut ports = vec![];
        for (p, start) in [
            (&mut self.state.config.backend_port, 28080),
            (&mut self.state.config.frontend_port, 23000),
            (&mut self.state.config.generation_port, 27374),
        ] {
            if !config::available(*p) || ports.contains(p) {
                *p = config::choose_port(start, &mut ports)?;
            } else {
                ports.push(*p);
            }
        }
        self.layout.save(&self.state.config)?;
        self.preflight()
    }
    fn compose(&self, generation: bool) -> Result<Command> {
        let project = self.layout.project(&self.state.config, generation);
        let mut c = process::docker_command(self.docker());
        process::clean_compose_env(&mut c);
        c.current_dir(&self.layout.data)
            .arg("compose")
            .arg("--project-name")
            .arg(project)
            .arg("--env-file")
            .arg(self.layout.data.join("runtime.env"))
            .arg("-f")
            .arg(self.layout.data.join(if generation {
                "generation.compose.json"
            } else {
                "infra.compose.json"
            }));
        Ok(c)
    }
    fn write_compose(&self, generation: bool) -> Result<()> {
        let c = &self.state.config;
        let root = &self.layout.payload;
        let labels = json!({"com.hh-clicker.instance-id":c.instance.to_string(),"com.hh-clicker.version":config::APP_VERSION,"com.hh-clicker.profile":"rust-portable"});
        let bind = |name: &str, target: &str| -> Result<Value> {
            let path = self.layout.data.join(name);
            fs::create_dir_all(&path)?;
            Ok(json!({"type":"bind","source":path,"target":target}))
        };
        let value = if generation {
            json!({"services":{"generation-agent":{
            "build":{"context":root.join("generation-agent"),"args":{"CODEX_CLI_VERSION":c.codex_version}},"init":true,"restart":"unless-stopped","labels":labels,
            "ports":[format!("127.0.0.1:{}:17374",c.generation_port)],
            "environment":{"GENERATION_HOST":"0.0.0.0","GENERATION_PORT":"17374","GENERATION_DATA_DIR":"/data","CODEX_MODEL":c.codex_model,"CODEX_REASONING_EFFORT":c.codex_effort,"CODEX_TIMEOUT_MS":"180000","GENERATION_MAX_PENDING":"20","GENERATION_MAX_CONCURRENT_ANALYSIS":"4","GENERATION_MEMORY_PERCENT":"25","GENERATION_JOB_MEMORY_MIB":"128","GENERATION_RETENTION_DAYS":"7"},
            "volumes":[bind("generation","/data")?,bind("codex","/home/node/.codex")?],"read_only":true,"tmpfs":["/tmp:rw,nosuid,size=128m,mode=1777"],"cap_drop":["ALL"],"security_opt":["no-new-privileges:true"],"pids_limit":-1,"stop_grace_period":"15s",
            "healthcheck":{"test":["CMD","node","-e","fetch('http://127.0.0.1:17374/health').then(r=>process.exit(r.ok?0:1)).catch(()=>process.exit(1))"],"interval":"30s","timeout":"5s","start_period":"10s","retries":3}
        }},"networks":{"default":{"labels":labels}}})
        } else {
            json!({"services":{
            "postgres":{"image":"postgres:17-alpine","restart":"unless-stopped","labels":labels,"environment":{"POSTGRES_DB":"hh_clicker","POSTGRES_USER":"hh_clicker","POSTGRES_PASSWORD":c.postgres_password},"volumes":[bind("postgres","/var/lib/postgresql/data")?],"healthcheck":{"test":["CMD-SHELL","pg_isready -U $${POSTGRES_USER} -d $${POSTGRES_DB}"],"interval":"5s","timeout":"3s","retries":10,"start_period":"10s"}},
            "backend":{"build":{"context":root.join("backend"),"dockerfile":"Dockerfile"},"restart":"unless-stopped","labels":labels,"environment":{"DB_URL":"jdbc:postgresql://postgres:5432/hh_clicker","DB_USERNAME":"hh_clicker","DB_PASSWORD":c.postgres_password,"DB_SCHEMA":"hhclicker","GENERATION_AGENT_URL":format!("http://host.docker.internal:{}",c.generation_port)},"ports":[format!("127.0.0.1:{}:8080",c.backend_port)],"depends_on":{"postgres":{"condition":"service_healthy"}},"healthcheck":{"test":["CMD","curl","--fail","--silent","http://localhost:8080/actuator/health"],"interval":"10s","timeout":"5s","retries":10,"start_period":"30s"}},
            "frontend":{"build":{"context":root.join("frontend"),"dockerfile":"Dockerfile"},"restart":"unless-stopped","labels":labels,"ports":[format!("127.0.0.1:{}:80",c.frontend_port)],"depends_on":{"backend":{"condition":"service_healthy"}}}
        },"networks":{"default":{"labels":labels}}})
        };
        config::atomic_write(
            &self.layout.data.join(if generation {
                "generation.compose.json"
            } else {
                "infra.compose.json"
            }),
            &serde_json::to_vec_pretty(&value)?,
        )
    }
    fn start_project(&mut self, generation: bool, publish: &mut impl FnMut(&State)) -> Result<()> {
        self.require_local_docker()?;
        self.containers(generation)?;
        self.ensure_volumes_owned(generation)?;
        self.write_compose(generation)?;
        let project = self.layout.project(&self.state.config, generation);
        let manifest = fs::read(self.layout.payload.join("release-manifest.json"))?;
        let id = config::payload_id(
            &manifest,
            if generation {
                Some(&self.state.config.codex_version)
            } else {
                None
            },
        );
        let marker = self.layout.data.join(format!("{project}.built-payload"));
        let images: Vec<String> = if generation {
            vec![format!("{project}-generation-agent")]
        } else {
            vec![format!("{project}-backend"), format!("{project}-frontend")]
        };
        let images_present = {
            let mut cmd = process::docker_command(self.docker());
            cmd.args(["image", "inspect", "--format", "{{.Id}}"])
                .args(images);
            self.execute(cmd, Duration::from_secs(20)).is_ok()
        };
        let build = fs::read_to_string(&marker).ok().as_deref() != Some(&id) || !images_present;
        self.state.message = if build {
            "Собираем Docker-образы этой версии…"
        } else {
            "Запускаем готовые Docker-образы…"
        }
        .into();
        publish(&self.state);
        let mut cmd = self.compose(generation)?;
        cmd.args(["up", "-d"]);
        if build {
            cmd.arg("--build");
        } else {
            cmd.arg("--no-build");
        }
        config::atomic_write(
            &self.layout.data.join("managed-projects.json"),
            &serde_json::to_vec(&json!({"instance":self.state.config.instance}))?,
        )?;
        self.execute(cmd, Duration::from_secs(600))
            .context("Не удалось запустить Compose-проект")?;
        if build {
            config::atomic_write(&marker, id.as_bytes())?;
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(180);
        loop {
            self.check_cancelled()?;
            self.observe()?;
            publish(&self.state);
            let indices: Vec<usize> = if generation { vec![3] } else { vec![0, 1, 2] };
            if indices
                .iter()
                .all(|i| self.state.components[*i].status == Status::Running)
            {
                return Ok(());
            }
            if std::time::Instant::now() > deadline {
                bail!("Сервисы не подтвердили готовность. Backend требует здоровую PostgreSQL");
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    pub fn start(&mut self, mut publish: impl FnMut(&State)) -> Result<()> {
        if self.offline {
            bail!("Запуск сервисов отключён в автономном просмотре");
        }
        self.state.operation = Operation::Starting;
        self.state.revision += 1;
        self.preflight()?;
        self.check_cancelled()?;
        publish(&self.state);
        if !self.state.ready {
            bail!("Выполните обязательные пункты в настройках");
        }
        self.layout.verify_payload()?;
        self.prepare_configuration()?;
        self.state.components[0].status = Status::Starting;
        self.state.components[1].status = Status::Starting;
        self.state.components[2].status = Status::Starting;
        publish(&self.state);
        self.start_project(false, &mut publish)?;
        self.state.components[3].status = Status::Starting;
        publish(&self.state);
        self.start_project(true, &mut publish)?;
        if !self.browser.as_mut().is_some_and(ManagedChild::alive) {
            self.state.components[4].status = Status::Starting;
            self.state.message = "Запускаем Browser Agent…".into();
            publish(&self.state);
            let node = crate::preflight::node_runtime(&self.layout)
                .context("Совместимый Node.js не найден")?;
            let entry = crate::preflight::browser_directory(&self.layout)
                .context("Browser Agent не подготовлен")?
                .join("dist/main.js");
            if !entry.is_file() {
                bail!("Подготовьте Browser Agent в настройках");
            }
            let id = format!(
                "rust-{}-{}",
                self.state.config.instance.simple(),
                uuid::Uuid::new_v4().simple()
            );
            let env = BTreeMap::from([
                ("HH_CLICKER_LAUNCHER_MANAGED".into(), "true".into()),
                ("HH_CLICKER_BACKEND_URL".into(), self.state.config.url(1)),
                ("HH_CLICKER_GENERATION_URL".into(), self.state.config.url(3)),
                ("HH_CLICKER_AGENT_ID".into(), id.clone()),
                (
                    "HH_CLICKER_AGENT_VERSION".into(),
                    config::APP_VERSION.into(),
                ),
                (
                    "HH_CLICKER_BROWSER_PROFILE_DIR".into(),
                    self.layout
                        .data
                        .join("browser-profiles")
                        .to_string_lossy()
                        .into_owned(),
                ),
            ]);
            self.browser = Some(ManagedChild::spawn(
                &node,
                &entry,
                &self.layout.data,
                env,
                id,
            )?);
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            while self.browser.as_mut().is_some_and(ManagedChild::alive)
                && std::time::Instant::now() < deadline
            {
                self.check_cancelled()?;
                if self.heartbeat().unwrap_or(false) {
                    self.observe()?;
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            if let Some(mut b) = self.browser.take() {
                let _ = b.stop();
            }
            self.state.components[4] = Component {
                status: Status::Failed,
                details:
                    "Backend не подтвердил heartbeat Browser Agent. См. data/logs/browser-agent.log"
                        .into(),
            };
            bail!("Browser Agent не подтвердил готовность");
        }
        self.observe()
    }
    pub fn needs_shutdown(&self) -> bool {
        self.layout.data.join("managed-projects.json").is_file()
            || self.observed_managed_resources
            || self.browser.is_some()
    }
    pub fn stop(&mut self, mut publish: impl FnMut(&State)) -> Result<()> {
        if self.offline {
            return Ok(());
        }
        if !self.needs_shutdown() {
            return Ok(());
        }
        let ever_started = self.layout.data.join("managed-projects.json").is_file();
        let observed = self.observed_managed_resources;
        self.state.revision += 1;
        let initial_components = self.state.components.clone();
        self.state.mark_stopping();
        self.state.message = "Останавливаем сервисы…".into();
        publish(&self.state);
        let mut errors = Vec::new();
        if let Some(mut browser) = self.browser.take() {
            if let Err(e) = browser.stop() {
                self.state.components[4] = Component {
                    status: Status::Failed,
                    details: e.to_string(),
                };
                errors.push(e.to_string());
            }
        }
        if self.state.components[4].status != Status::Failed {
            self.state.components[4].status = Status::Stopped;
        }
        publish(&self.state);
        if !ever_started && !observed {
            self.state.components[..4].clone_from_slice(&initial_components[..4]);
            publish(&self.state);
            if !errors.is_empty() {
                bail!("{}", errors.join("\n"));
            }
            return Ok(());
        }
        if let Err(error) = self.require_local_docker() {
            for c in &mut self.state.components[..4] {
                if c.status != Status::Stopped {
                    *c = Component {
                        status: Status::Failed,
                        details: error.to_string(),
                    };
                }
            }
            publish(&self.state);
            return Err(error);
        }
        for generation in [true, false] {
            let outcome = (|| -> Result<()> {
                self.containers(generation)?;
                self.ensure_volumes_owned(generation)?;
                self.layout.write_env(&self.state.config)?;
                self.write_compose(generation)?;
                let mut c = self.compose(generation)?;
                c.arg("down");
                self.execute(c, Duration::from_secs(120))?;
                Ok(())
            })();
            let indices: &[usize] = if generation { &[3] } else { &[0, 1, 2] };
            match outcome {
                Ok(()) => {
                    for &i in indices {
                        self.state.components[i].status = Status::Stopped;
                    }
                }
                Err(e) => {
                    let message = e.to_string();
                    for &i in indices {
                        self.state.components[i] = Component {
                            status: Status::Failed,
                            details: message.clone(),
                        };
                    }
                    errors.push(message);
                }
            }
            publish(&self.state);
        }
        self.observe()?;
        if !errors.is_empty() {
            bail!("{}", errors.join("\n"));
        }
        if self.state.components[..4]
            .iter()
            .all(|c| c.status == Status::Stopped)
        {
            let _ = fs::remove_file(self.layout.data.join("managed-projects.json"));
            self.observed_managed_resources = false;
        }
        Ok(())
    }
    pub fn auth_action(&mut self, path: &str) -> Result<()> {
        if self.offline || self.state.components[3].status != Status::Running {
            bail!("Сначала запустите Generation Agent этой portable-копии");
        }
        let containers = self.containers(true)?;
        if !containers
            .iter()
            .any(|v| v["State"]["Running"] == true && v["State"]["Health"]["Status"] == "healthy")
        {
            bail!("Generation Agent этой portable-копии не подтвердил готовность");
        }
        let v = auth::json(&format!("{}{path}", self.state.config.url(3)), true, 45)?;
        self.state.auth = if path.contains("status") {
            auth::account(&v)?
        } else {
            auth::login(&v)?
        };
        self.login_failures = 0;
        Ok(())
    }
    fn poll_login(&mut self) {
        match auth::json(
            &format!("{}/auth/login", self.state.config.url(3)),
            false,
            3,
        )
        .and_then(|v| auth::login(&v))
        {
            Ok(a) => {
                self.state.auth = a;
                self.login_failures = 0;
            }
            Err(_) => {
                self.login_failures += 1;
                if self.login_failures >= 10 {
                    self.state.auth.pending = false;
                    self.state.auth.message =
                        "Не удалось проверить вход после 10 попыток. Проверьте статус".into();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closing_a_fresh_profile_does_not_require_an_engine() {
        let mut runtime = fixture();
        fs::remove_file(runtime.layout.data.join("managed-projects.json")).unwrap();
        runtime.executor = Arc::new(|_, _| panic!("fresh profile must not send a stop command"));
        for c in &mut runtime.state.components[..4] {
            c.status = Status::Failed;
        }
        assert!(!runtime.needs_shutdown());
        runtime.stop(|_| {}).unwrap();
        assert!(runtime.state.components[..4]
            .iter()
            .all(|c| c.status == Status::Failed));
        fs::remove_dir_all(runtime.layout.root).unwrap();
    }
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };
    fn fixture() -> Runtime {
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let layout = Layout::new(root, None);
        let c = layout.load().unwrap();
        fs::create_dir_all(&layout.payload).unwrap();
        fs::write(layout.payload.join("release-manifest.json"), b"payload-one").unwrap();
        fs::write(layout.data.join("managed-projects.json"), b"{}").unwrap();
        Runtime::new(layout, c, false)
    }
    #[test]
    fn preflight_snapshot_does_not_hide_later_docker_changes() {
        let mut rt = fixture();
        let calls = Arc::new(Mutex::new(0));
        let recorded = calls.clone();
        rt.executor = Arc::new(move |_, _| {
            *recorded.lock().unwrap() += 1;
            bail!("engine unavailable")
        });
        *rt.preflight_snapshot.borrow_mut() = Some(PreflightSnapshot {
            docker: PathBuf::from("docker.exe"),
            containers: BTreeMap::new(),
        });
        assert!(rt.containers(false).is_err());
        assert!(rt.containers(false).is_err());
        assert_eq!(*calls.lock().unwrap(), 1);
        rt.preflight_snapshot.borrow_mut().take();
        rt.executor = Arc::new(|_, _| Ok(String::new()));
        assert!(rt.containers(false).unwrap().is_empty());
        // Even the offline early-return must discard the temporary snapshot.
        rt.offline = true;
        rt.preflight().unwrap();
        assert!(rt.preflight_snapshot.borrow().is_none());
        fs::remove_dir_all(rt.layout.root).unwrap();
    }
    #[test]
    fn stop_continues_after_foreign_generation_and_preserves_data() {
        let mut rt = fixture();
        let config = rt.state.config.clone();
        let project = rt.layout.project(&config, false);
        let generation = rt.layout.project(&config, true);
        let own = json!({"Config":{"Labels":{"com.hh-clicker.instance-id":config.instance.to_string(),"com.docker.compose.project":project,"com.docker.compose.service":"backend","com.hh-clicker.profile":"rust-portable"}},"HostConfig":{"PortBindings":{"8080/tcp":[{"HostPort":config.backend_port.to_string(),"HostIp":"127.0.0.1"}]}},"State":{"Running":false,"Status":"exited"}});
        let foreign = json!({"Config":{"Labels":{"com.hh-clicker.instance-id":"other","com.docker.compose.project":generation,"com.docker.compose.service":"generation-agent"}}});
        let commands = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
        let recorded = commands.clone();
        rt.executor = Arc::new(move |c, _| {
            let args: Vec<_> = c
                .get_args()
                .map(|s| s.to_string_lossy().into_owned())
                .collect();
            recorded.lock().unwrap().push(args.clone());
            if args.first().is_some_and(|s| s == "context") {
                return Ok("npipe:////./pipe/dockerDesktopLinuxEngine".into());
            }
            if args.first().is_some_and(|s| s == "ps") {
                return Ok(if args.iter().any(|s| s.contains("-generation")) {
                    "foreign"
                } else {
                    "own"
                }
                .into());
            }
            if args.first().is_some_and(|s| s == "inspect") {
                return Ok(serde_json::to_string(&vec![if args[1] == "foreign" {
                    foreign.clone()
                } else {
                    own.clone()
                }])
                .unwrap());
            }
            Ok(String::new())
        });
        let sentinel = rt.layout.data.join("postgres/keep");
        fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
        fs::write(&sentinel, b"database").unwrap();
        assert!(rt.stop(|_| {}).is_err());
        let commands = commands.lock().unwrap();
        let downs: Vec<_> = commands
            .iter()
            .filter(|a| a.last().is_some_and(|s| s == "down"))
            .collect();
        assert_eq!(downs.len(), 1);
        assert!(downs[0].contains(&project));
        assert!(!downs[0].contains(&generation));
        assert!(!downs[0].iter().any(|s| s == "-v" || s == "--volumes"));
        assert!(sentinel.exists());
        drop(commands);
        fs::remove_dir_all(rt.layout.root).unwrap();
    }
    #[test]
    fn payload_marker_never_substitutes_for_docker_images() {
        let mut rt = fixture();
        rt.offline = true;
        for c in &mut rt.state.components {
            c.status = Status::Running;
        }
        let images_exist = Arc::new(AtomicBool::new(true));
        let image_flag = images_exist.clone();
        let commands = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
        let recorded = commands.clone();
        rt.executor = Arc::new(move |c, _| {
            let args: Vec<_> = c
                .get_args()
                .map(|s| s.to_string_lossy().into_owned())
                .collect();
            recorded.lock().unwrap().push(args.clone());
            if args.first().is_some_and(|s| s == "context") {
                return Ok("npipe:////./pipe/dockerDesktopLinuxEngine".into());
            }
            if args.first().is_some_and(|s| s == "image") && !image_flag.load(Ordering::Relaxed) {
                bail!("images deleted");
            }
            Ok(String::new())
        });
        rt.start_project(false, &mut |_| {}).unwrap();
        rt.start_project(false, &mut |_| {}).unwrap();
        images_exist.store(false, Ordering::Relaxed);
        rt.start_project(false, &mut |_| {}).unwrap();
        let commands = commands.lock().unwrap();
        let ups: Vec<_> = commands
            .iter()
            .filter(|a| a.iter().any(|s| s == "up"))
            .collect();
        assert_eq!(ups.len(), 3);
        assert!(ups[0].iter().any(|s| s == "--build"));
        assert!(ups[1].iter().any(|s| s == "--no-build"));
        assert!(ups[2].iter().any(|s| s == "--build"));
        drop(commands);
        fs::remove_dir_all(rt.layout.root).unwrap();
    }
    #[test]
    fn compose_is_explicit_and_ignores_ambient_project_settings() {
        let rt = fixture();
        let c = rt.compose(false).unwrap();
        let args: Vec<_> = c
            .get_args()
            .map(|v| v.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[0], "compose");
        assert_eq!(args[1], "--project-name");
        assert!(args[2].starts_with("hh-rust-"));
        assert!(args.contains(&"--env-file".into()));
        assert!(args.contains(&"-f".into()));
        fs::remove_dir_all(rt.layout.root).unwrap();
    }
    #[test]
    fn portable_compose_uses_only_adjacent_data_and_loopback_ports() {
        let rt = fixture();
        rt.write_compose(false).unwrap();
        rt.write_compose(true).unwrap();
        let infra: Value =
            serde_json::from_slice(&fs::read(rt.layout.data.join("infra.compose.json")).unwrap())
                .unwrap();
        let gen: Value = serde_json::from_slice(
            &fs::read(rt.layout.data.join("generation.compose.json")).unwrap(),
        )
        .unwrap();
        assert!(infra["services"]["postgres"].get("ports").is_none());
        assert_eq!(
            infra["services"]["backend"]["depends_on"]["postgres"]["condition"],
            "service_healthy"
        );
        for (doc, services) in [
            (&infra, vec!["postgres", "backend", "frontend"]),
            (&gen, vec!["generation-agent"]),
        ] {
            for service in services {
                let v = &doc["services"][service];
                if let Some(ports) = v["ports"].as_array() {
                    assert!(ports
                        .iter()
                        .all(|p| p.as_str().unwrap().starts_with("127.0.0.1:")));
                }
                if let Some(volumes) = v["volumes"].as_array() {
                    for volume in volumes {
                        assert_eq!(volume["type"], "bind");
                        assert!(PathBuf::from(volume["source"].as_str().unwrap())
                            .starts_with(&rt.layout.data));
                    }
                }
            }
        }
        fs::remove_dir_all(rt.layout.root).unwrap();
    }
    #[test]
    fn foreign_container_rejected() {
        let mut c = json!({"Config":{"Labels":{"com.hh-clicker.instance-id":"a","com.docker.compose.project":"hh-rust-a","com.docker.compose.service":"postgres","com.hh-clicker.profile":"rust-portable"}}});
        assert!(owned(&c, "a", "hh-rust-a", "postgres"));
        c["Config"]["Labels"]["com.hh-clicker.instance-id"] = json!("b");
        assert!(!owned(&c, "a", "hh-rust-a", "postgres"));
    }
    #[test]
    fn in_flight_inspection_cannot_revert_stop_or_finished_stop() {
        let mut rt = fixture();
        let state = &mut rt.state;
        state.busy = true;
        state.revision = 12;
        state.operation = Operation::Starting;
        for c in &mut state.components {
            c.status = Status::Running;
        }
        let old = state.components.clone();
        state.mark_stopping();
        state.apply_observation(12, old.clone());
        assert!(state
            .components
            .iter()
            .all(|c| c.status == Status::Stopping));
        state.components[3].status = Status::Stopped;
        state.apply_observation(12, old.clone());
        assert_eq!(state.components[3].status, Status::Stopped);
        state.operation = Operation::Idle;
        state.busy = false;
        state.revision += 1;
        for c in &mut state.components {
            c.status = Status::Stopped;
        }
        state.apply_observation(12, old.clone());
        assert!(state.components.iter().all(|c| c.status == Status::Stopped));
        state.operation = Operation::Starting;
        state.busy = true;
        state.revision += 1;
        state.apply_observation(12, old.clone());
        assert!(state.components.iter().all(|c| c.status == Status::Stopped));
        state.apply_observation(14, old);
        assert!(state.components[..4]
            .iter()
            .all(|c| c.status == Status::Running));
        assert_eq!(state.components[4].status, Status::Stopped);
        fs::remove_dir_all(rt.layout.root).unwrap();
    }
    #[test]
    fn postgres_needs_health_not_presence() {
        let mut c = json!({"State":{"Running":true}});
        assert_eq!(container_status(&c, true).status, Status::Failed);
        c["State"]["Health"] = json!({"Status":"healthy"});
        assert_eq!(container_status(&c, true).status, Status::Running);
        c["State"]["Health"]["Status"] = json!("unhealthy");
        assert_eq!(container_status(&c, true).status, Status::Failed);
    }
}
