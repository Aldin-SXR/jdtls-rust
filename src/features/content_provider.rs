//! ContentProviderManager: provider registration, preference ordering and fallback.
//! Providers are constructed for each request; only individual decompilers may cache.

use futures::future::BoxFuture;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

pub const DECOMPILER_HEADER: &str = "// Source code is decompiled from a .class file using FernFlower decompiler (from Intellij IDEA).\n";

#[derive(Debug, Clone, Default)]
pub struct Preferences {
    pub preferred: Option<Vec<String>>,
}

#[derive(Debug, Default)]
pub struct Monitor(AtomicBool);
impl Monitor {
    pub fn is_canceled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    pub fn set_canceled(&self, canceled: bool) {
        self.0.store(canceled, Ordering::Release);
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecompilerResult {
    pub content: String,
    pub original_line_mappings: Option<Vec<i32>>,
    pub decompiled_line_mappings: Option<Vec<i32>>,
    #[serde(skip)]
    pub attached_source: bool,
}
impl DecompilerResult {
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            ..Self::default()
        }
    }
}

/// FernFlower supplies raw line pairs. Rust adds the header offset, sorts,
/// deduplicates and builds both mapping directions as the upstream adapter does.
pub fn fernflower_result(content: String, raw: Option<&[i32]>) -> DecompilerResult {
    let mut original: BTreeMap<i32, BTreeSet<i32>> = BTreeMap::new();
    let mut decompiled: BTreeMap<i32, BTreeSet<i32>> = BTreeMap::new();
    for pair in raw.unwrap_or_default().chunks_exact(2) {
        let dest = pair[1] + 1;
        original.entry(pair[0]).or_default().insert(dest);
        decompiled.entry(dest).or_default().insert(pair[0]);
    }
    let flatten = |map: BTreeMap<i32, BTreeSet<i32>>| {
        map.into_iter()
            .flat_map(|(from, values)| values.into_iter().flat_map(move |to| [from, to]))
            .collect()
    };
    DecompilerResult {
        content: format!("{DECOMPILER_HEADER}{content}"),
        original_line_mappings: Some(flatten(original)),
        decompiled_line_mappings: Some(flatten(decompiled)),
        attached_source: false,
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Source<'a> {
    Uri(&'a str),
    ClassFile(&'a str),
}
impl Source<'_> {
    pub fn uri(&self) -> &str {
        match self {
            Self::Uri(u) | Self::ClassFile(u) => u,
        }
    }
    fn interface(&self) -> &'static str {
        match self {
            Self::Uri(_) => "IContentProvider",
            Self::ClassFile(_) => "IDecompiler",
        }
    }
}

pub trait ContentProvider: Send + Sync {
    fn is_decompiler(&self) -> bool {
        false
    }
    fn set_preferences(&mut self, _preferences: Arc<Preferences>) {}
    fn provide<'a>(
        &'a mut self,
        source: Source<'a>,
        monitor: &'a Monitor,
    ) -> BoxFuture<'a, Result<Option<DecompilerResult>, String>>;
}

pub struct Descriptor<'env> {
    pub id: String,
    base_priority: i32,
    pattern: Regex,
    // None represents an extension object that does not implement IContentProvider.
    factory: Box<
        dyn Fn() -> Result<Option<Box<dyn ContentProvider + 'env>>, String> + Send + Sync + 'env,
    >,
}
impl<'env> Descriptor<'env> {
    pub fn new(
        id: &str,
        priority: Option<&str>,
        pattern: Option<&str>,
        factory: impl Fn() -> Result<Option<Box<dyn ContentProvider + 'env>>, String>
            + Send
            + Sync
            + 'env,
    ) -> Result<Self, regex::Error> {
        Ok(Self {
            id: id.into(),
            base_priority: priority.and_then(|p| p.parse().ok()).unwrap_or(500),
            pattern: Regex::new(pattern.unwrap_or(r".*\.class.*"))?,
            factory: Box::new(factory),
        })
    }
    fn priority(&self, preferences: &Preferences) -> i32 {
        preferences
            .preferred
            .as_ref()
            .and_then(|ids| ids.iter().position(|id| id == &self.id))
            .map_or(self.base_priority, |i| i as i32 + 1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Error(String),
    Info(String),
}

pub struct Manager<'env> {
    pub preferences: Arc<Preferences>,
    descriptors: Vec<Descriptor<'env>>,
    events: Mutex<Vec<Event>>,
}
impl<'env> Manager<'env> {
    pub fn new(preferences: Arc<Preferences>, descriptors: Vec<Descriptor<'env>>) -> Self {
        Self {
            preferences,
            descriptors,
            events: Mutex::new(Vec::new()),
        }
    }
    pub fn events(&self) -> Vec<Event> {
        self.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    fn log(&self, event: Event) {
        match &event {
            Event::Error(s) => tracing::error!("{s}"),
            Event::Info(s) => tracing::debug!("{s}"),
        }
        self.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(event);
    }
    pub async fn get_content(&self, uri: Option<&str>, monitor: &Monitor) -> Option<String> {
        Some(self.get_content_result(uri?, monitor).await.content)
    }
    pub async fn get_source(&self, class_file: Option<&str>, monitor: &Monitor) -> Option<String> {
        Some(self.get_source_result(class_file, monitor).await?.content)
    }
    pub async fn get_source_result(
        &self,
        class_file: Option<&str>,
        monitor: &Monitor,
    ) -> Option<DecompilerResult> {
        Some(self.dispatch(Source::ClassFile(class_file?), monitor).await)
    }
    pub async fn get_content_result(&self, uri: &str, monitor: &Monitor) -> DecompilerResult {
        self.dispatch(Source::Uri(uri), monitor).await
    }
    async fn dispatch(&self, source: Source<'_>, monitor: &Monitor) -> DecompilerResult {
        let uri = match source {
            Source::Uri(uri) => Some(uri),
            Source::ClassFile(_) => None,
        };
        let mut matches: Vec<_> = self
            .descriptors
            .iter()
            .filter(|d| uri.is_none_or(|u| d.pattern.is_match(u)))
            .collect();
        if self.descriptors.is_empty() {
            self.log(Event::Error("No content providers found".into()));
        } else if matches.is_empty() {
            self.log(Event::Error(format!(
                "Unable to find content provider for URI {}",
                source.uri()
            )));
        }
        matches.sort_by_key(|d| d.priority(&self.preferences));
        if monitor.is_canceled() {
            return DecompilerResult::default();
        }
        let mut previous_priority = -1;
        for descriptor in &matches {
            let mut provider = match (descriptor.factory)() {
                Ok(Some(provider)) => Some(provider),
                Ok(None) => {
                    self.log(Event::Error("Invalid extension to org.eclipse.jdt.ls.core.contentProvider. Must implement org.eclipse.jdt.ls.core.internal.IContentProvider".into()));
                    None
                }
                Err(error) => {
                    self.log(Event::Error(format!(
                        "Unable to create content provider {error}"
                    )));
                    None
                }
            };
            if provider
                .as_ref()
                .is_none_or(|p| matches!(source, Source::ClassFile(_)) && !p.is_decompiler())
            {
                self.log(Event::Info(format!(
                    "{} doesn't match {}. Skipping.",
                    descriptor.id,
                    source.interface()
                )));
                continue;
            }
            if monitor.is_canceled() {
                return DecompilerResult::default();
            }
            let priority = descriptor.priority(&self.preferences);
            if previous_priority == priority {
                let ids: Vec<_> = matches
                    .iter()
                    .filter(|d| d.priority(&self.preferences) == priority)
                    .map(|d| d.id.as_str())
                    .collect();
                self.log(Event::Error(format!("You have more than one content provider installed: [{}]. Please use the \"java.contentProvider.preferred\" setting to choose which one you want to use.", ids.join(", "))));
            }
            let provider = provider.as_mut().unwrap();
            provider.set_preferences(self.preferences.clone());
            let result = provider.provide(source, monitor).await;
            // Upstream exceptions are logged before the next cancellation check.
            match result {
                Err(error) => self.log(Event::Error(format!(
                    "Error getting content via {}: {error}",
                    descriptor.id
                ))),
                Ok(result) => {
                    if monitor.is_canceled() {
                        return DecompilerResult::default();
                    }
                    if let Some(result) = result {
                        return result;
                    }
                }
            }
            previous_priority = priority;
        }
        DecompilerResult::default()
    }
}
