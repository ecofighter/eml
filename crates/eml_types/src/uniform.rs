//! 一様な位置のグラフと制約付きの多相再帰 (E2012)
//! (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「一様な位置と制約付きの多相再帰」)。
//! 型検査の後に、書き出した本体の表から計算する。translate は結果を読むだけである。

use std::collections::{HashMap, HashSet};

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_hir::{
    ClassId, Function, FunctionId, FunctionKind, InstanceId, InstanceOrigin, ItemMap, MethodId,
    MethodImpl, Program, Signature, TypeVarId, ValueItem,
};

use crate::check::instance_head;
use crate::resolve::{Resolution, resolve};
use crate::store::{TypeId, TypeKind, TypeStore};
use crate::{BodyTypes, codes};

/// 多相再帰で大きくなる型変数の位置 (spec の「一様な位置と制約付きの多相再帰」)。
#[derive(Debug, Default)]
pub struct Uniform {
    functions: HashSet<(FunctionId, usize)>,
    instances: HashSet<(InstanceNode, usize)>,
}

impl Uniform {
    pub fn function(&self, function: FunctionId, position: usize) -> bool {
        self.functions.contains(&(function, position))
    }

    pub fn instance(&self, node: InstanceNode, position: usize) -> bool {
        self.instances.contains(&(node, position))
    }
}

/// 一様な位置のグラフの instance の節点。生成する関数 (導出とタプル) の鍵の位置もこれで決まる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstanceNode {
    Declared(InstanceId),
    /// 要素の数ごとのタプルの instance (Task 9)。
    Tuple(usize),
}

/// グラフの節点。位置は、関数とメソッドなら `Generics` の型変数の番号、instance なら頭の型変数の番号である。
/// メソッドの節点はメソッド自身の型変数 (番号 1 から) だけを持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Node {
    Function(FunctionId, usize),
    Instance(InstanceNode, usize),
    Method(MethodId, usize),
}

/// 一様な位置と、与えられた制約を持つ一様な位置の E2012。
pub(crate) fn uniform(
    program: &Program,
    types: &TypeStore,
    bodies: &ItemMap<Function, BodyTypes>,
) -> (Uniform, Vec<Diagnostic>) {
    let mut graph = Graph::new(program);
    graph.instance_edges();
    graph.method_edges();
    for (f, function) in program.functions() {
        if !function.kind.has_equations() {
            continue;
        }
        let names = type_vars(function.signature.as_ref());
        if names.is_empty() {
            continue;
        }
        let Some(body) = bodies.get(f) else {
            continue;
        };
        graph.body_edges(types, f, &names, body);
    }
    let component = strongly_connected(&graph.edges);
    let mut growing = HashSet::new();
    for (from, out) in graph.edges.iter().enumerate() {
        for &(to, grow) in out {
            if grow && component[from] == component[to] {
                growing.insert(component[from]);
            }
        }
    }
    let mut uniform = Uniform::default();
    let mut diagnostics = Vec::new();
    let mut reported = HashSet::new();
    for (index, &node) in graph.nodes.iter().enumerate() {
        if !growing.contains(&component[index]) {
            continue;
        }
        match node {
            Node::Function(function, position) => {
                uniform.functions.insert((function, position));
            }
            Node::Instance(instance, position) => {
                uniform.instances.insert((instance, position));
            }
            Node::Method(..) => {}
        }
        if let Some(error) = graph.constrained(node, &component, index)
            && reported.insert((error.file, error.range))
        {
            diagnostics.push(error.diagnostic(program));
        }
    }
    (uniform, diagnostics)
}

struct Graph<'a> {
    program: &'a Program,
    nodes: Vec<Node>,
    node_of: HashMap<Node, usize>,
    /// 節点ごとの (行き先, 大きくなる辺か)。
    edges: Vec<Vec<(usize, bool)>>,
    /// 辺の 5. の、省いたメソッドの既定のメソッドへの辺の出どころ。(instance の節点, instance, 既定のメソッドの関数)。
    /// 既定のメソッドの位置の E2012 が、どの instance の頭を指すかを決める。
    default_edges: Vec<(usize, InstanceId, FunctionId)>,
}

impl<'a> Graph<'a> {
    fn new(program: &'a Program) -> Graph<'a> {
        let mut nodes = Vec::new();
        for (id, function) in program.functions() {
            if function.kind.has_equations() {
                let count = type_vars(function.signature.as_ref()).len();
                nodes.extend((0..count).map(|position| Node::Function(id, position)));
            }
        }
        for (id, instance) in program.instances() {
            let count = instance.generics.type_vars.len();
            nodes.extend(
                (0..count).map(|position| Node::Instance(InstanceNode::Declared(id), position)),
            );
        }
        for (id, method) in program.methods() {
            let count = method.signature.generics.type_vars.len();
            nodes.extend((1..count).map(|position| Node::Method(id, position)));
        }
        let node_of = nodes
            .iter()
            .enumerate()
            .map(|(index, &node)| (node, index))
            .collect();
        Graph {
            program,
            edges: vec![Vec::new(); nodes.len()],
            nodes,
            node_of,
            default_edges: Vec::new(),
        }
    }

    fn edge(&mut self, from: Node, to: Node, grow: bool) {
        if let (Some(&from), Some(&to)) = (self.node_of.get(&from), self.node_of.get(&to)) {
            self.edges[from].push((to, grow));
        }
    }

    /// 辺の 5.: instance の節点から、instance のメソッドの関数へ大きくならない辺を、省いたメソッドの既定のメソッドの
    /// クラスの型変数へ大きくなる辺を引く。導出した instance は Task 9 で足す。
    fn instance_edges(&mut self) {
        let program = self.program;
        for (id, instance) in program.instances() {
            if instance.origin != InstanceOrigin::Written {
                continue;
            }
            let node = InstanceNode::Declared(id);
            for &method in &program[instance.class].methods {
                for position in 0..instance.generics.type_vars.len() {
                    let from = Node::Instance(node, position);
                    match instance.method(method) {
                        Some(MethodImpl::Function(function)) => {
                            self.edge(from, Node::Function(function, position), false);
                        }
                        Some(MethodImpl::Extern(_)) => {}
                        None => {
                            let Some(default) = program[method].default else {
                                continue;
                            };
                            self.edge(from, Node::Function(default, 0), true);
                            if let Some(&from) = self.node_of.get(&from) {
                                self.default_edges.push((from, id, default));
                            }
                        }
                    }
                }
            }
        }
    }

    /// 辺の 6.: メソッドの節点から、メソッドを定義するすべての instance のメソッドの関数と既定のメソッドの、同じ
    /// メソッド自身の型変数へ大きくならない辺を引く。
    fn method_edges(&mut self) {
        let program = self.program;
        let mut instances: HashMap<ClassId, Vec<InstanceId>> = HashMap::new();
        for (id, instance) in program.instances() {
            instances.entry(instance.class).or_default().push(id);
        }
        for (id, method) in program.methods() {
            let count = method.signature.generics.type_vars.len();
            let defining: Vec<(FunctionId, usize)> = instances
                .get(&method.class)
                .into_iter()
                .flatten()
                .filter_map(|&instance| match program[instance].method(id) {
                    Some(MethodImpl::Function(function)) => {
                        Some((function, program[instance].generics.type_vars.len()))
                    }
                    Some(MethodImpl::Extern(_)) | None => None,
                })
                .collect();
            for own in 1..count {
                let from = Node::Method(id, own);
                for &(function, head) in &defining {
                    self.edge(from, Node::Function(function, head + own - 1), false);
                }
                if let Some(default) = method.default {
                    self.edge(from, Node::Function(default, own), false);
                }
            }
        }
    }

    /// 本体の関数 `f` の参照ごとの辺 (辺の 1. から 4.)。
    fn body_edges(&mut self, types: &TypeStore, f: FunctionId, names: &[String], body: &BodyTypes) {
        let program = self.program;
        let mut flow = Flow {
            f,
            names,
            types,
            occurrences: HashMap::new(),
        };
        let mut wanted = Wanted::default();
        for (_, instantiation) in body.instantiations.iter() {
            let args = &instantiation.args;
            match instantiation.decl {
                ValueItem::Function(g) => {
                    for (j, &arg) in args.iter().enumerate() {
                        self.flow(&mut flow, arg, Node::Function(g, j));
                    }
                    wanted.constraints(program[g].signature.as_ref(), args);
                }
                ValueItem::Method(m) => {
                    self.method_reference(&mut flow, m, args, &mut wanted);
                    wanted.constraints(Some(&program[m].signature), args);
                }
                ValueItem::Operation(_) | ValueItem::Constructor(_) => {}
            }
        }
        self.resolve_wanted(&mut flow, wanted);
    }

    /// 辺の 2. と 3.: メソッドの参照 `m @[T, U1, …]`。解決の木の根の instance の文脈から下は 4. に回す。
    fn method_reference(
        &mut self,
        flow: &mut Flow,
        m: MethodId,
        args: &[TypeId],
        wanted: &mut Wanted,
    ) {
        let program = self.program;
        let method = &program[m];
        let Some((&root, own)) = args.split_first() else {
            return;
        };
        match resolve(program, flow.types, method.class, root) {
            Resolution::Instance {
                instance,
                args: head,
            } => {
                let def = &program[instance];
                match def.method(m) {
                    Some(MethodImpl::Function(function)) => {
                        for (j, &arg) in head.iter().enumerate() {
                            self.flow(flow, arg, Node::Function(function, j));
                        }
                        for (l, &arg) in own.iter().enumerate() {
                            self.flow(flow, arg, Node::Function(function, head.len() + l));
                        }
                    }
                    Some(MethodImpl::Extern(_)) => {}
                    None => {
                        let Some(default) = method.default else {
                            return;
                        };
                        self.flow(flow, root, Node::Function(default, 0));
                        for (l, &arg) in own.iter().enumerate() {
                            self.flow(flow, arg, Node::Function(default, l + 1));
                        }
                        // 既定のメソッドはクラスの型変数への制約 `C a` を与えられた制約として持ち、同じ instance の
                        // ほかのメソッドと上位クラスの instance をその証拠として呼ぶ。spec の 3. のとおり、既定の
                        // メソッドの本体はその参照に辺を引かないので、証拠の辺は呼び出し側が根ごと 4. で引く
                        wanted.push(method.class, root);
                        return;
                    }
                }
                wanted.context(program, instance, &head);
            }
            Resolution::Given => {
                for (l, &arg) in own.iter().enumerate() {
                    self.flow(flow, arg, Node::Method(m, l + 1));
                }
            }
            // タプルの instance は Task 9 で足す。解けない制約は型検査が報告済みである
            Resolution::Tuple(_) | Resolution::Missing => {}
        }
    }

    /// 辺の 4.: 制約の解決の木の、instance で解いた節点ごとに、頭の型引数から instance の節点へ辺を引く。上位クラスの
    /// 同じ頭の instance と、各 instance の文脈も同じ列でたどる。
    fn resolve_wanted(&mut self, flow: &mut Flow, mut wanted: Wanted) {
        let program = self.program;
        while let Some((class, ty)) = wanted.work.pop() {
            let Resolution::Instance {
                instance,
                args: head,
            } = resolve(program, flow.types, class, ty)
            else {
                continue;
            };
            for (j, &arg) in head.iter().enumerate() {
                self.flow(
                    flow,
                    arg,
                    Node::Instance(InstanceNode::Declared(instance), j),
                );
            }
            wanted.context(program, instance, &head);
            for &superclass in &program[class].superclasses {
                wanted.push(superclass, ty);
            }
        }
    }

    /// `ty` に `f` の型変数 i が現れるとき、(f, i) から `to` へ辺を引く。`ty` が i そのものでなければ大きくなる辺である。
    fn flow(&mut self, flow: &mut Flow, ty: TypeId, to: Node) {
        if !self.node_of.contains_key(&to) {
            return;
        }
        let occurs = vars_in(flow.types, ty, flow.names, &mut flow.occurrences);
        for (i, name) in flow.names.iter().enumerate() {
            if occurs[i] {
                let grow = !matches!(flow.types.kind(ty), TypeKind::Rigid(n) if n == name);
                self.edge(Node::Function(flow.f, i), to, grow);
            }
        }
    }

    /// 両端が成分 `of` にある、既定のメソッド `default` への辺の 5. を引いた instance。
    fn default_instance(
        &self,
        default: FunctionId,
        component: &[usize],
        of: usize,
    ) -> Option<InstanceId> {
        let root = *self.node_of.get(&Node::Function(default, 0))?;
        if component[root] != of {
            return None;
        }
        self.default_edges
            .iter()
            .find(|&&(from, _, to)| to == default && component[from] == of)
            .map(|&(_, instance, _)| instance)
    }

    /// 一様な位置 `node` が与えられた制約を持てば、その E2012 (spec の「一様な位置と E2012」)。
    fn constrained(&self, node: Node, component: &[usize], index: usize) -> Option<Constrained> {
        let program = self.program;
        match node {
            Node::Function(id, position) => {
                let function = &program[id];
                let class = constraint_on(&function.signature.as_ref()?.constraints, position)?;
                Some(match function.kind {
                    FunctionKind::Defined | FunctionKind::Extern(_) => Constrained {
                        file: program.file(id.module),
                        range: function.signature_name_range.unwrap_or(function.name_range),
                        name: function.name.clone(),
                        class,
                    },
                    FunctionKind::InstanceMethod(instance, _) => {
                        Constrained::at_instance(program, instance, class)
                    }
                    // 既定のメソッドは Prelude にあることが多く、そこを指しても直す場所が分からないので、この位置を
                    // 含む成分の中で既定のメソッドへの辺を引いた instance の頭を指す
                    FunctionKind::DefaultMethod(method) => {
                        match self.default_instance(id, component, component[index]) {
                            Some(instance) => Constrained::at_instance(program, instance, class),
                            None => Constrained {
                                file: program.file(method.module),
                                range: program[method].name_range,
                                name: program[method].name.clone(),
                                class,
                            },
                        }
                    }
                })
            }
            Node::Instance(InstanceNode::Declared(instance), position) => {
                let class = constraint_on(&program[instance].context, position)?;
                Some(Constrained::at_instance(program, instance, class))
            }
            Node::Instance(InstanceNode::Tuple(_), _) | Node::Method(..) => None,
        }
    }
}

/// 本体の中で共有された型を、各節点1回だけたどるための覚え書き。
struct Flow<'a> {
    f: FunctionId,
    names: &'a [String],
    types: &'a TypeStore,
    occurrences: HashMap<TypeId, Vec<bool>>,
}

/// 辺の 4. の作業の列。同じクラスと型の組は1回だけ解く。
#[derive(Default)]
struct Wanted {
    work: Vec<(ClassId, TypeId)>,
    seen: HashSet<(ClassId, TypeId)>,
}

impl Wanted {
    fn push(&mut self, class: ClassId, ty: TypeId) {
        if self.seen.insert((class, ty)) {
            self.work.push((class, ty));
        }
    }

    /// 参照が求めるシグネチャの制約。具体化の型引数は `Generics` の順に並ぶので、制約の型変数の番号で引ける。
    fn constraints(&mut self, signature: Option<&Signature>, args: &[TypeId]) {
        for constraint in signature.into_iter().flat_map(|s| &s.constraints) {
            if let Some(&ty) = args.get(index(constraint.var)) {
                self.push(constraint.class, ty);
            }
        }
    }

    /// instance の文脈を頭の型引数へ写した制約。
    fn context(&mut self, program: &Program, instance: InstanceId, head: &[TypeId]) {
        for constraint in &program[instance].context {
            if let Some(&ty) = head.get(index(constraint.var)) {
                self.push(constraint.class, ty);
            }
        }
    }
}

/// E2012 の位置と文言の材料。
struct Constrained {
    file: FileId,
    range: TextRange,
    name: String,
    class: ClassId,
}

impl Constrained {
    fn at_instance(program: &Program, instance: InstanceId, class: ClassId) -> Constrained {
        let def = &program[instance];
        // 型引数のある頭は、制約の表示 (`Same (Box a)`) と同じく括弧で囲む
        let head = instance_head(program, instance);
        let head = if def.generics.type_vars.is_empty() {
            head
        } else {
            format!("({head})")
        };
        Constrained {
            file: program.file(instance.module),
            range: def.head_range,
            name: format!("{} {head}", program.names.class(def.class)),
            class,
        }
    }

    fn diagnostic(&self, program: &Program) -> Diagnostic {
        Diagnostic::error(
            codes::CONSTRAINED_POLYMORPHIC_RECURSION,
            format!(
                "`{}` would need an instance of `{}` at infinitely many types",
                self.name,
                program.names.class(self.class)
            ),
            Label::new(
                self.file,
                self.range,
                "a constrained type variable grows on each recursive call",
            ),
        )
        .with_note("instances are chosen at compile time, so a constraint cannot follow polymorphic recursion")
    }
}

/// 型変数 `position` への最初の制約のクラス。
fn constraint_on(constraints: &[eml_hir::Constraint], position: usize) -> Option<ClassId> {
    constraints
        .iter()
        .find(|constraint| index(constraint.var) == position)
        .map(|constraint| constraint.class)
}

fn index(var: TypeVarId) -> usize {
    u32::from(var.into_raw()) as usize
}

/// シグネチャの型変数の名前。並びは具体化の表の型引数の順と同じである。
fn type_vars(signature: Option<&Signature>) -> Vec<String> {
    signature
        .map(|signature| {
            signature
                .generics
                .type_vars
                .iter()
                .map(|(_, var)| var.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// `ty` に現れる `names` の型変数 (番号ごとの真偽)。`substitute` がたどる位置 (型構成子の引数、関数型の引数と結果と
/// エフェクトの型引数、レコードとタプルの要素) を見る。結果を `memo` に覚え、共有された型は1回だけたどる。
fn vars_in(
    types: &TypeStore,
    ty: TypeId,
    names: &[String],
    memo: &mut HashMap<TypeId, Vec<bool>>,
) -> Vec<bool> {
    if !types.contains_type_vars(ty) {
        return vec![false; names.len()];
    }
    if let Some(found) = memo.get(&ty) {
        return found.clone();
    }
    let mut found = vec![false; names.len()];
    let mut merge = |child: Vec<bool>| {
        for (slot, child) in found.iter_mut().zip(child) {
            *slot |= child;
        }
    };
    match types.kind(ty) {
        TypeKind::Rigid(name) => {
            if let Some(position) = names.iter().position(|n| n == name) {
                merge({
                    let mut one = vec![false; names.len()];
                    one[position] = true;
                    one
                });
            }
        }
        TypeKind::Con { args, .. } => {
            for &arg in args {
                merge(vars_in(types, arg, names, memo));
            }
        }
        TypeKind::Record(fields) => {
            for &(_, field) in fields {
                merge(vars_in(types, field, names, memo));
            }
        }
        TypeKind::Fn {
            param,
            effects,
            ret,
            ..
        } => {
            merge(vars_in(types, *param, names, memo));
            for label in effects {
                for &arg in &label.args {
                    merge(vars_in(types, arg, names, memo));
                }
            }
            merge(vars_in(types, *ret, names, memo));
        }
        TypeKind::OpVar(_) | TypeKind::Flexible | TypeKind::Error => {}
    }
    memo.insert(ty, found.clone());
    found
}

/// 強連結成分の番号。Tarjan の方法を作業の列で行い、グラフの深さに比例して Rust のスタックを使わない。
fn strongly_connected(edges: &[Vec<(usize, bool)>]) -> Vec<usize> {
    const UNSEEN: usize = usize::MAX;
    let mut index = vec![UNSEEN; edges.len()];
    let mut low = vec![0; edges.len()];
    let mut on_stack = vec![false; edges.len()];
    let mut stack = Vec::new();
    let mut component = vec![UNSEEN; edges.len()];
    let (mut next_index, mut next_component) = (0, 0);
    for root in 0..edges.len() {
        if index[root] != UNSEEN {
            continue;
        }
        // (節点, 次に見る辺の位置)
        let mut work = vec![(root, 0)];
        index[root] = next_index;
        low[root] = next_index;
        next_index += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(&(node, edge)) = work.last() {
            if let Some(&(to, _)) = edges[node].get(edge) {
                work.last_mut().expect("read above").1 += 1;
                if index[to] == UNSEEN {
                    index[to] = next_index;
                    low[to] = next_index;
                    next_index += 1;
                    stack.push(to);
                    on_stack[to] = true;
                    work.push((to, 0));
                } else if on_stack[to] {
                    low[node] = low[node].min(index[to]);
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                low[parent] = low[parent].min(low[node]);
            }
            if low[node] == index[node] {
                loop {
                    let member = stack
                        .pop()
                        .expect("the root of a component is on the stack");
                    on_stack[member] = false;
                    component[member] = next_component;
                    if member == node {
                        break;
                    }
                }
                next_component += 1;
            }
        }
    }
    component
}
