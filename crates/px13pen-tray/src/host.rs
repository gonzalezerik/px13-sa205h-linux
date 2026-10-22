// SPDX-License-Identifier: AGPL-3.0-or-later
//! The tray on the session bus: one StatusNotifierItem at
//! `/StatusNotifierItem` and its `com.canonical.dbusmenu` at
//! `/MenuBar`, registered with the desktop's StatusNotifierWatcher —
//! and registered again every time a watcher appears, so a panel that
//! starts (or restarts) after the tray still finds it. The item says
//! what `PenTray` says; this file only translates.
//!
//! Change signals are the item's own `NewTitle`/`NewIcon`/
//! `NewToolTip`/`NewStatus` and, for the menu, both `LayoutUpdated` and
//! `ItemsPropertiesUpdated` carrying every item: hosts differ in which
//! one makes them refresh a menu that is open, and an open menu must
//! follow the pen (docked, lifted, battery) without being reopened.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use futures_util::StreamExt;
use serde::Serialize;
use zbus::fdo::{self, DBusProxy};
use zbus::object_server::SignalEmitter;
use zbus::wire::{OwnedObjectPath, OwnedVariant, StructureBuilder, Type, Variant};
use zbus::{Connection, block_on};

use crate::PenTray;
use crate::menu::{Action, MenuItem};

const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";
const WATCHER: &str = "org.kde.StatusNotifierWatcher";

type Shared = Arc<Mutex<PenTray>>;

fn locked(tray: &Shared) -> MutexGuard<'_, PenTray> {
    tray.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The running tray. `update` changes what it shows and tells the
/// host; clones share the one item.
#[derive(Clone)]
pub struct Handle {
    tray: Shared,
    connection: Connection,
    revision: Arc<AtomicU32>,
}

impl Handle {
    pub fn update(&self, change: impl FnOnce(&mut PenTray)) {
        change(&mut locked(&self.tray));
        let revision = self.revision.fetch_add(1, Ordering::Relaxed) + 1;
        let status = locked(&self.tray).status().as_str();
        // Off the caller's thread, like a menu write: the caller is
        // usually inside the daemon watch, which must not wait on the
        // session bus.
        let connection = self.connection.clone();
        std::thread::spawn(move || {
            if let Err(error) = block_on(announce(&connection, revision, status)) {
                eprintln!("px13pen-tray: announcing a change: {error}");
            }
        });
    }
}

async fn announce(connection: &Connection, revision: u32, status: &str) -> zbus::Result<()> {
    let server = connection.object_server();
    let item = server.interface::<_, Item>(ITEM_PATH).await?;
    item.new_title().await?;
    item.new_icon().await?;
    item.new_tool_tip().await?;
    item.new_status(status).await?;
    let menu = server.interface::<_, Menu>(MENU_PATH).await?;
    let updated: Vec<(i32, Properties)> = menu
        .get()
        .await
        .nodes()
        .into_iter()
        .enumerate()
        .map(|(id, node)| (id as i32, node.properties))
        .collect();
    menu.items_properties_updated(updated, Vec::new()).await?;
    menu.layout_updated(revision, 0).await
}

/// Put `tray` on the session bus. A desktop without a status-notifier
/// host is not an error: the tray waits for a watcher to appear
/// instead of failing, so a missing panel never becomes a restart
/// loop.
pub fn spawn(tray: PenTray) -> Result<Handle, String> {
    let tray = Arc::new(Mutex::new(tray));
    let revision = Arc::new(AtomicU32::new(1));
    let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
    let connection = block_on(
        zbus::connection::Builder::session()
            .name(name.as_str())
            .serve_at(
                ITEM_PATH,
                Item {
                    tray: Arc::clone(&tray),
                },
            )
            .serve_at(
                MENU_PATH,
                Menu {
                    tray: Arc::clone(&tray),
                    revision: Arc::clone(&revision),
                    served: Mutex::new(Vec::new()),
                },
            )
            .build(),
    )
    .map_err(|error| format!("cannot serve the tray on the session bus: {error}"))?;
    let watching = connection.clone();
    std::thread::spawn(move || block_on(keep_registered(&watching, &name)));
    Ok(Handle {
        tray,
        connection,
        revision,
    })
}

/// Register with the watcher now if there is one, and again whenever
/// one takes the watcher name.
async fn keep_registered(connection: &Connection, name: &str) {
    let appeared = match DBusProxy::new(connection).await {
        Ok(bus) => bus.receive_name_owner_changed().await,
        Err(error) => Err(error),
    };
    let mut appeared = match appeared {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("px13pen-tray: cannot follow the status notifier watcher: {error}");
            return;
        }
    };
    if let Err(error) = register(connection, name).await {
        eprintln!("px13pen-tray: no status notifier host yet ({error}); waiting for one");
    }
    while let Some(signal) = appeared.next().await {
        let Ok(args) = signal.args() else { continue };
        if args.name().as_str() == WATCHER
            && args.new_owner().is_some()
            && let Err(error) = register(connection, name).await
        {
            eprintln!("px13pen-tray: registering with the new watcher: {error}");
        }
    }
}

async fn register(connection: &Connection, name: &str) -> zbus::Result<()> {
    connection
        .call_method(
            Some(WATCHER),
            "/StatusNotifierWatcher",
            Some(WATCHER),
            "RegisterStatusNotifierItem",
            &(name,),
        )
        .await
        .map(|_| ())
}

/// An image in the SNI wire form; the tray sends icon names only.
type Pixmap = (i32, i32, Vec<u8>);

struct Item {
    tray: Shared,
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    #[zbus(property)]
    fn category(&self) -> String {
        "Hardware".to_string()
    }

    #[zbus(property)]
    fn id(&self) -> String {
        locked(&self.tray).id()
    }

    #[zbus(property)]
    fn title(&self) -> String {
        locked(&self.tray).title()
    }

    #[zbus(property)]
    fn status(&self) -> String {
        locked(&self.tray).status().as_str().to_string()
    }

    #[zbus(property)]
    fn window_id(&self) -> i32 {
        0
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn icon_name(&self) -> String {
        locked(&self.tray).icon_name()
    }

    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<Pixmap> {
        Vec::new()
    }

    #[zbus(property)]
    fn overlay_icon_name(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn overlay_icon_pixmap(&self) -> Vec<Pixmap> {
        Vec::new()
    }

    #[zbus(property)]
    fn attention_icon_name(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn attention_icon_pixmap(&self) -> Vec<Pixmap> {
        Vec::new()
    }

    #[zbus(property)]
    fn attention_movie_name(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn tool_tip(&self) -> (String, Vec<Pixmap>, String, String) {
        let tip = locked(&self.tray).tool_tip();
        (tip.icon_name, Vec::new(), tip.title, tip.description)
    }

    /// Left-click is how anyone reaches a tray's controls; opening
    /// the menu beats activating a window no one asked for.
    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(MENU_PATH).expect("a valid object path")
    }

    // With ItemIsMenu set, hosts open the menu themselves; clicks
    // and scrolls that still arrive have nothing to do.
    fn activate(&self, _x: i32, _y: i32) {}

    fn secondary_activate(&self, _x: i32, _y: i32) {}

    fn context_menu(&self, _x: i32, _y: i32) {}

    fn scroll(&self, _delta: i32, _orientation: &str) {}

    #[zbus(signal)]
    async fn new_title(signal_emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_icon(signal_emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_tool_tip(signal_emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_status(signal_emitter: &SignalEmitter<'_>, status: &str) -> zbus::Result<()>;
}

type Properties = HashMap<String, OwnedVariant>;

/// One dbusmenu entry. Its index in the flattened list is its id; 0
/// is the root.
struct Node {
    properties: Properties,
    action: Option<Action>,
    children: Vec<usize>,
}

fn value(value: impl Into<Variant<'static>>) -> OwnedVariant {
    value
        .into()
        .try_to_owned()
        .expect("menu values carry no file descriptors")
}

/// Number the menu depth-first, the way the host addresses it. A
/// radio group is no dbusmenu item of its own: its options become
/// radio items in place.
pub fn flatten(items: &[MenuItem]) -> Vec<NodeView> {
    let mut nodes = vec![Node {
        properties: Properties::from([("children-display".to_string(), value("submenu"))]),
        action: None,
        children: Vec::new(),
    }];
    add(&mut nodes, 0, items);
    nodes
        .into_iter()
        .map(|node| NodeView {
            properties: node.properties,
            action: node.action,
            children: node.children,
        })
        .collect()
}

/// A numbered menu entry, as `flatten` hands it out.
pub struct NodeView {
    pub properties: Properties,
    pub action: Option<Action>,
    pub children: Vec<usize>,
}

fn add(nodes: &mut Vec<Node>, parent: usize, items: &[MenuItem]) {
    for item in items {
        match item {
            MenuItem::Standard(item) => push(
                nodes,
                parent,
                [
                    ("label", value(item.label.clone())),
                    ("enabled", value(item.enabled)),
                ],
                item.action.clone(),
            ),
            MenuItem::Checkmark(item) => push(
                nodes,
                parent,
                [
                    ("label", value(item.label.clone())),
                    ("toggle-type", value("checkmark")),
                    ("toggle-state", value(i32::from(item.checked))),
                ],
                Some(item.action.clone()),
            ),
            MenuItem::SubMenu(item) => {
                push(
                    nodes,
                    parent,
                    [
                        ("label", value(item.label.clone())),
                        ("children-display", value("submenu")),
                    ],
                    None,
                );
                let id = nodes.len() - 1;
                add(nodes, id, &item.submenu);
            }
            MenuItem::RadioGroup(group) => {
                for (index, option) in group.options.iter().enumerate() {
                    push(
                        nodes,
                        parent,
                        [
                            ("label", value(option.label.clone())),
                            ("toggle-type", value("radio")),
                            ("toggle-state", value(i32::from(index == group.selected))),
                        ],
                        Some(option.action.clone()),
                    );
                }
            }
            MenuItem::Separator => push(nodes, parent, [("type", value("separator"))], None),
        }
    }
}

fn push<const N: usize>(
    nodes: &mut Vec<Node>,
    parent: usize,
    properties: [(&str, OwnedVariant); N],
    action: Option<Action>,
) {
    nodes.push(Node {
        properties: properties
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
        action,
        children: Vec::new(),
    });
    let id = nodes.len() - 1;
    nodes[parent].children.push(id);
}

/// `(ia{sv}av)`: an item, its properties and its children, each child
/// the same structure inside a variant.
#[derive(Serialize, Type)]
pub struct Layout {
    id: i32,
    properties: Properties,
    children: Vec<Variant<'static>>,
}

fn layout(nodes: &[NodeView], id: usize, depth: i32) -> Layout {
    let children = if depth == 0 {
        Vec::new()
    } else {
        nodes[id]
            .children
            .iter()
            .map(|&child| {
                let child = layout(nodes, child, depth - 1);
                Variant::from(
                    StructureBuilder::new()
                        .add_field(child.id)
                        .add_field(child.properties)
                        .add_field(child.children)
                        .build()
                        .expect("a non-empty structure"),
                )
            })
            .collect()
    };
    Layout {
        id: id as i32,
        properties: nodes[id].properties.clone(),
        children,
    }
}

fn filtered(properties: &Properties, names: &[String]) -> Properties {
    properties
        .iter()
        .filter(|(name, _)| names.is_empty() || names.contains(name))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

struct Menu {
    tray: Shared,
    revision: Arc<AtomicU32>,
    /// The actions of the layout the host last fetched: a click names
    /// an id from that layout, even if the pens changed since.
    served: Mutex<Vec<Option<Action>>>,
}

impl Menu {
    fn nodes(&self) -> Vec<NodeView> {
        flatten(&locked(&self.tray).menu())
    }

    fn click(&self, id: i32) -> bool {
        let action = {
            let served = self
                .served
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if served.is_empty() {
                let nodes = self.nodes();
                usize::try_from(id)
                    .ok()
                    .and_then(|id| nodes.get(id))
                    .map(|node| node.action.clone())
            } else {
                usize::try_from(id)
                    .ok()
                    .and_then(|id| served.get(id).cloned())
            }
        };
        match action {
            Some(Some(action)) => {
                locked(&self.tray).run(&action);
                true
            }
            Some(None) => true,
            None => false,
        }
    }
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl Menu {
    #[zbus(property)]
    fn version(&self) -> u32 {
        3
    }

    #[zbus(property)]
    fn text_direction(&self) -> String {
        "ltr".to_string()
    }

    #[zbus(property)]
    fn status(&self) -> String {
        "normal".to_string()
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<String> {
        Vec::new()
    }

    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        _property_names: Vec<String>,
    ) -> fdo::Result<(u32, Layout)> {
        let nodes = self.nodes();
        let parent = usize::try_from(parent_id)
            .ok()
            .filter(|&id| id < nodes.len())
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("no menu item {parent_id}")))?;
        *self
            .served
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            nodes.iter().map(|node| node.action.clone()).collect();
        Ok((
            self.revision.load(Ordering::Relaxed),
            layout(&nodes, parent, recursion_depth),
        ))
    }

    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<String>,
    ) -> Vec<(i32, Properties)> {
        let nodes = self.nodes();
        let wanted: Vec<usize> = if ids.is_empty() {
            (0..nodes.len()).collect()
        } else {
            ids.iter()
                .filter_map(|&id| usize::try_from(id).ok())
                .filter(|&id| id < nodes.len())
                .collect()
        };
        wanted
            .into_iter()
            .map(|id| (id as i32, filtered(&nodes[id].properties, &property_names)))
            .collect()
    }

    fn get_property(&self, id: i32, name: String) -> fdo::Result<OwnedVariant> {
        let nodes = self.nodes();
        usize::try_from(id)
            .ok()
            .and_then(|id| nodes.get(id))
            .and_then(|node| node.properties.get(&name).cloned())
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("no property {name} on item {id}")))
    }

    fn event(
        &self,
        id: i32,
        event_id: String,
        _data: OwnedVariant,
        _timestamp: u32,
    ) -> fdo::Result<()> {
        if event_id != "clicked" {
            return Ok(());
        }
        if self.click(id) {
            Ok(())
        } else {
            Err(fdo::Error::InvalidArgs(format!("no menu item {id}")))
        }
    }

    fn event_group(&self, events: Vec<(i32, String, OwnedVariant, u32)>) -> Vec<i32> {
        events
            .into_iter()
            .filter(|(id, event_id, _, _)| event_id == "clicked" && !self.click(*id))
            .map(|(id, _, _, _)| id)
            .collect()
    }

    /// The menu is rebuilt on every GetLayout, so it is never stale
    /// when it opens.
    fn about_to_show(&self, _id: i32) -> bool {
        false
    }

    fn about_to_show_group(&self, _ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
        (Vec::new(), Vec::new())
    }

    #[zbus(signal)]
    async fn items_properties_updated(
        signal_emitter: &SignalEmitter<'_>,
        updated_props: Vec<(i32, Properties)>,
        removed_props: Vec<(i32, Vec<String>)>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn layout_updated(
        signal_emitter: &SignalEmitter<'_>,
        revision: u32,
        parent: i32,
    ) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::{CheckmarkItem, RadioGroup, RadioItem, StandardItem, SubMenu};

    fn label(node: &NodeView) -> Option<String> {
        node.properties
            .get("label")
            .and_then(|value| String::try_from(value.clone()).ok())
    }

    #[test]
    fn ids_are_depth_first_and_radio_options_are_items() {
        let items = vec![
            MenuItem::Standard(StandardItem {
                label: "fact".to_string(),
                enabled: false,
                action: None,
            }),
            MenuItem::Checkmark(CheckmarkItem {
                label: "switch".to_string(),
                checked: true,
                action: Action::Pair,
            }),
            MenuItem::SubMenu(SubMenu {
                label: "sub".to_string(),
                submenu: vec![MenuItem::RadioGroup(RadioGroup {
                    selected: 1,
                    options: ["a", "b"]
                        .iter()
                        .map(|label| RadioItem {
                            label: (*label).to_string(),
                            action: Action::Settings,
                        })
                        .collect(),
                })],
            }),
            MenuItem::Separator,
        ];
        let nodes = flatten(&items);
        let labels: Vec<Option<String>> = nodes.iter().map(label).collect();
        assert_eq!(
            labels,
            [
                None,
                Some("fact".to_string()),
                Some("switch".to_string()),
                Some("sub".to_string()),
                Some("a".to_string()),
                Some("b".to_string()),
                None,
            ]
        );
        assert_eq!(nodes[0].children, [1, 2, 3, 6]);
        assert_eq!(nodes[3].children, [4, 5]);
        let state =
            |id: usize| i32::try_from(nodes[id].properties["toggle-state"].clone()).unwrap();
        assert_eq!((state(2), state(4), state(5)), (1, 0, 1));
        assert_eq!(nodes[1].action, None);
        assert_eq!(nodes[5].action, Some(Action::Settings));
    }

    #[test]
    fn the_layout_serializes_as_dbusmenu_expects() {
        let nodes = flatten(&[MenuItem::Separator]);
        let tree = layout(&nodes, 0, -1);
        assert_eq!(Layout::SIGNATURE.to_string(), "(ia{sv}av)");
        assert_eq!(tree.children.len(), 1);
    }
}
