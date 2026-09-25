/// Connection states mandated by the MCP initialization lifecycle.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lifecycle {
    #[default]
    New,
    AwaitingInitialized,
    Ready,
}

impl Lifecycle {
    pub(crate) fn begin_initialization(&mut self) -> bool {
        if *self != Self::New {
            return false;
        }
        *self = Self::AwaitingInitialized;
        true
    }

    pub(crate) fn mark_ready(&mut self) {
        if *self == Self::AwaitingInitialized {
            *self = Self::Ready;
        }
    }

    pub(crate) fn is_ready(self) -> bool {
        self == Self::Ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialization_is_only_accepted_once() {
        let mut lifecycle = Lifecycle::default();
        assert!(lifecycle.begin_initialization());
        assert!(!lifecycle.begin_initialization());
        assert!(!lifecycle.is_ready());
        lifecycle.mark_ready();
        assert!(lifecycle.is_ready());
    }
}
