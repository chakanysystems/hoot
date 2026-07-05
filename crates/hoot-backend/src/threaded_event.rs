use nostr::Event;
use std::cell::RefCell;
use std::collections::hash_map::HashMap;
use std::rc::Rc;

#[derive(Debug)]
pub struct ThreadedEvent {
    pub event: Event,
    pub children: Vec<Rc<RefCell<ThreadedEvent>>>,
}

pub fn build_thread(events: Vec<Event>) -> Vec<Rc<RefCell<ThreadedEvent>>> {
    let mut map: HashMap<String, Rc<RefCell<ThreadedEvent>>> = HashMap::new();

    // create nodes for each event
    for ev in events {
        let node = Rc::new(RefCell::new(ThreadedEvent {
            event: ev,
            children: vec![],
        }));
        // borrow once and clone the node for the map
        let event_id = node.borrow().event.id.to_string();
        map.insert(event_id, node.clone());
    }

    // attach children based on first "e" tag
    for node in map.values() {
        let node_ref = node.borrow();
        let parent_id = node_ref
            .event
            .tags
            .filter(nostr::TagKind::SingleLetter(
                nostr::SingleLetterTag::from_char('e').unwrap(),
            ))
            .find(|tag| tag.as_slice().len() == 2)
            .clone();
        if let Some(pid) = parent_id {
            let key = pid.as_slice()[1].to_string();
            if let Some(parent) = map.get(&key) {
                parent.borrow_mut().children.push(Rc::clone(node));
            }
        }
    }

    // filter roots: nodes with no valid parent
    map.values()
        .filter(|node| {
            let node_ref = node.borrow();
            let parent_id = node_ref
                .event
                .tags
                .filter(nostr::TagKind::SingleLetter(
                    nostr::SingleLetterTag::from_char('e').unwrap(),
                ))
                .find(|tag| tag.as_slice().len() == 2);
            match parent_id {
                Some(pid) if map.contains_key(&pid.as_slice()[1].to_string()) => false,
                _ => true,
            }
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, EventId, Keys, Kind, SecretKey, Tag};

    fn test_keys() -> Keys {
        Keys::new(SecretKey::from_slice(&[1; 32]).expect("test secret key should parse"))
    }

    fn text_event(keys: &Keys, content: &str, parent: Option<EventId>) -> Event {
        let builder = EventBuilder::new(Kind::TextNote, content);
        let builder = if let Some(parent_id) = parent {
            builder.tags(vec![Tag::event(parent_id)])
        } else {
            builder
        };

        builder
            .sign_with_keys(keys)
            .expect("test event should sign")
    }

    fn child_contents(node: &Rc<RefCell<ThreadedEvent>>) -> Vec<String> {
        let mut contents: Vec<String> = node
            .borrow()
            .children
            .iter()
            .map(|child| child.borrow().event.content.clone())
            .collect();
        contents.sort();
        contents
    }

    #[test]
    fn build_thread_attaches_child_to_existing_parent_and_returns_only_roots() {
        let keys = test_keys();
        let parent = text_event(&keys, "parent", None);
        let child = text_event(&keys, "child", Some(parent.id));

        let roots = build_thread(vec![child, parent]);

        assert_eq!(roots.len(), 1);
        let root = roots[0].borrow();
        assert_eq!(root.event.content, "parent");
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].borrow().event.content, "child");
    }

    #[test]
    fn build_thread_keeps_event_with_missing_parent_as_root() {
        let keys = test_keys();
        let missing_parent = text_event(&keys, "missing parent reference", None);
        let orphan = text_event(&keys, "orphan", Some(missing_parent.id));
        let unrelated_root = text_event(&keys, "unrelated root", None);

        let mut root_contents: Vec<String> = build_thread(vec![orphan, unrelated_root])
            .iter()
            .map(|root| root.borrow().event.content.clone())
            .collect();
        root_contents.sort();

        assert_eq!(root_contents, vec!["orphan", "unrelated root"]);
    }

    #[test]
    fn build_thread_assigns_multiple_children_to_their_parent_without_input_order_dependency() {
        let keys = test_keys();
        let parent = text_event(&keys, "parent", None);
        let first_child = text_event(&keys, "first child", Some(parent.id));
        let second_child = text_event(&keys, "second child", Some(parent.id));

        let roots = build_thread(vec![first_child, parent, second_child]);

        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].borrow().event.content, "parent");
        assert_eq!(
            child_contents(&roots[0]),
            vec!["first child", "second child"]
        );
    }
}
