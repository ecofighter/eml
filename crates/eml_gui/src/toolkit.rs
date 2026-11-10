use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::layout::WindowLayout;
use crate::{Cell, Event, EventQueue, GuiError, Id, Init, Kind, Prop, RawEvent, Track, Widgets};

/// ウィジェットの表と、作りかけの入れ物の積み重ね。ネイティブのcallbackはこれに触れず、`EventQueue`に積むだけである。
pub struct Toolkit {
    widgets: Rc<RefCell<dyn Widgets>>,
    queue: EventQueue,
    table: BTreeMap<Id, Entry>,
    windows: BTreeMap<Id, Window>,
    stack: Vec<Id>,
    /// `next_cell`が次に作るウィジェットに決めた升目。
    next: Option<Cell>,
    last_window: u32,
    layouts: u64,
}

struct Entry {
    kind: Kind,
    items: usize,
}

struct Window {
    next_seq: u32,
    layout: WindowLayout,
}

impl Toolkit {
    /// ウィンドウの番号は`last_window`の次から数える。同じbackendの上で続く`Toolkit`は、前の`Toolkit`の
    /// `last_window()`を渡して、idを使い回さない。
    pub fn new(widgets: Rc<RefCell<dyn Widgets>>, queue: EventQueue, last_window: u32) -> Toolkit {
        queue.attach();
        Toolkit {
            widgets,
            queue,
            table: BTreeMap::new(),
            windows: BTreeMap::new(),
            stack: Vec::new(),
            next: None,
            last_window,
            layouts: 0,
        }
    }

    /// 最後に作ったウィンドウの番号。まだなければ`new`に渡した番号である。
    pub fn last_window(&self) -> u32 {
        self.last_window
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
                layout: WindowLayout::new(id),
            },
        );
        self.table.insert(id, Entry::new(Kind::Window));
        self.stack.push(id);
        self.next = None;
        Ok(id)
    }

    pub fn begin_grid(&mut self, cols: &[Track], rows: &[Track]) -> Result<Id, GuiError> {
        let id = self.make(Init::Grid, cols, rows)?;
        self.stack.push(id);
        Ok(id)
    }

    pub fn end(&mut self) -> Result<(), GuiError> {
        self.next = None;
        self.stack.pop().map(drop).ok_or(GuiError::NothingOpen)
    }

    /// 開いている入れ物の中で次に作るウィジェットの升目を決める。決めずに作ったウィジェットは`Cell::default()`を取る。
    /// 升目は次に作られるウィジェットを待つので、本体がウィジェットを作らない`Gui.cell`は、その升目を次のウィジェットに渡す。
    /// `end`、`begin_window`、`close_window`は升目を捨てる。
    pub fn next_cell(&mut self, cell: Cell) -> Result<(), GuiError> {
        self.container()?;
        if cell.col_span == 0 || cell.row_span == 0 {
            return Err(GuiError::OutOfRange);
        }
        self.next = Some(cell);
        Ok(())
    }

    pub fn create(&mut self, init: Init) -> Result<Id, GuiError> {
        self.make(init, &[], &[])
    }

    fn make(&mut self, init: Init, cols: &[Track], rows: &[Track]) -> Result<Id, GuiError> {
        let parent = self.container()?;
        // ウィジェットを作るのに失敗しても升目は使い切り、後のウィジェットには渡さない
        let cell = self.next.take().unwrap_or_default();
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
        let layout = &mut window.layout;
        if kind == Kind::Grid {
            layout.add_grid(id, parent, cell, cols, rows);
        } else {
            layout.add_leaf(id, parent, cell);
        }
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
        self.invalidate(w);
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
        self.widgets.borrow_mut().set(w, &prop)?;
        // 大きさを変えるのは文字だけである。ウィンドウの題名は中身の配置に影響しない
        if matches!(prop, Prop::Text(_)) && !w.is_window() {
            self.invalidate(w);
        }
        Ok(())
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
        self.next = None;
        Ok(())
    }

    /// 変わったウィンドウだけを配置し直し、新しいウィンドウは中身のmax-contentの大きさで表示する
    /// (docs/implementation/architecture.md の「`eml_gui`の内部」)。
    pub fn flush(&mut self) {
        let mut widgets = self.widgets.borrow_mut();
        for (&w, window) in &mut self.windows {
            let layout = &mut window.layout;
            match layout.size() {
                None => {
                    let (size, min) = layout.natural(&mut *widgets);
                    layout.relayout(&mut *widgets, size);
                    widgets.show_window(w, size, min);
                }
                Some(size) if layout.is_dirty() => layout.relayout(&mut *widgets, size),
                Some(_) => continue,
            }
            self.layouts += 1;
        }
    }

    /// ループの`step`が取り出した`Resized`か`Invalidate`を反映し、そのウィンドウをすぐに配置し直す。
    /// まだ表示していないウィンドウは次の`flush`で配置するので、ここでは測った大きさを忘れるだけにする。
    pub fn settle(&mut self, event: &RawEvent) {
        let (w, resized) = match *event {
            RawEvent::Resized(w, size) => (w, Some(size)),
            RawEvent::Invalidate(w) if self.table.contains_key(&w) => {
                self.invalidate(w);
                (w.window(), None)
            }
            RawEvent::Invalidate(_) | RawEvent::User(_) | RawEvent::QuitRequested => return,
        };
        let Some(window) = self.windows.get_mut(&w) else {
            return;
        };
        let layout = &mut window.layout;
        let Some(size) = layout.size() else {
            return;
        };
        layout.relayout(&mut *self.widgets.borrow_mut(), resized.unwrap_or(size));
        self.layouts += 1;
    }

    /// 配置を計算した回数。`flush`では変わったウィンドウごとに1回、`settle`では1回と数える。
    pub fn layouts(&self) -> u64 {
        self.layouts
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

    fn invalidate(&mut self, w: Id) {
        if let Some(window) = self.windows.get_mut(&w.window()) {
            window.layout.invalidate(w);
        }
    }
}

impl Entry {
    fn new(kind: Kind) -> Entry {
        Entry { kind, items: 0 }
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
    use crate::{Align, Backend, Native, Script, Size};

    fn toolkit() -> (Toolkit, EventQueue) {
        take_transcript();
        let native = Backend::Stub(Script::default()).create().unwrap();
        let queue = native.queue.clone();
        (Toolkit::new(native.widgets, native.queue, 0), queue)
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
        let _second = Toolkit::new(widgets, queue, 0);
    }

    #[test]
    fn a_queue_takes_a_new_toolkit_after_the_old_one_is_dropped() {
        let (tk, queue) = toolkit();
        let widgets = Rc::clone(&tk.widgets);
        drop(tk);
        let mut tk = Toolkit::new(widgets, queue.clone(), 0);
        let w = tk.begin_window("A").unwrap();
        queue.push(RawEvent::User(Event::CloseRequest(w)));
        assert_eq!(tk.poll(), Some(Event::CloseRequest(w)));
    }

    #[test]
    fn a_new_toolkit_numbers_windows_after_the_last_one() {
        let (mut tk, queue) = toolkit();
        tk.begin_window("A").unwrap();
        tk.begin_window("B").unwrap();
        assert_eq!(tk.last_window(), 2);
        let widgets = Rc::clone(&tk.widgets);
        let last = tk.last_window();
        drop(tk);
        let mut tk = Toolkit::new(widgets, queue, last);
        assert_eq!(tk.last_window(), 2);
        assert_eq!(tk.begin_window("C").unwrap(), Id::new(3, 0));
        assert_eq!(
            tk.set(Id::new(1, 0), Prop::Text("x".into())),
            Err(GuiError::UnknownId)
        );
        take_transcript();
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
            placements(),
            [
                "place #1:1 (12, 12) 23x24",
                "show_window #1:0 47x48 min 47x48"
            ]
        );
    }

    /// ウィンドウの中の`[Fr 1, Fr 1]`の格子に、2列にまたがるラベルと2つのボタンを置く。
    fn counter(tk: &mut Toolkit) -> (Id, Id) {
        let w = tk.begin_window("Counter").unwrap();
        tk.begin_grid(&[Track::Fr(1), Track::Fr(1)], &[]).unwrap();
        tk.next_cell(Cell {
            col_span: 2,
            ..Cell::default()
        })
        .unwrap();
        let label = tk.create(Init::Label("0".into())).unwrap();
        tk.create(Init::Button("-".into())).unwrap();
        tk.create(Init::Button("+".into())).unwrap();
        tk.end().unwrap();
        tk.end().unwrap();
        take_transcript();
        (w, label)
    }

    fn placements() -> Vec<String> {
        take_transcript()
            .into_iter()
            .filter(|l| !l.starts_with("measure "))
            .collect()
    }

    #[test]
    fn counter_takes_its_natural_size() {
        let (mut tk, _) = toolkit();
        counter(&mut tk);
        tk.flush();
        assert_eq!(
            placements(),
            [
                "place #1:1 (12, 12) 54x56",
                "place #1:2 (0, 0) 54x24",
                "place #1:3 (0, 32) 23x24",
                "place #1:4 (31, 32) 23x24",
                "show_window #1:0 78x80 min 78x80",
            ]
        );
    }

    #[test]
    fn counter_resized_stretches_every_track() {
        let (mut tk, _) = toolkit();
        let (w, _) = counter(&mut tk);
        tk.flush();
        take_transcript();
        tk.settle(&RawEvent::Resized(w, Size { w: 300.0, h: 200.0 }));
        assert_eq!(
            placements(),
            [
                "place #1:1 (12, 12) 276x176",
                "place #1:2 (0, 0) 276x84",
                "place #1:3 (0, 92) 134x84",
                "place #1:4 (142, 92) 134x84",
            ]
        );
    }

    #[test]
    fn form_aligns_a_spanning_button_to_the_end() {
        let (mut tk, _) = toolkit();
        tk.begin_window("Form").unwrap();
        tk.begin_grid(&[Track::Auto, Track::Fr(1)], &[]).unwrap();
        tk.create(Init::Label("Name".into())).unwrap();
        tk.create(Init::TextField(String::new())).unwrap();
        let span = Cell {
            col_span: 2,
            ..Cell::default()
        };
        tk.next_cell(span).unwrap();
        tk.create(Init::CheckBox("I agree".into(), false)).unwrap();
        tk.next_cell(Cell {
            halign: Align::End,
            ..span
        })
        .unwrap();
        tk.create(Init::Button("Submit".into())).unwrap();
        tk.end().unwrap();
        tk.end().unwrap();
        take_transcript();
        tk.flush();
        assert_eq!(
            placements(),
            [
                "place #1:1 (12, 12) 68x88",
                "place #1:2 (0, 0) 44x24",
                "place #1:3 (52, 0) 16x24",
                "place #1:4 (0, 32) 68x24",
                "place #1:5 (10, 64) 58x24",
                "show_window #1:0 92x112 min 92x112",
            ]
        );
    }

    #[test]
    fn a_changed_label_is_measured_again_once_at_the_next_flush() {
        let (mut tk, requests) = counting_toolkit();
        let (_, label) = counter(&mut tk);
        tk.flush();
        requests.borrow_mut().clear();
        tk.set(label, Prop::Text("10".into())).unwrap();
        tk.flush();
        let asked = requests.take();
        assert!(!asked.is_empty());
        assert!(asked.iter().all(|&(id, _, _)| id == label));
        for (i, r) in asked.iter().enumerate() {
            assert!(!asked[..i].contains(r), "{r:?} was measured twice");
        }
        tk.flush();
        assert_eq!(requests.take(), []);
    }

    #[test]
    fn layout_runs_once_per_dirty_flush_and_per_resize() {
        let (mut tk, _) = toolkit();
        let (w, label) = counter(&mut tk);
        tk.begin_window("Other").unwrap();
        tk.end().unwrap();
        assert_eq!(tk.layouts(), 0);
        tk.flush();
        assert_eq!(tk.layouts(), 2);
        tk.flush();
        assert_eq!(tk.layouts(), 2);
        // ウィンドウの題名は配置を変えない
        tk.set(w, Prop::Text("Title".into())).unwrap();
        tk.flush();
        assert_eq!(tk.layouts(), 2);
        tk.set(label, Prop::Text("10".into())).unwrap();
        tk.set(label, Prop::Text("11".into())).unwrap();
        assert_eq!(tk.layouts(), 2);
        tk.flush();
        assert_eq!(tk.layouts(), 3);
        let size = Size { w: 300.0, h: 200.0 };
        tk.settle(&RawEvent::Resized(w, size));
        assert_eq!(tk.layouts(), 4);
        tk.settle(&RawEvent::Invalidate(label));
        assert_eq!(tk.layouts(), 5);
        tk.flush();
        assert_eq!(tk.layouts(), 5);
        tk.settle(&RawEvent::Invalidate(Id::new(9, 1)));
        tk.settle(&RawEvent::QuitRequested);
        assert_eq!(tk.layouts(), 5);
    }

    #[test]
    fn a_resize_places_only_children_whose_rect_changed() {
        let (mut tk, _) = toolkit();
        let (w, _) = counter(&mut tk);
        tk.flush();
        take_transcript();
        tk.settle(&RawEvent::Resized(w, Size { w: 78.0, h: 80.0 }));
        assert_eq!(placements(), Vec::<String>::new());
        tk.settle(&RawEvent::Resized(w, Size { w: 78.0, h: 120.0 }));
        assert_eq!(
            placements(),
            [
                "place #1:1 (12, 12) 54x96",
                "place #1:2 (0, 0) 54x44",
                "place #1:3 (0, 52) 23x44",
                "place #1:4 (31, 52) 23x44",
            ]
        );
    }

    #[test]
    fn invalidate_and_add_item_measure_again() {
        let (mut tk, requests) = counting_toolkit();
        let w = tk.begin_window("A").unwrap();
        let c = tk.create(Init::Choice).unwrap();
        // ラベルが最も幅が広いため、項目を足しても列の幅は変わらず、ラベルに新しい問い合わせは届かない
        let l = tk.create(Init::Label("a wide label".into())).unwrap();
        tk.end().unwrap();
        tk.flush();
        requests.borrow_mut().clear();
        tk.add_item(c, "red").unwrap();
        tk.flush();
        let asked = requests.take();
        assert!(!asked.is_empty() && asked.iter().all(|&(id, _, _)| id == c));
        tk.settle(&RawEvent::Invalidate(w));
        let asked = requests.take();
        assert!(asked.iter().any(|&(id, _, _)| id == c));
        assert!(asked.iter().any(|&(id, _, _)| id == l));
    }

    type Requests = Rc<RefCell<Vec<(Id, crate::Measure, crate::Measure)>>>;

    /// `measure`の問い合わせを、stubに渡す前に記録する。
    struct Counting {
        inner: Rc<RefCell<dyn Widgets>>,
        requests: Requests,
        wrap: bool,
        fail_create: Rc<std::cell::Cell<bool>>,
    }

    impl Widgets for Counting {
        fn create_window(&mut self, id: Id, title: &str) -> Result<(), GuiError> {
            self.inner.borrow_mut().create_window(id, title)
        }
        fn create(&mut self, id: Id, kind: Kind, parent: Id, init: &Init) -> Result<(), GuiError> {
            if self.fail_create.take() {
                return Err(GuiError::Backend("refused".into()));
            }
            self.inner.borrow_mut().create(id, kind, parent, init)
        }
        fn add_item(&mut self, w: Id, item: &str) -> Result<(), GuiError> {
            self.inner.borrow_mut().add_item(w, item)
        }
        fn set(&mut self, w: Id, prop: &Prop) -> Result<(), GuiError> {
            self.inner.borrow_mut().set(w, prop)
        }
        fn measure(&mut self, w: Id, width: crate::Measure, height: crate::Measure) -> Size {
            self.requests.borrow_mut().push((w, width, height));
            let natural = self.inner.borrow_mut().measure(w, width, height);
            if !self.wrap {
                return natural;
            }
            // 折り返すラベルのように、幅が足りなければ1行24の高さで行を増やす
            let room = match width {
                crate::Measure::Exact(v) | crate::Measure::AtMost(v) => v,
                crate::Measure::Unbounded => f32::INFINITY,
            };
            let width = room.clamp(16.0, natural.w);
            Size {
                w: width,
                h: 24.0 * (natural.w / width).ceil(),
            }
        }
        fn place(&mut self, w: Id, rect: crate::Rect) {
            self.inner.borrow_mut().place(w, rect)
        }
        fn show_window(&mut self, w: Id, size: Size, min: Size) {
            self.inner.borrow_mut().show_window(w, size, min)
        }
        fn destroy(&mut self, w: Id) {
            self.inner.borrow_mut().destroy(w)
        }
        fn set_presence(&mut self, active: bool) {
            self.inner.borrow_mut().set_presence(active)
        }
    }

    fn counting_toolkit() -> (Toolkit, Requests) {
        let (tk, requests, _) = probe(false);
        (tk, requests)
    }

    /// `wrap`なら測る大きさを折り返すラベルのようにする。返す旗を立てると、次の`create`が1回だけ失敗する。
    fn probe(wrap: bool) -> (Toolkit, Requests, Rc<std::cell::Cell<bool>>) {
        take_transcript();
        let native = Backend::Stub(Script::default()).create().unwrap();
        let requests = Requests::default();
        let fail_create = Rc::default();
        let counting = Counting {
            inner: native.widgets,
            requests: Rc::clone(&requests),
            wrap,
            fail_create: Rc::clone(&fail_create),
        };
        (
            Toolkit::new(Rc::new(RefCell::new(counting)), native.queue, 0),
            requests,
            fail_create,
        )
    }

    #[test]
    fn the_minimum_height_never_exceeds_the_natural_height() {
        let (mut tk, _, _) = probe(true);
        tk.begin_window("A").unwrap();
        tk.create(Init::Label("a label that wraps".into())).unwrap();
        tk.end().unwrap();
        tk.flush();
        let shown: Vec<String> = placements()
            .into_iter()
            .filter(|l| l.starts_with("show_window "))
            .collect();
        // min-contentの幅16では9行 (高さ216) になるが、最小の高さは中身の自然な高さで止まる
        assert_eq!(shown, ["show_window #1:0 166x48 min 40x48"]);
    }

    #[test]
    fn a_pending_cell_does_not_outlive_its_window() {
        let (mut tk, _) = toolkit();
        tk.begin_window("B").unwrap();
        tk.begin_grid(&[Track::Fr(1), Track::Fr(1)], &[]).unwrap();
        let a = tk.begin_window("A").unwrap();
        tk.next_cell(Cell {
            col_span: 2,
            ..Cell::default()
        })
        .unwrap();
        tk.close_window(a).unwrap();
        tk.create(Init::Label("x".into())).unwrap();
        tk.create(Init::Label("y".into())).unwrap();
        take_transcript();
        tk.flush();
        assert_eq!(
            placements(),
            [
                "place #1:1 (12, 12) 54x24",
                "place #1:2 (0, 0) 23x24",
                "place #1:3 (31, 0) 23x24",
                "show_window #1:0 78x48 min 78x48",
            ]
        );
    }

    #[test]
    fn a_failed_create_uses_up_the_pending_cell() {
        let (mut tk, _, fail_create) = probe(false);
        tk.begin_window("A").unwrap();
        tk.begin_grid(&[Track::Fr(1), Track::Fr(1)], &[]).unwrap();
        tk.next_cell(Cell {
            col_span: 2,
            ..Cell::default()
        })
        .unwrap();
        fail_create.set(true);
        assert_eq!(
            tk.create(Init::Label("z".into())),
            Err(GuiError::Backend("refused".into()))
        );
        tk.create(Init::Label("x".into())).unwrap();
        tk.create(Init::Label("y".into())).unwrap();
        take_transcript();
        tk.flush();
        assert_eq!(
            placements(),
            [
                "place #1:1 (12, 12) 54x24",
                "place #1:3 (0, 0) 23x24",
                "place #1:4 (31, 0) 23x24",
                "show_window #1:0 78x48 min 78x48",
            ]
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
