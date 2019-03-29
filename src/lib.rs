extern crate crossbeam_channel;
extern crate im;

use crossbeam_channel::{unbounded, Receiver, Sender};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::thread::JoinHandle;
use im::vector::Vector;
use std::cmp::PartialEq;

pub trait Action: Copy + Clone + 'static {
    type Request: Copy + Clone + Send + Sync;
    type Response: Copy + Clone + Send + Sync + PartialEq + Default;
}

pub trait Model<A: Action, S>: Send + Sync + 'static {
    fn init() -> Self;

    fn get_state(&self) -> S;

    fn from_state(state: &S) -> Self;

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
    runner: Arc<R>,
    threads: Vec<JoinHandle<()>>,
    history: Arc<RwLock<Vec<Entry<A>>>>,
    entry_sender: Sender<Entry<A>>,
    counter: Arc<AtomicUsize>,
    _p1: PhantomData<A>,
    _p2: PhantomData<S>,
    _p3: PhantomData<M>,
}

pub struct ImmutableChecker<A: Action, S, M: Model<A, S>> {
    history: Vector<Entry<A>>,
    state: S,
    _p1: PhantomData<M>,
}

impl<A: Action, S, M: Model<A, S>> ImmutableChecker<A, S, M> {
    pub fn new(history: Vector<Entry<A>>, state: S) -> Self {
        Self {
            history,
            state,
            _p1: PhantomData
        }
    }
    pub fn check(&self) -> bool {
        let mut ret = false;
        for (index, entry) in self.history.iter().enumerate() {
            match entry.data {
                EntryData::Invoke(invoke) => {
                    let model = M::from_state(&self.state);

                    let mut history = self.history.clone();
                    let mut return_index = index;
                    let mut expect_response= A::Response::default();
                    for i in index+1..self.history.len() {
                        if self.history[i].id == entry.id {
                            return_index = i;
                            match self.history[i].data {
                                EntryData::Invoke(_) => unreachable!(),
                                EntryData::Response(res) => expect_response = res,
                            }
                            break;
                        }
                    }
                    history.remove(index);
                    history.remove(return_index - 1);

                    let response = model.invoke(invoke);
                    if response == expect_response {
                        let sub_checker: ImmutableChecker<A, S, M> = ImmutableChecker::new(history, model.get_state());
                        let res = sub_checker.check();

                        if res {
                            ret = true;
                            break;
                        }
                    } else {
                        continue;
                    }
                }
                _ => {
                    break;
                }
            }
        }
        return ret;
    }
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
            runner: Arc::new(R::init()),
            threads: Vec::new(),
            history,
            entry_sender: s,
            counter: Arc::new(AtomicUsize::new(0)),
            _p1: PhantomData,
            _p2: PhantomData,
            _p3: PhantomData,
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
    pub fn check(&self, init_state: S) -> bool {
        let history = Vector::from(self.history.read().unwrap().clone());
        let im_checker: ImmutableChecker<A, S, M> = ImmutableChecker::new(history, init_state);

        im_checker.check()
    }
}

