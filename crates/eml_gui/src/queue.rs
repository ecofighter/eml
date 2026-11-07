use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use crate::{Event, RawEvent};

/// ネイティブのcallbackが積み、`Toolkit`とループの`step`が取り出すイベントの列。複製も同じ列を指す。
/// 1つの列に同時に付く`Toolkit`は1つだけである。古い`Toolkit`を落とすと列から外れ、列のユーザーのイベントを捨てるので、
/// 2つ目を付けると、古い方の後始末が新しい`Toolkit`の受け取るイベントを消してしまう。
#[derive(Clone, Default)]
pub struct EventQueue(Rc<Inner>);

#[derive(Default)]
struct Inner {
    events: RefCell<VecDeque<RawEvent>>,
    attached: Cell<bool>,
}

impl EventQueue {
    pub fn new() -> EventQueue {
        EventQueue::default()
    }

    /// callbackはこれだけを呼ぶ。`Toolkit`がない間のユーザーのイベントは、受け取る相手がいないので捨てる。
    pub fn push(&self, event: RawEvent) {
        if matches!(event, RawEvent::User(_)) && !self.0.attached.get() {
            return;
        }
        self.0.events.borrow_mut().push_back(event);
    }

    /// ユーザーのイベント以外のイベント (`QuitRequested`、`Resized`、`Invalidate`) を順に取り出す。ユーザーのイベントは列に残る。
    pub fn take_control(&self) -> Vec<RawEvent> {
        let mut events = self.0.events.borrow_mut();
        let (user, control) = events
            .drain(..)
            .partition::<VecDeque<_>, _>(|e| matches!(e, RawEvent::User(_)));
        *events = user;
        control.into()
    }

    pub fn pop_user(&self) -> Option<Event> {
        let mut events = self.0.events.borrow_mut();
        let at = events.iter().position(|e| matches!(e, RawEvent::User(_)))?;
        match events.remove(at) {
            Some(RawEvent::User(e)) => Some(e),
            _ => unreachable!("the position found a user event"),
        }
    }

    pub(crate) fn has_user(&self) -> bool {
        self.0
            .events
            .borrow()
            .iter()
            .any(|e| matches!(e, RawEvent::User(_)))
    }

    pub(crate) fn attach(&self) {
        assert!(
            !self.0.attached.get(),
            "an EventQueue serves one Toolkit at a time; drop the old Toolkit first"
        );
        self.0.attached.set(true);
    }

    pub(crate) fn detach(&self) {
        self.0.attached.set(false);
        self.0
            .events
            .borrow_mut()
            .retain(|e| !matches!(e, RawEvent::User(_)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Id, Size};

    fn click(seq: u32) -> RawEvent {
        RawEvent::User(Event::Click(Id::new(1, seq)))
    }

    #[test]
    fn take_control_leaves_user_events_queued() {
        let q = EventQueue::new();
        q.attach();
        q.push(click(1));
        q.push(RawEvent::QuitRequested);
        q.push(click(2));
        q.push(RawEvent::Resized(Id::new(1, 0), Size { w: 1.0, h: 2.0 }));
        q.push(RawEvent::Invalidate(Id::new(1, 0)));
        assert_eq!(
            q.take_control(),
            vec![
                RawEvent::QuitRequested,
                RawEvent::Resized(Id::new(1, 0), Size { w: 1.0, h: 2.0 }),
                RawEvent::Invalidate(Id::new(1, 0)),
            ]
        );
        assert!(q.take_control().is_empty());
        assert_eq!(q.pop_user(), Some(Event::Click(Id::new(1, 1))));
        assert_eq!(q.pop_user(), Some(Event::Click(Id::new(1, 2))));
        assert_eq!(q.pop_user(), None);
    }

    #[test]
    fn pop_user_skips_control_events() {
        let q = EventQueue::new();
        q.attach();
        q.push(RawEvent::QuitRequested);
        q.push(click(1));
        assert_eq!(q.pop_user(), Some(Event::Click(Id::new(1, 1))));
        assert_eq!(q.take_control(), vec![RawEvent::QuitRequested]);
    }

    #[test]
    fn user_events_without_a_toolkit_are_dropped() {
        let q = EventQueue::new();
        q.push(click(1));
        q.push(RawEvent::QuitRequested);
        assert!(!q.has_user());
        q.attach();
        q.push(click(2));
        assert!(q.has_user());
        q.detach();
        assert!(!q.has_user());
        assert_eq!(q.take_control(), vec![RawEvent::QuitRequested]);
    }

    #[test]
    fn clones_share_the_queue() {
        let q = EventQueue::new();
        let r = q.clone();
        q.attach();
        r.push(click(1));
        assert_eq!(q.pop_user(), Some(Event::Click(Id::new(1, 1))));
    }
}
