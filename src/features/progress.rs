//! Port of jdt.ls `ProgressReporterManager` and `CancellableProgressMonitor`:
//! progress monitors that report to the client through `language/progressReport`,
//! `$/progress` work-done progress or the legacy `language/status` messages.

#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tower_lsp::lsp_types::notification::{Notification, Progress};
use tower_lsp::lsp_types::request::WorkDoneProgressCreate;
use tower_lsp::lsp_types::{
    NumberOrString, ProgressParams, ProgressParamsValue, WorkDoneProgress, WorkDoneProgressBegin,
    WorkDoneProgressCreateParams, WorkDoneProgressEnd, WorkDoneProgressReport,
};
use tower_lsp::Client;

pub const IMPORTING_MAVEN_PROJECTS: &str = "Importing Maven project(s)";
const SEPARATOR: &str = " - ";
const JOBS_BLOCKED: &str = "The user operation is waiting for background work to complete.";

/// `ProgressReport`, sent as `language/progressReport`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProgressReport {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(rename = "subTask", skip_serializing_if = "Option::is_none")]
    pub sub_task: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(rename = "totalWork")]
    pub total_work: i32,
    #[serde(rename = "workDone")]
    pub work_done: i32,
    pub complete: bool,
}

impl ProgressReport {
    pub fn new(id: &str) -> Self {
        Self { id: id.to_owned(), ..Self::default() }
    }
}

/// `StatusReport`, sent as `language/status`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StatusReport {
    #[serde(rename = "type")]
    pub typ: String,
    pub message: String,
}

pub enum ProgressReportNotification {}

impl Notification for ProgressReportNotification {
    type Params = ProgressReport;
    const METHOD: &'static str = "language/progressReport";
}

pub enum StatusReportNotification {}

impl Notification for StatusReportNotification {
    type Params = StatusReport;
    const METHOD: &'static str = "language/status";
}

/// The progress related calls of `JavaLanguageClient`.
pub trait JavaLanguageClient: Send + Sync {
    fn send_progress_report(&self, report: ProgressReport);
    fn send_status_report(&self, report: StatusReport);
    fn create_progress(&self, params: WorkDoneProgressCreateParams);
    fn notify_progress(&self, params: ProgressParams);
}

/// The progress related questions asked of `ClientPreferences`.
pub trait ClientPreferences: Send + Sync {
    fn is_progress_report_supported(&self) -> bool;
    fn is_work_done_progress_supported(&self) -> bool;
}

/// `PreferenceManager.getClientPreferences()`.
pub trait PreferenceManager: Send + Sync {
    fn client_preferences(&self) -> Option<Arc<dyn ClientPreferences>>;
}

impl ClientPreferences for crate::features::client_caps::ClientPreferences {
    fn is_progress_report_supported(&self) -> bool {
        self.extended_flag("progressReportProvider")
    }

    fn is_work_done_progress_supported(&self) -> bool {
        crate::features::client_caps::ClientPreferences::is_work_done_progress_supported(self)
    }
}

enum Outgoing {
    ProgressReport(ProgressReport),
    StatusReport(StatusReport),
    CreateProgress(WorkDoneProgressCreateParams),
    NotifyProgress(ProgressParams),
}

/// A `JavaLanguageClient` over the LSP connection. Messages are delivered in
/// the order they were sent.
pub struct LspLanguageClient {
    tx: tokio::sync::mpsc::UnboundedSender<Outgoing>,
}

impl LspLanguageClient {
    pub fn new(client: Client) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Outgoing>();
        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                match message {
                    Outgoing::ProgressReport(r) => client.send_notification::<ProgressReportNotification>(r).await,
                    Outgoing::StatusReport(r) => client.send_notification::<StatusReportNotification>(r).await,
                    Outgoing::CreateProgress(p) => {
                        let _ = client.send_request::<WorkDoneProgressCreate>(p).await;
                    }
                    Outgoing::NotifyProgress(p) => client.send_notification::<Progress>(p).await,
                }
            }
        });
        Self { tx }
    }
}

impl JavaLanguageClient for LspLanguageClient {
    fn send_progress_report(&self, report: ProgressReport) {
        let _ = self.tx.send(Outgoing::ProgressReport(report));
    }

    fn send_status_report(&self, report: StatusReport) {
        let _ = self.tx.send(Outgoing::StatusReport(report));
    }

    fn create_progress(&self, params: WorkDoneProgressCreateParams) {
        let _ = self.tx.send(Outgoing::CreateProgress(params));
    }

    fn notify_progress(&self, params: ProgressParams) {
        let _ = self.tx.send(Outgoing::NotifyProgress(params));
    }
}

/// `org.eclipse.core.runtime.jobs.Job`, reduced to what progress reporting asks.
#[derive(Debug, Clone, Default)]
pub struct Job {
    pub name: String,
    pub system: bool,
    /// `belongsTo(InitHandler.JAVA_LS_INITIALIZATION_JOBS)`.
    pub initialization: bool,
}

impl Job {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_owned(), ..Self::default() }
    }

    pub fn initialization(mut self) -> Self {
        self.initialization = true;
        self
    }
}

/// `IProgressMonitor`.
pub trait ProgressMonitor: Send + Sync {
    fn begin_task(&self, name: &str, total_work: i32);
    fn done(&self);
    fn is_canceled(&self) -> bool;
    fn internal_worked(&self, work: f64);
    fn set_canceled(&self, canceled: bool);
    fn set_task_name(&self, name: &str);
    fn sub_task(&self, name: &str);
    fn worked(&self, work: i32);
}

/// lsp4j's `CancelChecker`; `Err` is the `CancellationException`.
pub trait CancelChecker: Send + Sync {
    fn check_canceled(&self) -> Result<(), CancellationException>;
}

#[derive(Debug, PartialEq, Eq)]
pub struct CancellationException;

/// `CancellableProgressMonitor`: a `NullProgressMonitor` that is also
/// cancelled when its `CancelChecker` throws.
pub struct CancellableProgressMonitor {
    cancel_checker: Option<Arc<dyn CancelChecker>>,
    canceled: AtomicBool,
    done: AtomicBool,
}

impl CancellableProgressMonitor {
    pub fn new(checker: Option<Arc<dyn CancelChecker>>) -> Self {
        Self { cancel_checker: checker, canceled: AtomicBool::new(false), done: AtomicBool::new(false) }
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }
}

impl ProgressMonitor for CancellableProgressMonitor {
    fn begin_task(&self, _: &str, _: i32) {}

    fn done(&self) {
        self.done.store(true, Ordering::SeqCst);
    }

    fn is_canceled(&self) -> bool {
        if self.canceled.load(Ordering::SeqCst) {
            return true;
        }
        self.cancel_checker.as_ref().is_some_and(|checker| checker.check_canceled().is_err())
    }

    fn internal_worked(&self, _: f64) {}

    fn set_canceled(&self, canceled: bool) {
        self.canceled.store(canceled, Ordering::SeqCst);
    }

    fn set_task_name(&self, _: &str) {}

    fn sub_task(&self, _: &str) {}

    fn worked(&self, _: i32) {}
}

struct Shared {
    client: Option<Arc<dyn JavaLanguageClient>>,
    preference_manager: Option<Arc<dyn PreferenceManager>>,
    delay_millis: AtomicU64,
}

/// `ProgressReporterManager`.
#[derive(Clone)]
pub struct ProgressReporterManager {
    shared: Arc<Shared>,
}

impl ProgressReporterManager {
    pub fn new(client: Option<Arc<dyn JavaLanguageClient>>, preference_manager: Option<Arc<dyn PreferenceManager>>) -> Self {
        Self { shared: Arc::new(Shared { client, preference_manager, delay_millis: AtomicU64::new(200) }) }
    }

    pub fn create_monitor(&self, job: Job) -> Arc<dyn ProgressMonitor> {
        if job.initialization {
            let monitors: Vec<Arc<dyn ProgressMonitor>> = vec![
                Arc::new(ProgressReporter::new(&self.shared, Kind::ServerStatus, None, None)),
                self.create_job_monitor(job),
            ];
            return Arc::new(MulticastProgressReporter { monitors });
        }
        self.create_job_monitor(job)
    }

    fn create_job_monitor(&self, job: Job) -> Arc<dyn ProgressMonitor> {
        Arc::new(ProgressReporter::new(&self.shared, Kind::Progress, Some(job), None))
    }

    pub fn default_monitor(&self) -> Arc<dyn ProgressMonitor> {
        Arc::new(ProgressReporter::new(&self.shared, Kind::Progress, None, None))
    }

    pub fn progress_reporter(&self, checker: Arc<dyn CancelChecker>) -> Arc<dyn ProgressMonitor> {
        Arc::new(ProgressReporter::new(&self.shared, Kind::Progress, None, Some(checker)))
    }

    pub fn create_progress_group(&self) -> Arc<dyn ProgressMonitor> {
        self.default_monitor()
    }

    pub fn set_report_throttle(&self, delay_millis: u64) {
        self.shared.delay_millis.store(delay_millis, Ordering::SeqCst);
    }
}

struct MulticastProgressReporter {
    monitors: Vec<Arc<dyn ProgressMonitor>>,
}

impl ProgressMonitor for MulticastProgressReporter {
    fn begin_task(&self, name: &str, total_work: i32) {
        self.monitors.iter().for_each(|m| m.begin_task(name, total_work));
    }

    fn done(&self) {
        self.monitors.iter().for_each(|m| m.done());
    }

    fn is_canceled(&self) -> bool {
        self.monitors.iter().all(|m| m.is_canceled())
    }

    fn internal_worked(&self, work: f64) {
        self.monitors.iter().for_each(|m| m.internal_worked(work));
    }

    fn set_canceled(&self, canceled: bool) {
        self.monitors.iter().for_each(|m| m.set_canceled(canceled));
    }

    fn set_task_name(&self, name: &str) {
        self.monitors.iter().for_each(|m| m.set_task_name(name));
    }

    fn sub_task(&self, name: &str) {
        self.monitors.iter().for_each(|m| m.sub_task(name));
    }

    fn worked(&self, work: i32) {
        self.monitors.iter().for_each(|m| m.worked(work));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Progress,
    /// `ServerStatusMonitor`: the legacy `Starting` status messages.
    ServerStatus,
}

struct State {
    task_name: Option<String>,
    sub_task_name: Option<String>,
    total_work: i32,
    progress: i32,
    last_report: Option<Instant>,
    progress_id: String,
    sent_begin: bool,
    sent_end: bool,
}

struct ProgressReporter {
    shared: Arc<Shared>,
    kind: Kind,
    job: Option<Job>,
    base: CancellableProgressMonitor,
    state: Mutex<State>,
}

fn new_progress_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!("{:08x}-{:04x}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Java's `%.0f` (HALF_UP) of `progress / total * 100`, including `NaN` and `Infinity`.
fn percent(progress: i32, total: i32) -> String {
    let value = f64::from(progress) / f64::from(total) * 100.0;
    if value.is_nan() {
        "NaN".to_owned()
    } else if value.is_infinite() {
        "Infinity".to_owned()
    } else {
        format!("{}", (value + 0.5).floor())
    }
}

impl ProgressReporter {
    fn new(shared: &Arc<Shared>, kind: Kind, job: Option<Job>, checker: Option<Arc<dyn CancelChecker>>) -> Self {
        Self {
            shared: Arc::clone(shared),
            kind,
            job,
            base: CancellableProgressMonitor::new(checker),
            state: Mutex::new(State {
                task_name: None,
                sub_task_name: None,
                total_work: 0,
                progress: 0,
                last_report: None,
                progress_id: new_progress_id(),
                sent_begin: false,
                sent_end: false,
            }),
        }
    }

    fn is_done_in(&self, state: &State) -> bool {
        self.base.is_done() || (state.total_work > 0 && state.progress >= state.total_work)
    }

    fn send_progress(&self, state: &mut State) {
        if self.job.as_ref().is_some_and(|j| j.system) || state.task_name.as_deref() == Some(JOBS_BLOCKED) {
            return;
        }
        let now = Instant::now();
        let delay = self.shared.delay_millis.load(Ordering::SeqCst);
        let due = match state.last_report {
            None => true,
            Some(last) => self.is_done_in(state) || now.duration_since(last).as_millis() >= u128::from(delay),
        };
        if due {
            state.last_report = Some(now);
            self.send_status(state);
        }
    }

    fn task(&self, state: &State) -> String {
        match state.task_name.as_deref() {
            Some(name) if !name.trim().is_empty() => name.to_owned(),
            _ => match &self.job {
                Some(job) if !job.name.trim().is_empty() => job.name.clone(),
                _ => "Background task".to_owned(),
            },
        }
    }

    fn message(state: &State) -> String {
        state.sub_task_name.clone().unwrap_or_default()
    }

    fn format_message(&self, state: &State) -> String {
        let message = Self::message(state);
        match self.kind {
            Kind::Progress => {
                if state.total_work > 0 {
                    format!("{}% {}", percent(state.progress, state.total_work), message)
                } else {
                    message
                }
            }
            Kind::ServerStatus => {
                let message = if state.total_work > 0 && !message.is_empty() { format!("{SEPARATOR}{message}") } else { message };
                format!("{}% Starting Java Language Server{}", percent(state.progress, state.total_work), message)
            }
        }
    }

    fn send_status(&self, state: &mut State) {
        let Some(client) = &self.shared.client else { return };
        if self.kind == Kind::ServerStatus {
            let message = self.format_message(state);
            client.send_status_report(StatusReport { typ: "Starting".to_owned(), message });
            return;
        }
        let Some(preferences) = self.shared.preference_manager.as_ref().and_then(|m| m.client_preferences()) else {
            return;
        };
        let task = self.task(state);
        let maven_sub_task = state.sub_task_name.as_deref().is_some_and(|s| !s.is_empty()) && task == IMPORTING_MAVEN_PROJECTS;
        if preferences.is_progress_report_supported() {
            let mut report = ProgressReport::new(&state.progress_id);
            report.task = Some(task.clone());
            report.sub_task = state.sub_task_name.clone();
            report.total_work = state.total_work;
            report.work_done = state.progress;
            report.complete = self.is_done_in(state);
            report.status = Some(if maven_sub_task {
                format!("{task}{SEPARATOR}{}", state.sub_task_name.as_deref().unwrap_or_default())
            } else {
                self.format_message(state)
            });
            client.send_progress_report(report);
        } else if preferences.is_work_done_progress_supported() && !state.sent_end {
            let token = NumberOrString::String(state.progress_id.clone());
            if !state.sent_begin {
                client.create_progress(WorkDoneProgressCreateParams { token: token.clone() });
                client.notify_progress(ProgressParams {
                    token: token.clone(),
                    value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(WorkDoneProgressBegin {
                        title: state.sub_task_name.clone().unwrap_or_else(|| task.clone()),
                        cancellable: None,
                        message: Some(task.clone()),
                        percentage: None,
                    })),
                });
                state.sent_begin = true;
            }
            let notification = if self.is_done_in(state) {
                let end = WorkDoneProgress::End(WorkDoneProgressEnd { message: Some(task.clone()) });
                state.progress_id = new_progress_id();
                state.sent_begin = false;
                state.sent_end = true;
                end
            } else {
                let message = if maven_sub_task {
                    format!("{task}{SEPARATOR}{}", state.sub_task_name.as_deref().unwrap_or_default())
                } else {
                    format!("{task}{SEPARATOR}{}", self.format_message(state))
                };
                let percentage = (f64::from(state.progress) / f64::from(state.total_work) * 100.0) as i32;
                WorkDoneProgress::Report(WorkDoneProgressReport {
                    cancellable: None,
                    message: Some(message),
                    percentage: Some(percentage.max(0) as u32),
                })
            };
            client.notify_progress(ProgressParams { token, value: ProgressParamsValue::WorkDone(notification) });
        }
    }
}

impl ProgressMonitor for ProgressReporter {
    fn begin_task(&self, name: &str, total_work: i32) {
        let mut state = self.state.lock().unwrap();
        state.task_name = Some(name.to_owned());
        state.total_work = total_work;
        self.send_progress(&mut state);
    }

    fn done(&self) {
        self.base.done();
        let mut state = self.state.lock().unwrap();
        self.send_progress(&mut state);
    }

    fn is_canceled(&self) -> bool {
        self.base.is_canceled()
    }

    fn internal_worked(&self, work: f64) {
        self.base.internal_worked(work);
    }

    fn set_canceled(&self, canceled: bool) {
        self.base.set_canceled(canceled);
    }

    fn set_task_name(&self, name: &str) {
        self.state.lock().unwrap().task_name = Some(name.to_owned());
    }

    fn sub_task(&self, name: &str) {
        let mut state = self.state.lock().unwrap();
        state.sub_task_name = Some(name.to_owned());
        if state.task_name.as_deref() == Some(IMPORTING_MAVEN_PROJECTS) && name.is_empty() {
            return;
        }
        self.send_progress(&mut state);
    }

    fn worked(&self, work: i32) {
        let mut state = self.state.lock().unwrap();
        state.progress += work;
        self.send_progress(&mut state);
    }
}

#[cfg(test)]
mod cancellable_progress_monitor_test {
    use super::*;

    struct Throwing;

    impl CancelChecker for Throwing {
        fn check_canceled(&self) -> Result<(), CancellationException> {
            Err(CancellationException)
        }
    }

    struct Quiet;

    impl CancelChecker for Quiet {
        fn check_canceled(&self) -> Result<(), CancellationException> {
            Ok(())
        }
    }

    #[test]
    fn test_cancelled() {
        assert!(CancellableProgressMonitor::new(Some(Arc::new(Throwing))).is_canceled());
    }

    #[test]
    fn test_not_cancelled() {
        assert!(!CancellableProgressMonitor::new(None).is_canceled());
        assert!(!CancellableProgressMonitor::new(Some(Arc::new(Quiet))).is_canceled());
    }
}

#[cfg(test)]
mod progress_reporter_manager_test {
    use super::*;

    #[derive(Debug, Clone)]
    enum Sent {
        Progress(ProgressReport),
        Status(StatusReport),
        Create(WorkDoneProgressCreateParams),
        Notify(ProgressParams),
    }

    #[derive(Default)]
    struct MockClient(Mutex<Vec<Sent>>);

    impl MockClient {
        fn progress_reports(&self) -> Vec<ProgressReport> {
            self.0.lock().unwrap().iter().filter_map(|s| if let Sent::Progress(p) = s { Some(p.clone()) } else { None }).collect()
        }

        fn status_reports(&self) -> Vec<StatusReport> {
            self.0.lock().unwrap().iter().filter_map(|s| if let Sent::Status(p) = s { Some(p.clone()) } else { None }).collect()
        }

        fn notified(&self) -> Vec<ProgressParams> {
            self.0.lock().unwrap().iter().filter_map(|s| if let Sent::Notify(p) = s { Some(p.clone()) } else { None }).collect()
        }
    }

    impl JavaLanguageClient for MockClient {
        fn send_progress_report(&self, report: ProgressReport) {
            self.0.lock().unwrap().push(Sent::Progress(report));
        }

        fn send_status_report(&self, report: StatusReport) {
            self.0.lock().unwrap().push(Sent::Status(report));
        }

        fn create_progress(&self, params: WorkDoneProgressCreateParams) {
            self.0.lock().unwrap().push(Sent::Create(params));
        }

        fn notify_progress(&self, params: ProgressParams) {
            self.0.lock().unwrap().push(Sent::Notify(params));
        }
    }

    struct MockClientPreferences {
        progress_report_supported: AtomicBool,
        work_done_progress_supported: AtomicBool,
    }

    impl ClientPreferences for MockClientPreferences {
        fn is_progress_report_supported(&self) -> bool {
            self.progress_report_supported.load(Ordering::SeqCst)
        }

        fn is_work_done_progress_supported(&self) -> bool {
            self.work_done_progress_supported.load(Ordering::SeqCst)
        }
    }

    struct MockPreferenceManager(Arc<MockClientPreferences>);

    impl PreferenceManager for MockPreferenceManager {
        fn client_preferences(&self) -> Option<Arc<dyn ClientPreferences>> {
            Some(self.0.clone())
        }
    }

    struct Fixture {
        manager: ProgressReporterManager,
        client: Arc<MockClient>,
        client_preferences: Arc<MockClientPreferences>,
    }

    fn setup() -> Fixture {
        let client = Arc::new(MockClient::default());
        let client_preferences = Arc::new(MockClientPreferences {
            progress_report_supported: AtomicBool::new(true),
            work_done_progress_supported: AtomicBool::new(false),
        });
        let preference_manager = Arc::new(MockPreferenceManager(client_preferences.clone()));
        let manager = ProgressReporterManager::new(Some(client.clone()), Some(preference_manager));
        Fixture { manager, client, client_preferences }
    }

    #[test]
    fn test_report_throttling() {
        let f = setup();
        f.manager.set_report_throttle(100);
        let monitor = f.manager.default_monitor();
        monitor.begin_task("Some task", 10);
        for _ in 0..10 {
            monitor.worked(1);
            std::thread::sleep(std::time::Duration::from_millis(40));
        }

        let reports = f.client.progress_reports();
        assert_eq!(4, reports.len());
        assert_eq!(0, reports[0].work_done);
        assert_eq!(4, reports[1].work_done);
        assert_eq!(7, reports[2].work_done);
        assert_eq!(10, reports[3].work_done);
        monitor.done();
    }

    #[test]
    fn test_job_reporting() {
        let f = setup();
        f.manager.set_report_throttle(275);
        let job = Job::new("Test Job");
        let monitor = f.manager.create_monitor(job.clone());
        monitor.done();

        let reports = f.client.progress_reports();
        assert_eq!(1, reports.len());
        let report = &reports[0];
        assert_eq!(Some(""), report.status.as_deref());
        assert_eq!(Some(job.name.as_str()), report.task.as_deref());
        assert!(report.complete);
    }

    #[test]
    fn test_job_reporting_with_notify_progress() {
        let f = setup();
        f.client_preferences.progress_report_supported.store(false, Ordering::SeqCst);
        f.client_preferences.work_done_progress_supported.store(true, Ordering::SeqCst);
        f.manager.set_report_throttle(275);
        let monitor = f.manager.create_monitor(Job::new("Test Job"));
        monitor.done();

        assert_eq!(2, f.client.notified().len());
    }

    #[test]
    fn test_multicast_job_reporting() {
        let f = setup();
        f.manager.set_report_throttle(275);
        let job = Job::new("Test Job").initialization();
        let monitor = f.manager.create_monitor(job.clone());
        monitor.done();

        let status_reports = f.client.status_reports();
        assert_eq!(1, status_reports.len());
        assert_eq!("Starting", status_reports[0].typ);

        let progress_reports = f.client.progress_reports();
        assert_eq!(1, progress_reports.len());
        let progress_report = &progress_reports[0];
        assert_eq!(Some(""), progress_report.status.as_deref());
        assert_eq!(Some(job.name.as_str()), progress_report.task.as_deref());
        assert!(progress_report.complete);
    }

    #[test]
    fn test_startup_job_reporting() {
        let f = setup();
        f.manager.set_report_throttle(0);
        let monitor = f.manager.create_monitor(Job::new("Startup job").initialization());

        monitor.begin_task("Do stuff", 10);
        monitor.worked(5);

        let reports = f.client.status_reports();
        assert_eq!(2, reports.len());
        assert_eq!("0% Starting Java Language Server", reports[0].message);
        assert_eq!("Starting", reports[0].typ);
        assert_eq!("50% Starting Java Language Server", reports[1].message);
        assert_eq!("Starting", reports[1].typ);
        monitor.done();
    }
}
