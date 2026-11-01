//! Navigation history (per-tab back/forward stack).

#[derive(Clone, Default)]
pub struct History {
    /// Entries oldest → newest. `cursor` points at the current entry.
    entries: Vec<String>,
    cursor: Option<usize>,
    /// Titles for each entry, in parallel with `entries`.
    titles: Vec<String>,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_back(&mut self, url: &str) {
        if let Some(c) = self.cursor {
            self.entries.truncate(c + 1);
            self.titles.truncate(c + 1);
        }
        self.entries.push(url.to_string());
        self.titles.push(String::new());
        self.cursor = Some(self.entries.len() - 1);
    }

    pub fn back(&mut self) -> Option<String> {
        let c = self.cursor?;
        if c == 0 {
            return None;
        }
        self.cursor = Some(c - 1);
        Some(self.entries[c - 1].clone())
    }

    pub fn forward(&mut self) -> Option<String> {
        let c = self.cursor?;
        if c + 1 >= self.entries.len() {
            return None;
        }
        self.cursor = Some(c + 1);
        Some(self.entries[c + 1].clone())
    }

    pub fn clear_forward(&mut self) {
        if let Some(c) = self.cursor {
            self.entries.truncate(c + 1);
            self.titles.truncate(c + 1);
        }
    }

    pub fn visit(&mut self, url: &str, title: &str) {
        // Update the title of the current entry if it matches.
        if let Some(c) = self.cursor {
            if self.entries.get(c).map(|s| s == url).unwrap_or(false) {
                self.titles[c] = title.to_string();
            }
        }
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    pub fn titles(&self) -> &[String] {
        &self.titles
    }

    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }
}
