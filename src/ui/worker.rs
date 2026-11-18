//! Serial background actions and publication of UI snapshots.
use super::*;

pub(super) fn run(
    mut runtime: Runtime,
    rx: Receiver<Action>,
    tx: Sender<State>,
    closed: Sender<()>,
    ctx: egui::Context,
    shared: Arc<Mutex<State>>,
) {
    let diagnostics_file = runtime.layout.data.join("launcher-state.json");
    let publish = |s: &State| {
        *shared.lock().unwrap_or_else(|p| p.into_inner()) = s.clone();
        let redactor = hh_clicker_launcher::diagnostics::Redactor::new(
            std::iter::once(s.config.postgres_password.clone()).chain(s.auth.code.clone()),
        );
        let redact = |value: &str| redactor.text(value);
        let diagnostic = serde_json::json!({"revision":s.revision,"operation":format!("{:?}",s.operation),"busy":s.busy,"ready":s.ready,"message":redact(&s.message),"error":s.error.as_deref().map(redact),"auth":{"signed_in":s.auth.signed_in,"pending":s.auth.pending},"components":s.components.iter().enumerate().map(|(i,c)|serde_json::json!({"name":NAMES[i],"status":format!("{:?}",c.status),"details":redact(&c.details)})).collect::<Vec<_>>()});
        if let Ok(bytes) = serde_json::to_vec_pretty(&diagnostic) {
            let _ = hh_clicker_launcher::config::atomic_write(&diagnostics_file, &bytes);
        }
        let _ = tx.send(s.clone());
        ctx.request_repaint();
    };
    let _ = runtime.preflight();
    publish(&runtime.state);
    while let Ok(action) = rx
        .recv_timeout(Duration::from_secs(3))
        .or_else(|e| match e {
            mpsc::RecvTimeoutError::Timeout => Ok(Action::Tick),
            other => Err(other),
        })
    {
        let close = matches!(action, Action::Close);
        runtime.state.closing = close;
        let refresh = matches!(action, Action::Tick);
        let environment_check = matches!(action, Action::Refresh | Action::Prepare(_));
        if matches!(action, Action::Close | Action::Stop) {
            runtime.cancellation.store(false, Ordering::Release);
        }
        runtime.state.revision += 1;
        runtime.state.operation = match &action {
            Action::Stop | Action::Close => Operation::Stopping,
            Action::Start => Operation::Starting,
            Action::Refresh | Action::Tick => Operation::Idle,
            _ => Operation::Other,
        };
        runtime.state.busy = !refresh;
        if !refresh {
            runtime.state.busy = true;
            runtime.state.error = None;
            runtime.state.message = if close {
                if runtime.needs_shutdown() {
                    "Останавливаем сервисы перед закрытием…"
                } else {
                    "Закрываем лаунчер…"
                }
            } else {
                "Выполняем действие…"
            }
            .into();
            if runtime.state.operation == Operation::Stopping && runtime.needs_shutdown() {
                let mut transition = runtime.state.clone();
                transition.mark_stopping();
                publish(&transition);
            } else {
                publish(&runtime.state);
            }
        }
        let result = match action {
            Action::Refresh => runtime.preflight(),
            Action::Tick => runtime.observe(),
            Action::Start => runtime.start(publish),
            Action::Stop => runtime.stop(publish),
            Action::Ports => runtime.free_ports(),
            Action::Prepare(action) => {
                if runtime.offline {
                    Err(anyhow::anyhow!("Действие отключено в автономном просмотре"))
                } else {
                    let layout = runtime.layout.clone();
                    let cancellation = runtime.cancellation.clone();
                    let result =
                        setup::prepare_cancellable(&layout, action, &cancellation, |text| {
                            runtime.state.message = text.into();
                            publish(&runtime.state);
                        });
                    let message = runtime.state.message.clone();
                    let checked = if cancellation.load(Ordering::Acquire) {
                        Ok(())
                    } else {
                        runtime.preflight()
                    };
                    runtime.state.message = message;
                    result.and(checked)
                }
            }
            Action::Auth(path) => runtime.auth_action(path),
            Action::Close => runtime.stop(publish),
        };
        runtime.state.busy = false;
        runtime.state.operation = Operation::Idle;
        runtime.state.revision += 1;
        if let Err(e) = result {
            runtime.state.error = Some(format!("{e:#}"));
        }
        if environment_check {
            runtime.state.environment_revision += 1;
        }
        if close {
            if runtime.state.error.is_none() {
                runtime.state.message = "Лаунчер закрыт".into();
                runtime.state.closing = false;
                publish(&runtime.state);
                let _ = closed.send(());
                ctx.request_repaint();
                return;
            }
        }
        publish(&runtime.state);
    }
}
