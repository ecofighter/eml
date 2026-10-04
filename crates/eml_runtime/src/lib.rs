mod heap;
mod output;

pub use heap::{Closure, Frame, Heap, HeapError, ObjRef, Payload, Value};
pub use output::{Captured, OutputSink};
