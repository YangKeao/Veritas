extern crate crossbeam_channel;

use crossbeam_channel::{unbounded, Receiver, Sender};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::thread::JoinHandle;

pub trait Action: Copy + Clone + 'static {
    type Request: Copy + Clone + Send + Sync;
    type Response: Copy + Clone + Send + Sync;
}

pub trait Model<A: Action, S>: Send + Sync + 'static {
    fn init() -> Self;

    fn get_state() -> S;

    fn invoke(&self, action: A::Request) -> A::Response;
}

#[derive(Copy, Clone)]
pub enum EntryData<A: Action> {
    Invoke(A::Request),
    Response(A::Response),
}

#[derive(Copy, Clone)]
pub struct Entry<A: Action> {
    id: usize,
    data: EntryData<A>,
}

pub struct Checker<A: Action, S, M: Model<A, S>, R: Model<A, S>> {
    model: M,
    runner: Arc<R>,
    threads: Vec<JoinHandle<()>>,
    history: Arc<RwLock<Vec<Entry<A>>>>,
    entry_sender: Sender<Entry<A>>,
    counter: Arc<AtomicUsize>,
    _p1: PhantomData<A>,
    _p2: PhantomData<S>,
}

impl<A: Action, S, M: Model<A, S>, R: Model<A, S>> Checker<A, S, M, R> {
    pub fn new() -> Self {
        let (s, r) = unbounded();
        let history = Arc::new(RwLock::new(Vec::new()));
        let receive_history = history.clone();
        let receiver = r.clone();
        thread::spawn(move || {
            let res = receiver.recv().unwrap();
            receive_history.write().unwrap().push(res);
        });
        return Self {
            model: M::init(),
            runner: Arc::new(R::init()),
            threads: Vec::new(),
            history,
            entry_sender: s,
            counter: Arc::new(AtomicUsize::new(0)),
            _p1: PhantomData,
            _p2: PhantomData,
        };
    }
    pub fn add_thread(&mut self, history: Vec<A::Request>) {
        let runner = self.runner.clone();
        let sender = self.entry_sender.clone();
        let counter = self.counter.clone();

        self.threads.push(thread::spawn(move || {
            for action in history {
                let id = counter.fetch_add(1, Ordering::SeqCst);
                sender
                    .send(Entry::<A> {
                        id,
                        data: EntryData::Invoke(action),
                    })
                    .unwrap();
                let res = runner.invoke(action);
                sender
                    .send(Entry::<A> {
                        id,
                        data: EntryData::Response(res),
                    })
                    .unwrap();
            }
        }));
    }
}
