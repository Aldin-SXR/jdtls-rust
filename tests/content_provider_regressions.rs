//! Provider policy regressions. These are additional tests, not upstream ports.
#[path = "../src/features/content_provider.rs"]
mod content_provider;
use content_provider::*;
use futures::future::BoxFuture;
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

enum Answer {
    Empty,
    Text(&'static str),
    Error(&'static str),
    Cancel,
}
#[derive(Default)]
struct State {
    answers: Mutex<VecDeque<Answer>>,
    preferences: Mutex<Option<Arc<Preferences>>>,
    creations: AtomicUsize,
}
impl State {
    fn new(answers: impl IntoIterator<Item = Answer>) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into_iter().collect()),
            ..Self::default()
        })
    }
}
struct Provider(Arc<State>);
impl ContentProvider for Provider {
    fn is_decompiler(&self) -> bool {
        true
    }
    fn set_preferences(&mut self, preferences: Arc<Preferences>) {
        *self.0.preferences.lock().unwrap() = Some(preferences);
    }
    fn provide<'a>(
        &'a mut self,
        _: Source<'a>,
        monitor: &'a Monitor,
    ) -> BoxFuture<'a, Result<Option<DecompilerResult>, String>> {
        Box::pin(async move {
            match self
                .0
                .answers
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Answer::Empty)
            {
                Answer::Empty => Ok(None),
                Answer::Text(s) => Ok(Some(DecompilerResult::text(s))),
                Answer::Error(s) => Err(s.into()),
                Answer::Cancel => {
                    monitor.set_canceled(true);
                    Ok(Some(DecompilerResult::text("Canceled")))
                }
            }
        })
    }
}
fn descriptor(id: &str, priority: Option<&str>, state: Arc<State>) -> Descriptor<'static> {
    Descriptor::new(id, priority, None, move || {
        state.creations.fetch_add(1, Ordering::SeqCst);
        Ok(Some(Box::new(Provider(state.clone()))))
    })
    .unwrap()
}
fn content(manager: &Manager<'_>, monitor: &Monitor) -> String {
    futures::executor::block_on(manager.get_content(Some("file:///BigDecimal.class"), monitor))
        .unwrap()
}

#[test]
fn preferred_ids_override_priorities_and_receive_current_preferences() {
    let source = State::new([Answer::Text("attached source")]);
    let preferred = State::new([Answer::Text("decompiled source")]);
    let prefs = Arc::new(Preferences {
        preferred: Some(vec!["fernflower".into(), "source".into()]),
    });
    let manager = Manager::new(
        prefs.clone(),
        vec![
            descriptor("source", Some("0"), source.clone()),
            descriptor("fernflower", Some("2147483647"), preferred.clone()),
        ],
    );
    assert_eq!("decompiled source", content(&manager, &Monitor::default()));
    assert_eq!(0, source.creations.load(Ordering::SeqCst));
    assert!(Arc::ptr_eq(
        &prefs,
        preferred.preferences.lock().unwrap().as_ref().unwrap()
    ));
    assert!(manager.events().is_empty());
}

#[test]
fn errors_fall_through_and_duplicate_priorities_are_reported() {
    let manager = Manager::new(
        Arc::new(Preferences::default()),
        vec![
            descriptor(
                "fakeContentProvider",
                None,
                State::new([Answer::Error("Something bad happened here")]),
            ),
            descriptor("fakeContentProvider2", None, State::new([Answer::Empty])),
            descriptor(
                "fernflower",
                Some("2147483647"),
                State::new([Answer::Text("decompiled source")]),
            ),
        ],
    );
    assert_eq!("decompiled source", content(&manager, &Monitor::default()));
    assert_eq!(manager.events(), vec![
        Event::Error("Error getting content via fakeContentProvider: Something bad happened here".into()),
        Event::Error("You have more than one content provider installed: [fakeContentProvider, fakeContentProvider2]. Please use the \"java.contentProvider.preferred\" setting to choose which one you want to use.".into()),
    ]);
}

#[test]
fn canceled_requests_discard_provider_results_and_stop_fallback() {
    let cancel = State::new([Answer::Cancel]);
    let fallback = State::new([Answer::Text("fallback")]);
    let manager = Manager::new(
        Arc::new(Preferences::default()),
        vec![
            descriptor("cancel", Some("1"), cancel.clone()),
            descriptor("fallback", Some("2"), fallback.clone()),
        ],
    );
    let monitor = Monitor::default();
    monitor.set_canceled(true);
    assert_eq!("", content(&manager, &monitor));
    assert_eq!(0, cancel.creations.load(Ordering::SeqCst));
    monitor.set_canceled(false);
    assert_eq!("", content(&manager, &monitor));
    assert!(monitor.is_canceled());
    assert_eq!(0, fallback.creations.load(Ordering::SeqCst));
}

#[test]
fn successive_requests_construct_providers_and_do_not_cache_content() {
    let state = State::new([
        Answer::Text("some value"),
        Answer::Text("something else"),
        Answer::Text(""),
    ]);
    let fallback = State::new([Answer::Text("must not replace an empty result")]);
    let manager = Manager::new(
        Arc::new(Preferences::default()),
        vec![
            descriptor("fake", Some("1"), state.clone()),
            descriptor("fallback", Some("2"), fallback.clone()),
        ],
    );
    let monitor = Monitor::default();
    assert_eq!("some value", content(&manager, &monitor));
    assert_eq!("something else", content(&manager, &monitor));
    assert_eq!("", content(&manager, &monitor));
    assert_eq!(3, state.creations.load(Ordering::SeqCst));
    assert_eq!(0, fallback.creations.load(Ordering::SeqCst));
}
