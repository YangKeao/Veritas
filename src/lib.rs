extern crate crossbeam_channel;
extern crate im;

use crossbeam_channel::{unbounded, Receiver, Select, Sender};
use im::vector::Vector;
use std::cmp::PartialEq;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::thread::JoinHandle;

pub trait Action: Copy + Clone + 'static {
    type Request: Copy + Clone + Send + Sync;
    type Response: Copy + Clone + Send + Sync + PartialEq + Default;
}

pub trait Runner<A: Action>: Send + Sync + 'static {
    fn init() -> Self;
    fn invoke(&self, action: A::Request) -> A::Response;
}

pub trait Model<A: Action, S: Sync + Send + Clone + 'static>:
    Send + Sync + Runner<A> + 'static
{
    fn get_state(&self) -> S;
    fn from_state(state: &S) -> Self;
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

pub struct Checker<A: Action, S: Sync + Send + Clone + 'static, M: Model<A, S>, R: Runner<A>> {
    runner: Arc<R>,
    threads: Vec<Option<JoinHandle<()>>>,
    history: Arc<RwLock<Vec<Entry<A>>>>,
    entry_sender: Option<Sender<Entry<A>>>,
    counter: Arc<AtomicUsize>,
    _p1: PhantomData<A>,
    _p2: PhantomData<S>,
    _p3: PhantomData<M>,
}

pub struct CheckerRunner<A: Action, S: Sync + Send + Clone + 'static, M: Model<A, S>> {
    sender: Sender<(ImmutableChecker<A, S, M>, Sender<bool>)>,
    pub receiver: Receiver<(ImmutableChecker<A, S, M>, Sender<bool>)>,
}
impl<A: Action, S: Sync + Send + Clone + 'static, M: Model<A, S>> CheckerRunner<A, S, M> {
    fn new(thread_num: usize, stack_size: usize) -> Self {
        let (s, r) = unbounded::<(ImmutableChecker<A, S, M>, Sender<bool>)>();
        for i in 0..thread_num {
            let receiver = r.clone();
            thread::Builder::new()
                .stack_size(stack_size)
                .name(format!("Runner Thread {}", i))
                .spawn(move || {
                    // TODO: Set stack size carefully
                    loop {
                        match receiver.recv() {
                            Ok((checker, sender)) => {
                                match sender.send(checker.check()) {
                                    Ok(_) => {}
                                    Err(_) => {}
                                };
                            }
                            Err(_) => {
                                break;
                            }
                        }
                    }
                })
                .unwrap();
        }
        Self {
            sender: s,
            receiver: r,
        }
    }
    fn run(&self, checker: ImmutableChecker<A, S, M>, sender: Sender<bool>) {
        self.sender.send((checker.clone(), sender.clone())).unwrap();
    }
}

pub struct ImmutableChecker<A: Action, S: Sync + Send + Clone + 'static, M: Model<A, S>> {
    history: Vector<Entry<A>>,
    check_runner: Arc<CheckerRunner<A, S, M>>,
    state: S,
    _p1: PhantomData<M>,
}
impl<A: Action, S: Sync + Send + Clone + 'static, M: Model<A, S>> Clone
    for ImmutableChecker<A, S, M>
{
    fn clone(&self) -> Self {
        Self {
            history: self.history.clone(),
            check_runner: self.check_runner.clone(),
            state: self.state.clone(),
            _p1: PhantomData,
        }
    }
}

impl<A: Action, S: Sync + Send + Clone + 'static, M: Model<A, S>> ImmutableChecker<A, S, M> {
    pub fn new(
        history: Vector<Entry<A>>,
        state: S,
        check_runner: Arc<CheckerRunner<A, S, M>>,
    ) -> Self {
        Self {
            history,
            check_runner,
            state,
            _p1: PhantomData,
        }
    }
    pub fn check(&self) -> bool {
        let mut ret = false;
        let (sender, receiver) = unbounded();
        for (index, entry) in self.history.iter().enumerate() {
            match entry.data {
                EntryData::Invoke(invoke) => {
                    let model = M::from_state(&self.state);

                    let mut history = self.history.clone();
                    let mut return_index = index;
                    let mut expect_response = A::Response::default();
                    for i in index + 1..self.history.len() {
                        if self.history[i].id == entry.id {
                            return_index = i;
                            match self.history[i].data {
                                EntryData::Invoke(_) => unreachable!(),
                                EntryData::Response(res) => expect_response = res,
                            }
                            break;
                        }
                    }

                    let response = model.invoke(invoke);
                    if response == expect_response {
                        if history.len() > 2 {
                            history.remove(index);
                            history.remove(return_index - 1);

                            let sub_checker: ImmutableChecker<A, S, M> = ImmutableChecker::new(
                                history,
                                model.get_state(),
                                self.check_runner.clone(),
                            );
                            self.check_runner.run(sub_checker, sender.clone());
                        } else {
                            ret = true;
                            break;
                        }
                    }
                }
                _ => {
                    break;
                }
            }
        }

        drop(sender);
        let mut sel = Select::new();
        let recv_op = sel.recv(&receiver);
        let help_op = sel.recv(&self.check_runner.receiver);

        loop {
            let op = sel.select();
            if op.index() == recv_op {
                match op.recv(&receiver) {
                    Ok(val) => {
                        if val {
                            ret = true;
                            break;
                        }
                    }
                    Err(_) => {
                        break;
                    }
                }
            } else if op.index() == help_op {
                match op.recv(&self.check_runner.receiver) {
                    Ok((checker, sender)) => match sender.send(checker.check()) {
                        Ok(_) => {}
                        Err(_) => {}
                    },
                    Err(_) => unreachable!(),
                }
            } else {
                unreachable!()
            }
        }
        return ret;
    }
}

impl<A: Action, S: Sync + Send + Clone + 'static, M: Model<A, S>, R: Runner<A>>
    Checker<A, S, M, R>
{
    pub fn new() -> Self {
        let (s, r) = unbounded();
        let history = Arc::new(RwLock::new(Vec::new()));
        let receive_history = history.clone();
        let mut threads = Vec::new();
        threads.push(Some(thread::spawn(move || loop {
            let res = match r.recv() {
                Ok(res) => res,
                Err(_) => break,
            };
            receive_history.write().unwrap().push(res);
        })));
        return Self {
            runner: Arc::new(R::init()),
            threads,
            history,
            entry_sender: Some(s),
            counter: Arc::new(AtomicUsize::new(0)),
            _p1: PhantomData,
            _p2: PhantomData,
            _p3: PhantomData,
        };
    }
    pub fn add_thread(&mut self, history: Vec<A::Request>) {
        let runner = self.runner.clone();
        let sender = match &self.entry_sender {
            Some(sender) => sender.clone(),
            None => unreachable!(),
        };
        let counter = self.counter.clone();

        self.threads.push(Some(thread::spawn(move || {
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
        })));
    }
    pub fn check(&mut self, init_state: S, thread_num: usize, stack_size: usize) -> bool {
        let history = Vector::from(self.history.read().unwrap().clone());
        let check_runner = Arc::new(CheckerRunner::new(thread_num, stack_size));
        let im_checker: ImmutableChecker<A, S, M> =
            ImmutableChecker::new(history, init_state, check_runner.clone());

        return thread::Builder::new()
            .stack_size(stack_size)
            .name(String::from("Main Check Thread"))
            .spawn(move || im_checker.check())
            .unwrap()
            .join()
            .unwrap();
    }
    pub fn finish_prepare(&mut self) {
        drop(self.entry_sender.take().unwrap());
        for thread in self.threads.iter_mut() {
            thread.take().unwrap().join().unwrap();
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    struct Stack {
        data: RwLock<Vec<u32>>,
    }
    #[derive(Copy, Clone)]
    struct StackAction {}
    #[derive(Copy, Clone)]
    enum StackRequest {
        PushAction(PushAction),
        #[allow(dead_code)]
        PopAction,
    }
    #[derive(Copy, Clone)]
    struct PushAction {
        data: u32,
    }
    impl Action for StackAction {
        type Request = StackRequest;
        type Response = u32;
    }
    impl Stack {
        fn new(v: Vec<u32>) -> Self {
            Self {
                data: RwLock::new(v),
            }
        }
        fn push(&self, data: u32) -> u32 {
            self.data.write().unwrap().push(data);
            return data;
        }
        fn pop(&self) -> u32 {
            self.data.write().unwrap().pop().unwrap()
        }
    }

    impl Runner<StackAction> for Stack {
        fn init() -> Self {
            Stack::new(Vec::new())
        }
        fn invoke(
            &self,
            action: <StackAction as Action>::Request,
        ) -> <StackAction as Action>::Response {
            match action {
                StackRequest::PushAction(push_action) => self.push(push_action.data),
                StackRequest::PopAction => self.pop(),
            }
        }
    }

    impl Model<StackAction, Vec<u32>> for Stack {
        fn get_state(&self) -> Vec<u32> {
            self.data.read().unwrap().clone()
        }

        fn from_state(state: &Vec<u32>) -> Self {
            Stack::new(state.clone())
        }
    }

    #[test]
    fn single_thread_stack_test() {
        let mut checker: Checker<StackAction, Vec<u32>, Stack, Stack> = Checker::new();
        let mut history = Vec::new();
        for i in 0..1000 {
            history.push(StackRequest::PushAction(PushAction { data: i }));
        }
        checker.add_thread(history);
        checker.finish_prepare();

        assert!(checker.check(vec![], 4, 1024 * 1024 * 1024));
    }
}
