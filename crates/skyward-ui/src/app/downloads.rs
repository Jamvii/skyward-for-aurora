//! The independent download manager: browser downloads and links typed by
//! hand, all in one list.

use super::*;

/// Where a download came from, which decides how it is stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadSource {
    Aera,
    Custom,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownloadState {
    Downloading,
    Done,
    Failed(String),
    Cancelled,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ManagerDownload {
    pub id: u64,
    pub source: DownloadSource,
    pub name: String,
    pub url: String,
    pub path: String,
    pub done: u64,
    pub total: Option<u64>,
    pub state: DownloadState,
}

impl ManagerDownload {
    pub fn share(&self) -> Option<f32> {
        self.total.filter(|&t| t > 0).map(|t| (self.done as f32 / t as f32).clamp(0.0, 1.0))
    }

    pub fn active(&self) -> bool {
        self.state == DownloadState::Downloading
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ManagerDownloads {
    pub items: Vec<ManagerDownload>,
    /// The address typed on the custom-link page.
    pub address: String,
    pub error: Option<String>,
    next_custom: u64,
}

impl Default for ManagerDownloads {
    fn default() -> Self {
        Self { items: Vec::new(), address: String::new(), error: None, next_custom: 1_000_000 }
    }
}

impl ManagerDownloads {
    pub fn next_custom_id(&mut self) -> u64 {
        let id = self.next_custom;
        self.next_custom += 1;
        id
    }

    pub fn add_custom(&mut self, id: u64, url: &str, name: &str, path: &str) {
        self.items.push(ManagerDownload {
            id,
            source: DownloadSource::Custom,
            name: name.into(),
            url: url.into(),
            path: path.into(),
            done: 0,
            total: None,
            state: DownloadState::Downloading,
        });
    }

    pub fn find(&mut self, id: u64) -> Option<&mut ManagerDownload> {
        self.items.iter_mut().find(|d| d.id == id)
    }

    pub fn aera_started(&mut self, id: u64, name: &str, total: Option<u64>) {
        let path = format!("/sdcard/AERA/Downloads/{name}");
        if let Some(item) = self.find(id) {
            item.name = name.into();
            item.path = path;
            item.total = total;
            item.state = DownloadState::Downloading;
            return;
        }
        self.items.push(ManagerDownload {
            id,
            source: DownloadSource::Aera,
            name: name.into(),
            url: String::new(),
            path,
            done: 0,
            total,
            state: DownloadState::Downloading,
        });
    }

    pub fn aera_progress(&mut self, id: u64, done: u64, total: Option<u64>) {
        if let Some(item) = self.find(id) {
            item.done = done;
            if total.is_some() {
                item.total = total;
            }
        }
    }

    pub fn aera_ended(&mut self, id: u64, done: u64, total: Option<u64>, failure: Option<String>) {
        if let Some(item) = self.find(id) {
            item.done = done;
            if total.is_some() {
                item.total = total;
            }
            item.state = match failure {
                Some(why) if why == "Cancelled" => DownloadState::Cancelled,
                Some(why) => DownloadState::Failed(why),
                None => DownloadState::Done,
            };
        }
    }

    pub fn progress(&mut self, id: u64, done: u64, total: Option<u64>) {
        if let Some(item) = self.find(id) {
            item.done = done;
            if total.is_some() {
                item.total = total;
            }
        }
    }

    pub fn ended(&mut self, id: u64, ended: Result<u64, String>) {
        if let Some(item) = self.find(id) {
            match ended {
                Ok(size) => {
                    item.done = size;
                    item.total.get_or_insert(size);
                    item.state = DownloadState::Done;
                }
                Err(why) if why == "stopped as asked" => item.state = DownloadState::Cancelled,
                Err(why) => item.state = DownloadState::Failed(why),
            }
        }
    }
}

impl App {
    pub fn aera_download(&mut self, id: u64, event: crate::app::DownloadEventKind, name: &str, done: u64, total: Option<u64>, error: Option<String>) {
        match event {
            crate::app::DownloadEventKind::Started => self.state.downloads.aera_started(id, name, total),
            crate::app::DownloadEventKind::Progress => self.state.downloads.aera_progress(id, done, total),
            crate::app::DownloadEventKind::Finished => self.state.downloads.aera_ended(id, done, total, None),
            crate::app::DownloadEventKind::Failed => self.state.downloads.aera_ended(id, done, total, error),
            crate::app::DownloadEventKind::Cancelled => self.state.downloads.aera_ended(id, done, total, Some("Cancelled".into())),
        }
        self.refresh();
    }

    pub fn manager_download_progress(&mut self, id: u64, done: u64, total: Option<u64>) {
        self.state.downloads.progress(id, done, total);
        self.refresh();
    }

    pub fn manager_download_ended(&mut self, id: u64, ended: Result<u64, String>) {
        self.state.downloads.ended(id, ended);
        self.refresh();
    }
}
