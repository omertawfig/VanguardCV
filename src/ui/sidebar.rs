//! Parent–Child CV library tree.
//!
//! **Owner:** Sub-Agent 3 (GPUI Desktop & Tuta Theme Engineer).
//!
//! Renders a collapsible tree of Root CVs (with 🛡️ shield badges) and their
//! Child Sub-Variations (with 🎯 target badges). Supports selection state,
//! expansion toggling, and affordance buttons for creating new roots and
//! variations.

use gpui::{
    Context, EventEmitter, IntoElement, MouseButton, MouseDownEvent, SharedString, Window, div,
    prelude::*, px, rgb,
};

use crate::contracts::{CvSummary, DocumentId, Services};
use crate::theme;
use crate::ui::text_input::{TextInput, TextInputEvent};

// ---------------------------------------------------------------------------
// Tree node model
// ---------------------------------------------------------------------------

/// A flattened row in the sidebar tree, ready for rendering.
#[derive(Debug, Clone)]
pub struct TreeRow {
    pub summary: CvSummary,
    /// `0` for roots, `1` for children.
    pub depth: usize,
    /// Whether the row is expanded (only meaningful for roots).
    pub expanded: bool,
}

/// Flattens the root/child hierarchy into a rendering-order list.
///
/// Roots appear in order, with their children listed directly after when
/// the root is expanded.
pub fn flatten_tree(
    roots: &[CvSummary],
    children_map: &std::collections::HashMap<DocumentId, Vec<CvSummary>>,
    expanded_set: &std::collections::HashSet<DocumentId>,
) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    for root in roots {
        let expanded = expanded_set.contains(&root.id);
        rows.push(TreeRow {
            summary: root.clone(),
            depth: 0,
            expanded,
        });
        if expanded && let Some(children) = children_map.get(&root.id) {
            for child in children {
                rows.push(TreeRow {
                    summary: child.clone(),
                    depth: 1,
                    expanded: false,
                });
            }
        }
    }
    rows
}

// ---------------------------------------------------------------------------
// Sidebar view
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum SidebarEvent {
    NewRootRequested,
    NewVariationRequested { root_id: DocumentId },
}

pub struct SidebarView {
    services: Services,
    roots: Vec<CvSummary>,
    children_map: std::collections::HashMap<DocumentId, Vec<CvSummary>>,
    expanded: std::collections::HashSet<DocumentId>,
    selected_id: Option<DocumentId>,
    search: gpui::Entity<TextInput>,
}

impl EventEmitter<SidebarEvent> for SidebarView {}

impl SidebarView {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            TextInput::new(cx).with_placeholder("Search CVs")
        });
        cx.subscribe(&search, |_this, _input, _event: &TextInputEvent, cx| {
            cx.notify();
        })
        .detach();

        let mut view = Self {
            services,
            roots: Vec::new(),
            children_map: std::collections::HashMap::new(),
            expanded: std::collections::HashSet::new(),
            selected_id: None,
            search,
        };
        view.reload_tree();
        // Auto-expand and select the first root if available.
        if let Some(first) = view.roots.first() {
            let id = first.id;
            view.expanded.insert(id);
            view.selected_id = Some(id);
        }
        view
    }

    /// Reloads the tree data from the repository.
    pub fn reload_tree(&mut self) {
        self.roots = self.services.repository.list_roots().unwrap_or_default();
        self.children_map.clear();
        for root in &self.roots {
            if let Ok(children) = self.services.repository.list_children(root.id) {
                self.children_map.insert(root.id, children);
            }
        }
    }

    /// Returns the currently selected document id.
    pub fn selected_id(&self) -> Option<DocumentId> {
        self.selected_id
    }

    /// Sets the selected document id and notifies observers.
    pub fn select(&mut self, id: DocumentId, cx: &mut Context<Self>) {
        self.selected_id = Some(id);
        cx.notify();
    }

    /// Selects a document and ensures its parent root is expanded if it is a child.
    pub fn select_and_reveal(&mut self, id: DocumentId, cx: &mut Context<Self>) {
        for (parent_id, children) in &self.children_map {
            if children.iter().any(|c| c.id == id) {
                self.expanded.insert(*parent_id);
                break;
            }
        }
        self.selected_id = Some(id);
        cx.notify();
    }

    /// Resolves which Root CV should be targeted for a new variation.
    ///
    /// If the selected document is a child, returns its parent root id.
    /// Otherwise returns the selected root id, or falls back to the first available root.
    pub fn resolve_selected_root_id(&self) -> Option<DocumentId> {
        if let Some(id) = self.selected_id {
            if self.roots.iter().any(|r| r.id == id) {
                return Some(id);
            }
            for (parent_id, children) in &self.children_map {
                if children.iter().any(|c| c.id == id) {
                    return Some(*parent_id);
                }
            }
        }
        self.roots.first().map(|r| r.id)
    }

    /// Creates a blank root CV via repository, reloads the tree, selects it,
    /// and emits `SidebarEvent::NewRootRequested`.
    pub fn create_new_root(&mut self, cx: &mut Context<Self>) {
        match self.services.repository.create_root(
            "Untitled Master CV",
            crate::contracts::PiiContactHeader::default(),
            crate::contracts::SanitizedCvBody::default(),
        ) {
            Ok(doc) => {
                self.reload_tree();
                self.select_and_reveal(doc.id, cx);
                cx.emit(SidebarEvent::NewRootRequested);
            }
            Err(err) => {
                tracing::error!("Failed to create new root CV: {err}");
            }
        }
    }

    /// Emits `SidebarEvent::NewVariationRequested` targeting the resolved root CV.
    pub fn request_new_variation(&mut self, cx: &mut Context<Self>) {
        if let Some(root_id) = self.resolve_selected_root_id() {
            cx.emit(SidebarEvent::NewVariationRequested { root_id });
        } else {
            tracing::warn!("No root CV available to create a variation from");
        }
    }

    fn toggle_expanded(&mut self, id: DocumentId, cx: &mut Context<Self>) {
        if self.expanded.contains(&id) {
            self.expanded.remove(&id);
        } else {
            self.expanded.insert(id);
        }
        cx.notify();
    }

    fn flattened_rows(&self) -> Vec<TreeRow> {
        flatten_tree(&self.roots, &self.children_map, &self.expanded)
    }
}

impl Render for SidebarView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme::colors();
        let t = theme::typography();
        let query = self.search.read(cx).text().trim().to_lowercase();
        let mut rows = self.flattened_rows();
        if !query.is_empty() {
            rows.retain(|row| row.summary.title.to_lowercase().contains(&query));
        }

        let mut content = div()
            .id("sidebar-scroll")
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(c.bg_surface))
            .overflow_y_scroll();

        // Header
        content = content.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px(px(12.0))
                .py(px(10.0))
                .child(
                    div()
                        .text_size(t.subheading)
                        .text_color(rgb(c.text_primary))
                        .child("📁 CV LIBRARY"),
                ),
        );

        content = content.child(theme::hairline());
        content = content.child(
            div()
                .px(px(8.0))
                .py(px(6.0))
                .child(self.search.clone()),
        );

        // Tree rows
        for row in &rows {
            let id = row.summary.id;
            let is_selected = self.selected_id == Some(id);
            let indent = px(12.0 + (row.depth as f32) * 20.0);
            let is_root = row.summary.is_root();
            let expanded = row.expanded;

            let icon: SharedString = if is_root {
                if expanded {
                    "▾ 🛡️"
                } else {
                    "▸ 🛡️"
                }
            } else {
                "  🎯"
            }
            .into();

            let title: SharedString = row.summary.display_title().into();

            let mut row_div = div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .w_full()
                .pl(indent)
                .pr(px(8.0))
                .py(px(6.0))
                .text_size(t.body)
                .cursor_pointer();

            if is_selected {
                row_div = row_div
                    .bg(rgb(c.tuta_crimson))
                    .text_color(rgb(c.text_on_accent));
            } else {
                row_div = row_div
                    .text_color(rgb(c.text_primary))
                    .hover(|s| s.bg(rgb(c.bg_input)));
            }

            row_div = row_div.child(div().flex_shrink_0().child(icon)).child(
                div()
                    .overflow_x_hidden()
                    .text_ellipsis()
                    .min_w_0()
                    .child(title),
            );

            // Click handler: for roots, toggle expand; for all, select
            if is_root {
                row_div = row_div.on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _event, _window, cx| {
                        this.toggle_expanded(id, cx);
                        this.selected_id = Some(id);
                        cx.notify();
                    }),
                );
            } else {
                row_div = row_div.on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _event, _window, cx| {
                        this.selected_id = Some(id);
                        cx.notify();
                    }),
                );
            }

            content = content.child(row_div);
        }

        // Spacer
        content = content.child(div().flex_grow());

        // Bottom affordances
        content = content.child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .p(px(8.0))
                .child(
                    div()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.create_new_root(cx);
                            }),
                        )
                        .child(
                            theme::button_ghost("+ New Root CV")
                                .w_full()
                                .justify_center(),
                        ),
                )
                .child(
                    div()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.request_new_variation(cx);
                            }),
                        )
                        .child(
                            theme::button_ghost("+ Variation")
                                .w_full()
                                .justify_center(),
                        ),
                ),
        );

        content
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::CvSummary;
    use std::collections::{HashMap, HashSet};
    use uuid::Uuid;

    fn make_summary(parent: Option<DocumentId>) -> CvSummary {
        CvSummary {
            id: Uuid::new_v4(),
            parent_id: parent,
            title: "Test".to_string(),
            version: 1,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            job_context: None,
            child_count: 0,
        }
    }

    #[test]
    fn flatten_empty_tree() {
        let rows = flatten_tree(&[], &HashMap::new(), &HashSet::new());
        assert!(rows.is_empty());
    }

    #[test]
    fn flatten_collapsed_root_hides_children() {
        let root = make_summary(None);
        let child = make_summary(Some(root.id));
        let mut map = HashMap::new();
        map.insert(root.id, vec![child]);
        let rows = flatten_tree(std::slice::from_ref(&root), &map, &HashSet::new());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].summary.id, root.id);
        assert_eq!(rows[0].depth, 0);
        assert!(!rows[0].expanded);
    }

    #[test]
    fn flatten_expanded_root_shows_children() {
        let root = make_summary(None);
        let child1 = make_summary(Some(root.id));
        let child2 = make_summary(Some(root.id));
        let mut map = HashMap::new();
        map.insert(root.id, vec![child1.clone(), child2.clone()]);
        let mut expanded = HashSet::new();
        expanded.insert(root.id);
        let rows = flatten_tree(std::slice::from_ref(&root), &map, &expanded);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].depth, 0);
        assert!(rows[0].expanded);
        assert_eq!(rows[1].depth, 1);
        assert_eq!(rows[1].summary.id, child1.id);
        assert_eq!(rows[2].depth, 1);
        assert_eq!(rows[2].summary.id, child2.id);
    }

    #[test]
    fn flatten_multiple_roots() {
        let root1 = make_summary(None);
        let root2 = make_summary(None);
        let rows = flatten_tree(
            &[root1.clone(), root2.clone()],
            &HashMap::new(),
            &HashSet::new(),
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].summary.id, root1.id);
        assert_eq!(rows[1].summary.id, root2.id);
    }

    #[test]
    fn tree_row_selection() {
        let root = make_summary(None);
        let row = TreeRow {
            summary: root.clone(),
            depth: 0,
            expanded: false,
        };
        assert_eq!(row.depth, 0);
        assert!(!row.expanded);
    }
}
