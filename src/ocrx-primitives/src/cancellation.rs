//! Cooperative cancellation shared by runtime, scan, and CLI layers.
//!
//! The first cancellation request marks the current job as cancelled (used by
//! the CLI to stop taking new pages and return exit code 130). A second
//! request escalates to force-exit, mirroring the frozen Ctrl+C contract
//! where the first interrupt is graceful and the second is immediate.
//!
//! A [`CancellationToken::child`] scopes a subsystem's own lifecycle (e.g. a
//! renderer dropping its prefetch): the child reports cancelled/forced as soon
//! as any ancestor is, while cancelling the child never touches the ancestor —
//! that is what keeps one document's teardown from cancelling a whole
//! orchestrated invocation.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    requests: AtomicU8,
    parent: Option<Arc<Inner>>,
}

/// Walks the ancestor chain (self included) for the strongest observed state.
fn chain_state(inner: &Inner) -> (bool, bool) {
    let mut current = Some(inner);
    let mut cancelled = false;
    let mut forced = false;
    while let Some(node) = current {
        let value = node.requests.load(Ordering::SeqCst);
        cancelled |= value >= 1;
        forced |= value >= 2;
        current = node.parent.as_deref();
    }
    (cancelled, forced)
}

impl CancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A lifecycle-scoped child token: it reports cancelled/forced as soon as
    /// it OR any ancestor is, while its own `request_cancel` stays local to
    /// the child and never propagates upward.
    #[must_use]
    pub fn child(&self) -> Self {
        Self {
            inner: Arc::new(Inner {
                requests: AtomicU8::new(0),
                parent: Some(Arc::clone(&self.inner)),
            }),
        }
    }

    pub fn request_cancel(&self) {
        self.inner
            .requests
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                Some(value.saturating_add(1))
            })
            .ok();
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.requests.load(Ordering::SeqCst) >= 1 || chain_state(&self.inner).0
    }

    #[must_use]
    pub fn force_exit(&self) -> bool {
        self.inner.requests.load(Ordering::SeqCst) >= 2 || chain_state(&self.inner).1
    }
}

#[cfg(test)]
mod tests {
    use super::CancellationToken;

    // --- BLK-1: child tokens scope subsystem lifecycle teardown. -----------

    #[test]
    fn child_observes_ancestors_but_its_cancel_stays_local() {
        let parent = CancellationToken::new();
        let child = parent.child();
        // Without any request, neither side is cancelled.
        assert!(!parent.is_cancelled());
        assert!(!child.is_cancelled());
        // Cancelling the child (renderer teardown) must NOT touch the parent.
        child.request_cancel();
        assert!(child.is_cancelled());
        assert!(!parent.is_cancelled(), "child cancel leaked to the parent");
        assert!(!parent.force_exit());
        // Cancelling the parent (user Ctrl+C) must cancel every descendant.
        let other_child = parent.child();
        parent.request_cancel();
        assert!(other_child.is_cancelled());
        assert!(
            !other_child.force_exit(),
            "single parent cancel is graceful"
        );
        // Escalation on the parent is observed by descendants too.
        parent.request_cancel();
        assert!(parent.force_exit());
        assert!(other_child.force_exit());
    }

    #[test]
    fn child_chain_propagates_through_nesting() {
        let root = CancellationToken::new();
        let mid = root.child();
        let leaf = mid.child();
        assert!(!leaf.is_cancelled());
        // The leaf's own double request escalates only itself.
        leaf.request_cancel();
        leaf.request_cancel();
        assert!(leaf.force_exit());
        assert!(!mid.is_cancelled());
        assert!(!mid.force_exit());
        // Ancestor single cancel reaches the deepest descendant gracefully; a
        // sibling that self-escalated keeps its own stronger state.
        let leaf2 = mid.child();
        root.request_cancel();
        assert!(leaf.is_cancelled());
        assert!(leaf2.is_cancelled());
        assert!(!leaf2.force_exit(), "one ancestor cancel stays graceful");
        assert!(leaf.force_exit(), "self-escalated leaf keeps its own state");
    }

    #[test]
    fn child_keeps_observing_after_the_parent_handle_is_dropped() {
        // The child keeps the ancestor chain alive through its own Arc, so a
        // dropped parent handle leaves its recorded state observable.
        let child = {
            let parent = CancellationToken::new();
            let child = parent.child();
            parent.request_cancel();
            child
        };
        assert!(child.is_cancelled());
        assert!(!child.force_exit());
    }

    #[test]
    fn first_request_is_graceful_and_second_escalates() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        assert!(!token.force_exit());
        token.request_cancel();
        assert!(token.is_cancelled());
        assert!(!token.force_exit());
        token.request_cancel();
        assert!(token.is_cancelled());
        assert!(token.force_exit());
    }
}
