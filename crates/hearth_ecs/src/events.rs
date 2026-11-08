use std::collections::VecDeque;

/// Double-buffered event channel stored as a resource. Events survive two [`Events::update`] calls,
/// so every reader that runs once per frame sees each event exactly once.
pub struct Events<T> {
    events: VecDeque<(u64, T)>,
    next_id: u64,
    marker: u64,
}

impl<T> Default for Events<T> {
    fn default() -> Self {
        Events { events: VecDeque::new(), next_id: 0, marker: 0 }
    }
}

impl<T> Events<T> {
    pub fn send(&mut self, event: T) {
        self.events.push_back((self.next_id, event));
        self.next_id += 1;
    }

    /// Drop events from before the previous update.
    pub fn update(&mut self) {
        let marker = self.marker;
        while self.events.front().is_some_and(|(id, _)| *id < marker) {
            self.events.pop_front();
        }
        self.marker = self.next_id;
    }

    pub fn reader(&self) -> EventReader<T> {
        EventReader { next: self.next_id, _p: std::marker::PhantomData }
    }

    /// Iterate all buffered events, ignoring cursors.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.events.iter().map(|(_, e)| e)
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Cursor into an [`Events`] channel.
pub struct EventReader<T> {
    next: u64,
    _p: std::marker::PhantomData<fn() -> T>,
}

impl<T> Default for EventReader<T> {
    fn default() -> Self {
        EventReader { next: 0, _p: std::marker::PhantomData }
    }
}

impl<T> EventReader<T> {
    pub fn read<'a>(&mut self, events: &'a Events<T>) -> impl Iterator<Item = &'a T> + use<'a, T> {
        let start = self.next;
        self.next = events.next_id;
        events.events.iter().filter(move |(id, _)| *id >= start).map(|(_, e)| e)
    }
}
