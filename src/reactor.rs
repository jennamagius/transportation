use mio::{Event, Events, Poll, Token};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Reactor {
    i: Rc<RefCell<ReactorInternal>>,
}

pub struct ReactorWeak {
    i: std::rc::Weak<RefCell<ReactorInternal>>,
}

impl ReactorWeak {
    pub fn upgrade(&self) -> Option<Reactor> {
        Some(Reactor {
            i: self.i.upgrade()?,
        })
    }
}

struct ReactorInternal {
    poll: Poll,
    quit: bool,
    token_ticker: usize,
    event_listeners: BTreeMap<Token, Rc<RefCell<FnMut(Event)>>>,
    timeout_listeners: Vec<(Instant, Box<FnOnce()>)>,
}

impl Reactor {
    pub fn run(&self) {
        self.run_internal();
    }

    pub fn downgrade(&self) -> ReactorWeak {
        ReactorWeak {
            i: Rc::downgrade(&self.i),
        }
    }

    pub fn strong_count(&self) -> usize {
        Rc::strong_count(&self.i)
    }

    pub fn poll<T, R>(&self, callback: T) -> R
    where
        T: FnOnce(&Poll) -> R,
    {
        let lock = self.i.borrow();
        callback(&lock.poll)
    }

    pub fn new() -> Reactor {
        let poll = Poll::new().expect("failed to create poll");
        let quit = false;
        let token_ticker = 0;
        let event_listeners = Default::default();
        let timeout_listeners = Default::default();
        Reactor {
            i: Rc::new(RefCell::new(ReactorInternal {
                poll,
                quit,
                token_ticker,
                event_listeners,
                timeout_listeners,
            })),
        }
    }

    pub fn quit(&self) {
        self.i.borrow_mut().quit = true;
    }

    pub fn set_event_listener(&self, token: Token, listener: impl FnMut(Event) + 'static) {
        self.i
            .borrow_mut()
            .event_listeners
            .insert(token, Rc::new(RefCell::new(listener)));
    }

    pub fn remove_event_listener(&self, token: Token) {
        self.i.borrow_mut().event_listeners.remove(&token);
    }

    pub fn set_timeout(&self, timeout: Duration, callback: impl FnOnce() + 'static) {
        let run_at = Instant::now() + timeout;
        self.i
            .borrow_mut()
            .timeout_listeners
            .push((run_at, Box::new(callback)));
    }

    pub fn set_interval(
        &self,
        interval: Duration,
        callback: impl FnMut() + 'static,
    ) -> IntervalHandle {
        let handle = IntervalHandle {
            cancelled: Rc::new(RefCell::new(false)),
        };
        let callback = Rc::new(RefCell::new(callback));
        self.reschedule_interval(interval, handle.clone(), callback);
        handle
    }

    fn reschedule_interval(
        &self,
        interval: Duration,
        handle: IntervalHandle,
        callback: Rc<RefCell<dyn FnMut()>>,
    ) {
        let proxy = self.clone();
        self.set_timeout(interval, move || {
            if !handle.is_cancelled() {
                proxy.reschedule_interval(interval, handle, callback.clone());
                (&mut *callback.borrow_mut())();
            }
        });
    }

    pub fn issue_token(&self) -> Token {
        let mine = self.i.borrow().token_ticker;
        if mine == std::usize::MAX {
            panic!("Out of tokens");
        }
        self.i.borrow_mut().token_ticker += 1;
        Token(mine)
    }

    fn calculate_duration(&self) -> CalculateDurationResult {
        let now = Instant::now();
        let mut duration = None;
        let mut fire_at = None;
        let mut idx = 0;
        for (candidate_idx, timeout) in self.i.borrow().timeout_listeners.iter().enumerate() {
            let candidate = if timeout.0 > now {
                timeout.0.duration_since(now)
            } else {
                Duration::from_millis(0)
            };
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
        self.i.borrow().timeout_listeners.is_empty() && self.i.borrow().event_listeners.is_empty()
    }

    fn run_internal(&self) {
        let mut events = Events::with_capacity(1024);
        while !self.i.borrow().quit && !self.is_empty() {
            let duration = self.calculate_duration();
            self.i
                .borrow()
                .poll
                .poll(&mut events, duration.duration)
                .expect("poll failed");
            if let Some(fire_at) = duration.fire_at {
                if Instant::now() >= fire_at {
                    let (_, callback) = self.i.borrow_mut().timeout_listeners.remove(duration.idx);
                    callback();
                    if self.i.borrow().quit {
                        break;
                    }
                }
            }
            for event in events.iter() {
                let token = event.token();
                let handler = self.i.borrow().event_listeners.get(&token).cloned();
                if let Some(handler) = handler {
                    (&mut *handler.borrow_mut())(event);
                    if self.i.borrow().quit {
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
