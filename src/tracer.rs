use evm_event::Event;

pub trait EventTracer: Default {
    fn push(&mut self, _event: Event) {}

    fn peek(&self) -> &[Event] {
        &[]
    }

    fn take(&mut self) -> Vec<Event> {
        vec![]
    }

    fn fork(&self) -> Self {
        Self::default()
    }

    fn join(&mut self, mut other: Self, reverted: bool) {
        for mut event in other.take() {
            event.reverted = reverted;
            self.push(event);
        }
    }
}

#[derive(Default)]
pub struct NoopTracer;

impl EventTracer for NoopTracer {}

#[derive(Default)]
pub struct LoggingTracer(Vec<Event>);

impl EventTracer for LoggingTracer {
    fn push(&mut self, event: Event) {
        self.0.push(event);
    }

    fn peek(&self) -> &[Event] {
        &self.0
    }

    fn take(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.0)
    }
}
