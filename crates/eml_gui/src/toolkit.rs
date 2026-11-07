use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::{
    Cell, Event, EventQueue, GuiError, Id, Init, Kind, Prop, RawEvent, Rect, Size, Track, Widgets,
};

/// ウィジェットの表と、作りかけの入れ物の積み重ね。ネイティブのcallbackはこれに触れず、`EventQueue`に積むだけである。
pub struct Toolkit {
    widgets: Rc<RefCell<dyn Widgets>>,
    queue: EventQueue,
    table: BTreeMap<Id, Entry>,
    windows: BTreeMap<Id, Window>,
    stack: Vec<Id>,
    last_window: u32,
}

struct Entry {
    kind: Kind,
    items: usize,
    placed: bool,
}

struct Window {
    next_seq: u32,
    shown: bool,
}

impl Toolkit {
    pub fn new(widgets: Rc<RefCell<dyn Widgets>>, queue: EventQueue) -> Toolkit {
        queue.attach();
        Toolkit {
            widgets,
            queue,
            table: BTreeMap::new(),
            windows: BTreeMap::new(),
            stack: Vec::new(),
            last_window: 0,
        }
    }

    pub fn begin_window(&mut self, title: &str) -> Result<Id, GuiError> {
        let no = self
            .last_window
            .checked_add(1)
            .ok_or(GuiError::OutOfRange)?;
        let id = Id::new(no, 0);
        self.widgets.borrow_mut().create_window(id, title)?;
        self.last_window = no;
        self.windows.insert(
            id,
            Window {
                next_seq: 1,
                shown: false,
            },
        );
        self.table.insert(id, Entry::new(Kind::Window));
        self.stack.push(id);
        Ok(id)
    }

    pub fn begin_grid(&mut self, _cols: &[Track], _rows: &[Track]) -> Result<Id, GuiError> {
        let id = self.create(Init::Grid)?;
        self.stack.push(id);
        Ok(id)
    }

    pub fn end(&mut self) -> Result<(), GuiError> {
        self.stack.pop().map(drop).ok_or(GuiError::NothingOpen)
    }

    pub fn next_cell(&mut self, cell: Cell) -> Result<(), GuiError> {
        self.container()?;
        if cell.col_span == 0 || cell.row_span == 0 {
            return Err(GuiError::OutOfRange);
        }
        Ok(())
    }

    pub fn create(&mut self, init: Init) -> Result<Id, GuiError> {
        let parent = self.container()?;
        let window = self
            .windows
            .get_mut(&parent.window())
            .expect("an open container belongs to a live window");
        let seq = window.next_seq;
        window.next_seq = seq.checked_add(1).ok_or(GuiError::OutOfRange)?;
        let id = Id::new(parent.window_no(), seq);
        let kind = init.kind();
        // `set`と同じく、ProgressBarの値は0..=100の範囲でどのbackendにも届く
        let init = match init {
            Init::ProgressBar(v) => Init::ProgressBar(v.clamp(0, 100)),
            init => init,
        };
        self.widgets.borrow_mut().create(id, kind, parent, &init)?;
        self.table.insert(id, Entry::new(kind));
        Ok(id)
    }

    pub fn add_item(&mut self, w: Id, item: &str) -> Result<(), GuiError> {
        let entry = self.entry(w)?;
        if entry.kind != Kind::Choice {
            return Err(GuiError::WrongKind);
        }
        self.widgets.borrow_mut().add_item(w, item)?;
        self.entry(w)?.items += 1;
        Ok(())
    }

    /// 値はすぐにネイティブのウィジェットに渡す。ProgressBarの値は0..=100に収め、項目の範囲の外のChoiceの番号は誤りとする。
    pub fn set(&mut self, w: Id, prop: Prop) -> Result<(), GuiError> {
        let entry = self.entry(w)?;
        let applies = match &prop {
            Prop::Text(_) => matches!(
                entry.kind,
                Kind::Window | Kind::Label | Kind::Button | Kind::TextField | Kind::CheckBox
            ),
            Prop::Value(_) => matches!(entry.kind, Kind::ProgressBar | Kind::Choice),
            Prop::Checked(_) => entry.kind == Kind::CheckBox,
            Prop::Enabled(_) => !matches!(entry.kind, Kind::Window | Kind::Grid | Kind::Label),
        };
        if !applies {
            return Err(GuiError::WrongKind);
        }
        let prop = match prop {
            Prop::Value(v) if entry.kind == Kind::ProgressBar => Prop::Value(v.clamp(0, 100)),
            Prop::Value(v) if usize::try_from(v).is_ok_and(|i| i < entry.items) => Prop::Value(v),
            Prop::Value(_) => return Err(GuiError::OutOfRange),
            prop => prop,
        };
        self.widgets.borrow_mut().set(w, &prop)
    }

    pub fn close_window(&mut self, w: Id) -> Result<(), GuiError> {
        if self.entry(w)?.kind != Kind::Window {
            return Err(GuiError::WrongKind);
        }
        self.widgets.borrow_mut().destroy(w);
        self.windows.remove(&w);
        let no = w.window_no();
        self.table.retain(|id, _| id.window_no() != no);
        self.stack.retain(|id| id.window_no() != no);
        Ok(())
    }

    /// 新しいウィンドウを表示する。子は原点に大きさ0で置く。
    pub fn flush(&mut self) {
        let mut widgets = self.widgets.borrow_mut();
        for (&w, window) in &mut self.windows {
            let no = w.window_no();
            let children = self
                .table
                .range_mut(Id::new(no, 1)..=Id::new(no, u32::MAX))
                .filter(|(_, e)| !e.placed);
            for (&id, entry) in children {
                widgets.place(id, Rect::default());
                entry.placed = true;
            }
            if !window.shown {
                widgets.show_window(w, Size::default(), Size::default());
                window.shown = true;
            }
        }
    }

    /// 次のユーザーのイベントを返す。もう存在しないウィジェットのイベントは捨てる。
    pub fn poll(&mut self) -> Option<Event> {
        loop {
            let event = self.queue.pop_user()?;
            if self.table.contains_key(&event.id()) {
                return Some(event);
            }
        }
    }

    /// `close_window`で閉じていないウィンドウの数。閉じる要求だけではウィンドウは閉じない。
    pub fn open_windows(&self) -> usize {
        self.windows.len()
    }

    pub fn close_all(&mut self) {
        let windows: Vec<Id> = self.windows.keys().copied().collect();
        for w in windows {
            self.close_window(w)
                .expect("every window in the table is live");
        }
    }

    /// OSの終了の操作を、開いているウィンドウごとの閉じる要求に変える。
    pub fn request_close_all(&mut self) {
        for &w in self.windows.keys() {
            self.queue.push(RawEvent::User(Event::CloseRequest(w)));
        }
    }

    pub fn has_events(&self) -> bool {
        self.queue.has_user()
    }

    fn container(&self) -> Result<Id, GuiError> {
        self.stack.last().copied().ok_or(GuiError::NoContainer)
    }

    fn entry(&mut self, w: Id) -> Result<&mut Entry, GuiError> {
        self.table.get_mut(&w).ok_or(GuiError::UnknownId)
    }
}

impl Entry {
    fn new(kind: Kind) -> Entry {
        Entry {
            kind,
            items: 0,
            // ウィンドウは置かず、表示する
            placed: kind == Kind::Window,
        }
    }
}

impl Drop for Toolkit {
    fn drop(&mut self) {
        self.queue.detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stub::take_transcript;
    use crate::{Backend, Native, Script};

    fn toolkit() -> (Toolkit, EventQueue) {
        take_transcript();
        let native = Backend::Stub(Script::default()).create().unwrap();
        let queue = native.queue.clone();
        (Toolkit::new(native.widgets, native.queue), queue)
    }

    fn user(e: Event) -> RawEvent {
        RawEvent::User(e)
    }

    /// `Send`な型では両方の実装が当てはまるので、型の推論が決まらず、コンパイルが止まる。
    trait AmbiguousIfSend<A> {
        fn some_item() {}
    }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}

    #[test]
    fn handles_are_not_send() {
        <Toolkit as AmbiguousIfSend<_>>::some_item();
        <EventQueue as AmbiguousIfSend<_>>::some_item();
        <Native as AmbiguousIfSend<_>>::some_item();
        <dyn Widgets as AmbiguousIfSend<_>>::some_item();
        <dyn crate::EventLoop as AmbiguousIfSend<_>>::some_item();
    }

    #[test]
    fn ids_encode_the_window_and_are_never_reused() {
        let (mut tk, _) = toolkit();
        let w1 = tk.begin_window("A").unwrap();
        let g = tk.begin_grid(&[Track::Fr(1)], &[]).unwrap();
        let l = tk.create(Init::Label("x".into())).unwrap();
        tk.end().unwrap();
        tk.end().unwrap();
        assert_eq!((w1, g, l), (Id::new(1, 0), Id::new(1, 1), Id::new(1, 2)));
        assert_eq!(w1.0, 1 << 32);
        tk.close_window(w1).unwrap();
        let w2 = tk.begin_window("B").unwrap();
        let b = tk.create(Init::Button("b".into())).unwrap();
        assert_eq!((w2, b), (Id::new(2, 0), Id::new(2, 1)));
        assert_eq!(
            take_transcript(),
            [
                "create_window #1:0 \"A\"",
                "create Grid #1:1 in #1:0",
                "create Label #1:2 in #1:1 \"x\"",
                "destroy #1:0",
                "create_window #2:0 \"B\"",
                "create Button #2:1 in #2:0 \"b\"",
            ]
        );
    }

    #[test]
    fn stale_ids_are_unknown() {
        let (mut tk, _) = toolkit();
        let w = tk.begin_window("A").unwrap();
        let l = tk.create(Init::Label("x".into())).unwrap();
        let c = tk.create(Init::Choice).unwrap();
        tk.close_window(w).unwrap();
        assert_eq!(tk.set(l, Prop::Text("y".into())), Err(GuiError::UnknownId));
        assert_eq!(tk.add_item(c, "a"), Err(GuiError::UnknownId));
        assert_eq!(tk.close_window(w), Err(GuiError::UnknownId));
        assert_eq!(
            tk.set(Id::new(9, 9), Prop::Value(1)),
            Err(GuiError::UnknownId)
        );
        assert_eq!(
            tk.create(Init::Label("z".into())),
            Err(GuiError::NoContainer)
        );
        assert_eq!(tk.open_windows(), 0);
    }

    #[test]
    fn containers_must_be_open() {
        let (mut tk, _) = toolkit();
        assert_eq!(
            tk.create(Init::Label("x".into())),
            Err(GuiError::NoContainer)
        );
        assert_eq!(tk.begin_grid(&[], &[]), Err(GuiError::NoContainer));
        assert_eq!(tk.next_cell(Cell::default()), Err(GuiError::NoContainer));
        assert_eq!(tk.end(), Err(GuiError::NothingOpen));
        tk.begin_window("A").unwrap();
        tk.next_cell(Cell::default()).unwrap();
        let zero = Cell {
            col_span: 0,
            ..Cell::default()
        };
        assert_eq!(tk.next_cell(zero), Err(GuiError::OutOfRange));
        tk.end().unwrap();
        assert_eq!(tk.end(), Err(GuiError::NothingOpen));
    }

    #[test]
    fn props_check_the_kind_and_the_range() {
        let (mut tk, _) = toolkit();
        let w = tk.begin_window("A").unwrap();
        let g = tk.begin_grid(&[], &[]).unwrap();
        let l = tk.create(Init::Label("x".into())).unwrap();
        let p = tk.create(Init::ProgressBar(0)).unwrap();
        let c = tk.create(Init::Choice).unwrap();
        take_transcript();
        assert_eq!(tk.set(w, Prop::Enabled(false)), Err(GuiError::WrongKind));
        assert_eq!(tk.set(g, Prop::Text("t".into())), Err(GuiError::WrongKind));
        assert_eq!(tk.set(l, Prop::Enabled(false)), Err(GuiError::WrongKind));
        assert_eq!(tk.set(l, Prop::Checked(true)), Err(GuiError::WrongKind));
        assert_eq!(tk.add_item(l, "a"), Err(GuiError::WrongKind));
        assert_eq!(tk.close_window(l), Err(GuiError::WrongKind));
        assert_eq!(tk.set(c, Prop::Value(0)), Err(GuiError::OutOfRange));
        tk.add_item(c, "a").unwrap();
        tk.add_item(c, "b").unwrap();
        assert_eq!(tk.set(c, Prop::Value(2)), Err(GuiError::OutOfRange));
        assert_eq!(tk.set(c, Prop::Value(-1)), Err(GuiError::OutOfRange));
        tk.set(c, Prop::Value(1)).unwrap();
        tk.set(p, Prop::Value(130)).unwrap();
        tk.set(p, Prop::Value(-5)).unwrap();
        tk.set(w, Prop::Text("B".into())).unwrap();
        assert_eq!(
            take_transcript(),
            [
                "add_item #1:4 \"a\"",
                "add_item #1:4 \"b\"",
                "set #1:4 Value(1)",
                "set #1:3 Value(100)",
                "set #1:3 Value(0)",
                "set #1:0 Text(\"B\")",
            ]
        );
    }

    #[test]
    fn create_clamps_a_progress_bar() {
        let (mut tk, _) = toolkit();
        tk.begin_window("A").unwrap();
        tk.create(Init::ProgressBar(150)).unwrap();
        tk.create(Init::ProgressBar(-3)).unwrap();
        tk.create(Init::ProgressBar(42)).unwrap();
        assert_eq!(
            take_transcript()[1..],
            [
                "create ProgressBar #1:1 in #1:0 value=100",
                "create ProgressBar #1:2 in #1:0 value=0",
                "create ProgressBar #1:3 in #1:0 value=42",
            ]
        );
    }

    #[test]
    #[should_panic(expected = "an EventQueue serves one Toolkit at a time")]
    fn a_second_toolkit_on_one_queue_panics() {
        let (tk, queue) = toolkit();
        let widgets = Rc::clone(&tk.widgets);
        let _second = Toolkit::new(widgets, queue);
    }

    #[test]
    fn a_queue_takes_a_new_toolkit_after_the_old_one_is_dropped() {
        let (tk, queue) = toolkit();
        let widgets = Rc::clone(&tk.widgets);
        drop(tk);
        let mut tk = Toolkit::new(widgets, queue.clone());
        let w = tk.begin_window("A").unwrap();
        queue.push(RawEvent::User(Event::CloseRequest(w)));
        assert_eq!(tk.poll(), Some(Event::CloseRequest(w)));
    }

    #[test]
    fn flush_shows_new_windows_once_and_places_new_children() {
        let (mut tk, _) = toolkit();
        tk.begin_window("A").unwrap();
        tk.create(Init::Label("x".into())).unwrap();
        tk.end().unwrap();
        take_transcript();
        tk.flush();
        tk.flush();
        assert_eq!(
            take_transcript(),
            ["place #1:1 (0, 0) 0x0", "show_window #1:0 0x0 min 0x0"]
        );
    }

    #[test]
    fn events_of_destroyed_widgets_are_dropped() {
        let (mut tk, queue) = toolkit();
        let w1 = tk.begin_window("A").unwrap();
        let b1 = tk.create(Init::Button("b".into())).unwrap();
        tk.end().unwrap();
        let w2 = tk.begin_window("B").unwrap();
        let b2 = tk.create(Init::Button("b".into())).unwrap();
        tk.end().unwrap();
        queue.push(user(Event::Click(b1)));
        queue.push(user(Event::CloseRequest(w1)));
        queue.push(user(Event::Click(b2)));
        queue.push(user(Event::Click(Id::new(7, 1))));
        tk.close_window(w1).unwrap();
        assert!(tk.has_events());
        assert_eq!(tk.poll(), Some(Event::Click(b2)));
        assert_eq!(tk.poll(), None);
        assert!(!tk.has_events());
        assert_eq!(tk.open_windows(), 1);
        tk.close_window(w2).unwrap();
        assert_eq!(tk.open_windows(), 0);
    }

    #[test]
    fn request_close_all_fans_out_to_every_open_window() {
        let (mut tk, queue) = toolkit();
        let w1 = tk.begin_window("A").unwrap();
        tk.end().unwrap();
        let w2 = tk.begin_window("B").unwrap();
        tk.end().unwrap();
        let w3 = tk.begin_window("C").unwrap();
        tk.end().unwrap();
        tk.close_window(w2).unwrap();
        tk.request_close_all();
        assert!(queue.take_control().is_empty());
        assert_eq!(tk.poll(), Some(Event::CloseRequest(w1)));
        assert_eq!(tk.poll(), Some(Event::CloseRequest(w3)));
        assert_eq!(tk.poll(), None);
        // 閉じる要求ではウィンドウは閉じない
        assert_eq!(tk.open_windows(), 2);
    }

    #[test]
    fn close_all_destroys_every_window() {
        let (mut tk, _) = toolkit();
        tk.begin_window("A").unwrap();
        tk.begin_window("B").unwrap();
        tk.create(Init::Label("x".into())).unwrap();
        take_transcript();
        tk.close_all();
        assert_eq!(take_transcript(), ["destroy #1:0", "destroy #2:0"]);
        assert_eq!(tk.open_windows(), 0);
        assert_eq!(
            tk.create(Init::Label("y".into())),
            Err(GuiError::NoContainer)
        );
    }

    #[test]
    fn dropping_the_toolkit_drops_queued_user_events() {
        let (mut tk, queue) = toolkit();
        let w = tk.begin_window("A").unwrap();
        queue.push(user(Event::CloseRequest(w)));
        queue.push(RawEvent::QuitRequested);
        drop(tk);
        queue.push(user(Event::CloseRequest(w)));
        assert_eq!(queue.pop_user(), None);
        assert_eq!(queue.take_control(), [RawEvent::QuitRequested]);
    }
}
