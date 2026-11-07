use std::collections::{BTreeMap, HashMap};

use taffy::prelude::{
    AlignItems, AvailableSpace, Display, GridPlacement, Line, NodeId, Style, TaffyTree,
    TrackSizingFunction, auto, fr, length, percent,
};
use taffy::{GridTemplateComponent, compute_leaf_layout};

use crate::{Align, Cell, Id, Measure, Rect, Size, Track, Widgets};

const GAP: f32 = 8.0;
const PADDING: f32 = 12.0;
/// 1つのウィジェットが覚えておく測った大きさの数。ウィンドウの幅が変わり続けても表が際限なく伸びないよう、古いものから捨てる。
const CACHED_SIZES: usize = 8;

/// 1つのウィンドウのTaffyの木。木は`flush`をまたいで持ち続け、変わったウィジェットだけをdirtyとして印を付ける
/// (docs/implementation/architecture.md の「`eml_gui`の内部」)。
pub(crate) struct WindowLayout {
    tree: TaffyTree<Leaf>,
    root: NodeId,
    nodes: HashMap<Id, NodeId>,
    placed: BTreeMap<Id, Rect>,
    /// 表示したウィンドウの今の大きさ。表示する前は`None`となる。
    size: Option<Size>,
    dirty: bool,
}

/// 葉のウィジェットと、問い合わせごとに測った大きさ。
struct Leaf {
    id: Id,
    sizes: Vec<(Measure, Measure, Size)>,
}

impl WindowLayout {
    /// ウィンドウ自体を外側の格子`[Fr 1]`とする。中身の周りに12の余白を置き、ウィンドウ全体に広がる。
    pub(crate) fn new(window: Id) -> WindowLayout {
        let mut tree = TaffyTree::new();
        let style = Style {
            padding: length(PADDING),
            size: percent(1.0),
            ..grid_style(&[Track::Fr(1)], &[])
        };
        let root = tree.new_leaf(style).expect("a new node fits in the tree");
        WindowLayout {
            tree,
            root,
            nodes: HashMap::from([(window, root)]),
            placed: BTreeMap::new(),
            size: None,
            dirty: true,
        }
    }

    pub(crate) fn add_grid(
        &mut self,
        id: Id,
        parent: Id,
        cell: Cell,
        cols: &[Track],
        rows: &[Track],
    ) {
        let style = cell_style(cell, grid_style(cols, rows));
        let node = self
            .tree
            .new_leaf(style)
            .expect("a new node fits in the tree");
        self.attach(id, parent, node);
    }

    pub(crate) fn add_leaf(&mut self, id: Id, parent: Id, cell: Cell) {
        let leaf = Leaf {
            id,
            sizes: Vec::new(),
        };
        let style = cell_style(cell, Style::default());
        let node = self
            .tree
            .new_leaf_with_context(style, leaf)
            .expect("a new node fits in the tree");
        self.attach(id, parent, node);
    }

    fn attach(&mut self, id: Id, parent: Id, node: NodeId) {
        let parent = self.nodes[&parent];
        self.tree
            .add_child(parent, node)
            .expect("the parent is in the tree");
        self.nodes.insert(id, node);
        self.dirty = true;
    }

    /// 大きさが変わったかもしれないウィジェットの、測った大きさを忘れる。ウィンドウを渡すと、その中のすべてのウィジェットの分を忘れる。
    pub(crate) fn invalidate(&mut self, id: Id) {
        let nodes: Vec<NodeId> = if id.is_window() {
            self.nodes.values().copied().collect()
        } else {
            self.nodes.get(&id).copied().into_iter().collect()
        };
        for node in nodes {
            if let Some(leaf) = self.tree.get_node_context_mut(node) {
                leaf.sizes.clear();
            }
            self.tree.mark_dirty(node).expect("the node is in the tree");
        }
        self.dirty = true;
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// ウィンドウを最初に表示するときの大きさと最小の大きさ。大きさは中身のmax-contentの大きさである。
    /// 最小の幅はmin-contentの幅で、最小の高さはその幅での高さだが、大きさの高さは超えない。
    /// 折り返すラベルは幅が狭いほど高くなり、最小が大きさを超えてはならないからである。
    pub(crate) fn natural(&mut self, widgets: &mut dyn Widgets) -> (Size, Size) {
        let min = self.compute(
            widgets,
            AvailableSpace::MinContent,
            AvailableSpace::MinContent,
        );
        let size = self.compute(
            widgets,
            AvailableSpace::MaxContent,
            AvailableSpace::MaxContent,
        );
        let min = Size {
            w: min.w,
            h: min.h.min(size.h),
        };
        (size, min)
    }

    /// ウィンドウを`size`の大きさで配置し直し、丸めた位置か大きさが変わった子だけを置き直す。
    pub(crate) fn relayout(&mut self, widgets: &mut dyn Widgets, size: Size) {
        self.size = Some(size);
        self.compute(
            widgets,
            AvailableSpace::Definite(size.w),
            AvailableSpace::Definite(size.h),
        );
        let mut moved: Vec<(Id, Rect)> = Vec::new();
        for (&id, &node) in &self.nodes {
            if node == self.root {
                continue;
            }
            let layout = self.tree.layout(node).expect("the node is in the tree");
            let rect = Rect {
                x: layout.location.x,
                y: layout.location.y,
                w: layout.size.width,
                h: layout.size.height,
            };
            if self.placed.get(&id) != Some(&rect) {
                moved.push((id, rect));
            }
        }
        moved.sort_by_key(|&(id, _)| id);
        for (id, rect) in moved {
            widgets.place(id, rect);
            self.placed.insert(id, rect);
        }
        self.dirty = false;
    }

    pub(crate) fn size(&self) -> Option<Size> {
        self.size
    }

    fn compute(
        &mut self,
        widgets: &mut dyn Widgets,
        width: AvailableSpace,
        height: AvailableSpace,
    ) -> Size {
        let available = taffy::Size { width, height };
        self.tree
            .compute_layout_with_measure(self.root, available, |inputs, _, leaf, style| {
                compute_leaf_layout(
                    inputs,
                    style,
                    |_, _| 0.0,
                    |known, available| {
                        let Some(leaf) = leaf else {
                            return taffy::Size::ZERO;
                        };
                        // 幅と高さの両方が既に決まっていれば、測るものはない
                        if let taffy::Size {
                            width: Some(width),
                            height: Some(height),
                        } = known
                        {
                            return taffy::Size { width, height };
                        }
                        let size = leaf.measure(
                            widgets,
                            request(known.width, available.width),
                            request(known.height, available.height),
                        );
                        taffy::Size {
                            width: size.w,
                            height: size.h,
                        }
                    },
                )
            })
            .expect("the root is in the tree");
        let root = self
            .tree
            .layout(self.root)
            .expect("the root is in the tree");
        Size {
            w: root.size.width,
            h: root.size.height,
        }
    }
}

impl Leaf {
    fn measure(&mut self, widgets: &mut dyn Widgets, width: Measure, height: Measure) -> Size {
        if let Some(&(_, _, size)) = self
            .sizes
            .iter()
            .find(|&&(w, h, _)| w == width && h == height)
        {
            return size;
        }
        let size = widgets.measure(self.id, width, height);
        if self.sizes.len() == CACHED_SIZES {
            self.sizes.remove(0);
        }
        self.sizes.push((width, height, size));
        size
    }
}

/// Taffyの問い合わせを`Measure`に変える。`Measure`にはmin-contentの問い合わせを表す形がないため、「幅0まで」として問い合わせる。
/// 折り返すラベルは最も狭い幅を、折り返さないウィジェットは自身の最小の大きさを返す。
fn request(known: Option<f32>, available: AvailableSpace) -> Measure {
    match (known, available) {
        (Some(v), _) => Measure::Exact(v),
        (None, AvailableSpace::Definite(v)) => Measure::AtMost(v),
        (None, AvailableSpace::MinContent) => Measure::AtMost(0.0),
        (None, AvailableSpace::MaxContent) => Measure::Unbounded,
    }
}

fn grid_style(cols: &[Track], rows: &[Track]) -> Style {
    Style {
        display: Display::Grid,
        grid_template_columns: cols.iter().map(|&t| template(t)).collect(),
        grid_template_rows: rows.iter().map(|&t| template(t)).collect(),
        grid_auto_rows: vec![auto()],
        gap: length(GAP),
        ..Style::default()
    }
}

fn template(track: Track) -> GridTemplateComponent<String> {
    let track: TrackSizingFunction = match track {
        Track::Auto => auto(),
        Track::Fr(n) => fr(f32::from(n)),
        Track::Px(n) => length(n as f32),
    };
    GridTemplateComponent::Single(track)
}

fn cell_style(cell: Cell, style: Style) -> Style {
    Style {
        grid_column: Line {
            start: GridPlacement::Auto,
            end: GridPlacement::Span(cell.col_span),
        },
        grid_row: Line {
            start: GridPlacement::Auto,
            end: GridPlacement::Span(cell.row_span),
        },
        justify_self: Some(align(cell.halign)),
        align_self: Some(align(cell.valign)),
        ..style
    }
}

fn align(a: Align) -> AlignItems {
    match a {
        Align::Start => AlignItems::START,
        Align::Center => AlignItems::CENTER,
        Align::End => AlignItems::END,
        Align::Stretch => AlignItems::STRETCH,
    }
}
