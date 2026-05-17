use leptos::prelude::*;

#[derive(Clone, Copy)]
pub(crate) struct RequestEpoch {
    current: ReadSignal<u64>,
    set_current: WriteSignal<u64>,
}

impl RequestEpoch {
    pub(crate) fn new() -> Self {
        let (current, set_current) = signal(0);
        Self {
            current,
            set_current,
        }
    }

    pub(crate) fn next(&self) -> u64 {
        let next = self.current.get_untracked().wrapping_add(1).max(1);
        self.set_current.set(next);
        next
    }

    pub(crate) fn is_current(&self, ticket: u64) -> bool {
        is_current_ticket(self.current.get_untracked(), ticket)
    }
}

fn is_current_ticket(current: u64, ticket: u64) -> bool {
    current == ticket
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_ticket_becomes_stale_after_next_request() {
        assert!(!is_current_ticket(2, 1));
        assert!(is_current_ticket(2, 2));
    }
}
