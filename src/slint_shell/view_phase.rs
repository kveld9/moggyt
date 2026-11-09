//! View lifecycle phase definitions and legal phase transition rules.

/// Lifecycle phase for a primary UI container view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewPhase {
    #[default]
    Idle,
    Loading,
    Ready,
    Empty,
    Error,
}

/// Identifier for primary navigation container views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewId {
    Library,
    Search,
    Detail,
    Explore,
    Category,
}

impl ViewPhase {
    /// Returns the next legal phase. Idle can only be left through Loading; any
    /// settled phase may return to Idle or reload; repeating a phase is a no-op.
    pub fn transition(self, next: ViewPhase) -> Self {
        if self == next {
            return self;
        }
        if next == ViewPhase::Idle {
            return ViewPhase::Idle;
        }
        if next == ViewPhase::Loading {
            return ViewPhase::Loading;
        }
        if self == ViewPhase::Idle {
            return self;
        }
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_table() {
        let all_phases = [
            ViewPhase::Idle,
            ViewPhase::Loading,
            ViewPhase::Ready,
            ViewPhase::Empty,
            ViewPhase::Error,
        ];

        for next in all_phases {
            let expected =
                if next == ViewPhase::Loading { ViewPhase::Loading } else { ViewPhase::Idle };
            assert_eq!(ViewPhase::Idle.transition(next), expected);
        }

        assert_eq!(ViewPhase::Idle.transition(ViewPhase::Idle), ViewPhase::Idle);
        assert_eq!(ViewPhase::Idle.transition(ViewPhase::Loading), ViewPhase::Loading);
        assert_eq!(ViewPhase::Idle.transition(ViewPhase::Ready), ViewPhase::Idle);
        assert_eq!(ViewPhase::Idle.transition(ViewPhase::Empty), ViewPhase::Idle);
        assert_eq!(ViewPhase::Idle.transition(ViewPhase::Error), ViewPhase::Idle);

        assert_eq!(ViewPhase::Ready.transition(ViewPhase::Ready), ViewPhase::Ready);
        assert_eq!(ViewPhase::Ready.transition(ViewPhase::Loading), ViewPhase::Loading);
        assert_eq!(ViewPhase::Ready.transition(ViewPhase::Idle), ViewPhase::Idle);
        assert_eq!(ViewPhase::Ready.transition(ViewPhase::Empty), ViewPhase::Empty);
        assert_eq!(ViewPhase::Ready.transition(ViewPhase::Error), ViewPhase::Error);

        assert_eq!(ViewPhase::Empty.transition(ViewPhase::Empty), ViewPhase::Empty);
        assert_eq!(ViewPhase::Empty.transition(ViewPhase::Loading), ViewPhase::Loading);
        assert_eq!(ViewPhase::Empty.transition(ViewPhase::Idle), ViewPhase::Idle);
        assert_eq!(ViewPhase::Empty.transition(ViewPhase::Ready), ViewPhase::Ready);
        assert_eq!(ViewPhase::Empty.transition(ViewPhase::Error), ViewPhase::Error);

        assert_eq!(ViewPhase::Error.transition(ViewPhase::Error), ViewPhase::Error);
        assert_eq!(ViewPhase::Error.transition(ViewPhase::Loading), ViewPhase::Loading);
        assert_eq!(ViewPhase::Error.transition(ViewPhase::Idle), ViewPhase::Idle);
        assert_eq!(ViewPhase::Error.transition(ViewPhase::Ready), ViewPhase::Ready);
        assert_eq!(ViewPhase::Error.transition(ViewPhase::Empty), ViewPhase::Empty);

        let settled_phases = [ViewPhase::Ready, ViewPhase::Empty, ViewPhase::Error];
        for current in settled_phases {
            for next in all_phases {
                assert_eq!(current.transition(next), next);
            }
        }
    }
}
