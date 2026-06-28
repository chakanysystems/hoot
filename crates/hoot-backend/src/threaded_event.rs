use std::collections::hash_map::HashMap;
use std::cell::RefCell;
use std::rc::Rc;
use nostr::Event;

#[derive(Debug)]
pub struct ThreadedEvent {
    pub event: Event,
    pub children: Vec<Rc<RefCell<ThreadedEvent>>>,
}

pub fn build_thread(events: Vec<Event>) -> Vec<Rc<RefCell<ThreadedEvent>>> {
    let mut map: HashMap<String, Rc<RefCell<ThreadedEvent>>> = HashMap::new();

    // create nodes for each event
    for ev in events {
        let node = Rc::new(RefCell::new(ThreadedEvent { event: ev, children: vec![] }));
        // borrow once and clone the node for the map
        let event_id = node.borrow().event.id.to_string();
        map.insert(event_id, node.clone());
    }

    // attach children based on first "e" tag
    for node in map.values() {
        let node_ref = node.borrow();
        let parent_id = node_ref.event.tags
            .filter(nostr::TagKind::SingleLetter(nostr::SingleLetterTag::from_char('e').unwrap()))
            .find(|tag| tag.as_slice().len() == 2).clone();
        if let Some(pid) = parent_id {
            let key = pid.as_slice()[1].to_string();
            if let Some(parent) = map.get(&key) {
                parent.borrow_mut().children.push(Rc::clone(node));
            }
        }
    }

    // filter roots: nodes with no valid parent
    map.values().filter(|node| {
        let node_ref = node.borrow();
        let parent_id = node_ref.event.tags
            .filter(nostr::TagKind::SingleLetter(nostr::SingleLetterTag::from_char('e').unwrap()))
            .find(|tag| tag.as_slice().len() == 2);
        match parent_id {
            Some(pid) if map.contains_key(&pid.as_slice()[1].to_string()) => false,
            _ => true,
        }
    }).cloned().collect()
}
