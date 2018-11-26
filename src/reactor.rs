use mio::{Event, Events, Poll, Token};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    time::{Duration, Instant},
};

pub struct Reactor {
    pub poll: Poll,
    quit: bool,
    token_ticker: usize,
    event_listeners: BTreeMap<Token, Rc<RefCell<FnMut(&mut Reactor, Event)>>>,
    timeout_listeners: Vec<(Instant, Box<FnOnce(&mut Reactor)>)>,
}

impl Reactor {
    pub fn run(initializer: impl FnOnce(&mut Reactor)) {
        let mut reactor = Reactor::new();
        initializer(&mut reactor);
        reactor.run_internal();
    }

    fn new() -> Reactor {
        let poll = Poll::new().expect("failed to create poll");
        let quit = false;
        let token_ticker = 0;
        let event_listeners = Default::default();
        let timeout_listeners = Default::default();
        Reactor {
            poll,
            quit,
            token_ticker,
            event_listeners,
            timeout_listeners,
        }
    }

    pub fn quit(&mut self) {
        self.quit = true;
    }

    pub fn set_event_listener(
        &mut self,
        token: Token,
        listener: impl FnMut(&mut Reactor, Event) + 'static,
    ) {
        self.event_listeners
            .insert(token, Rc::new(RefCell::new(listener)));
    }

    pub fn remove_event_listener(&mut self, token: Token) {
        self.event_listeners.remove(&token);
    }

    pub fn set_timeout(
        &mut self,
        timeout: Duration,
        callback: impl FnOnce(&mut Reactor) + 'static,
    ) {
        let run_at = Instant::now() + timeout;
        self.timeout_listeners.push((run_at, Box::new(callback)));
    }

    pub fn set_interval(
        &mut self,
        interval: Duration,
        callback: impl FnMut(&mut Reactor) + 'static,
    ) -> IntervalHandle {
        let handle = IntervalHandle {
            cancelled: Rc::new(RefCell::new(false)),
        };
        let callback = Rc::new(RefCell::new(callback));
        self.reschedule_interval(interval, handle.clone(), callback);
        handle
    }

    fn reschedule_interval(
        &mut self,
        interval: Duration,
        handle: IntervalHandle,
        callback: Rc<RefCell<dyn FnMut(&mut Reactor)>>,
    ) {
        self.set_timeout(interval, move |proxy| {
            if !handle.is_cancelled() {
                proxy.reschedule_interval(interval, handle, callback.clone());
                (&mut *callback.borrow_mut())(proxy);
            }
        });
    }

    pub fn issue_token(&mut self) -> Token {
        let mine = self.token_ticker;
        if mine == std::usize::MAX {
            panic!("Out of tokens");
        }
        self.token_ticker += 1;
        Token(mine)
    }

    fn calculate_duration(&self) -> CalculateDurationResult {
        let now = Instant::now();
        let mut duration = None;
        let mut fire_at = None;
        let mut idx = 0;
        for (candidate_idx, timeout) in self.timeout_listeners.iter().enumerate() {
            let candidate = timeout.0.duration_since(now);
            if duration.is_none() || candidate < duration.unwrap() {
                duration = Some(candidate);
                fire_at = Some(timeout.0);
                idx = candidate_idx;
            }
        }
        CalculateDurationResult {
            duration,
            fire_at,
            idx,
        }
    }

    fn is_empty(&self) -> bool {
        self.timeout_listeners.is_empty() && self.event_listeners.is_empty()
    }

    fn run_internal(&mut self) {
        let mut events = Events::with_capacity(1024);
        while !self.quit && !self.is_empty() {
            let duration = self.calculate_duration();
            self.poll
                .poll(&mut events, duration.duration)
                .expect("poll failed");
            if let Some(fire_at) = duration.fire_at {
                if Instant::now() >= fire_at {
                    let (_, callback) = self.timeout_listeners.remove(duration.idx);
                    callback(self);
                    if self.quit {
                        break;
                    }
                }
            }
            for event in events.iter() {
                let token = event.token();
                let handler = self.event_listeners.get(&token).cloned();
                if let Some(handler) = handler {
                    (&mut *handler.borrow_mut())(self, event);
                    if self.quit {
                        break;
                    }
                }
            }
        }
    }
}

struct CalculateDurationResult {
    duration: Option<Duration>,
    fire_at: Option<Instant>,
    idx: usize,
}

#[derive(Clone)]
pub struct IntervalHandle {
    cancelled: Rc<RefCell<bool>>,
}

impl IntervalHandle {
    pub fn cancel(&self) {
        *self.cancelled.borrow_mut() = true;
    }

    fn is_cancelled(&self) -> bool {
        *self.cancelled.borrow()
    }
}
