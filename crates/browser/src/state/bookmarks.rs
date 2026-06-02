use std::cell::RefCell;
use std::rc::Rc;

/// Store en mémoire (Sprint 2 : persistance dans le vault chiffré).
pub type Bookmarks = Rc<RefCell<Vec<Bookmark>>>;

pub struct Bookmark {
    pub url:   String,
    pub title: String,
}

pub fn new() -> Bookmarks {
    Rc::new(RefCell::new(Vec::new()))
}
