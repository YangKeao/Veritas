extern crate crossbeam_queue;
extern crate veritas;

use crossbeam_queue::ArrayQueue;
use std::collections::vec_deque::VecDeque;
use std::sync::RwLock;
use veritas::*;

struct QueueRunner {
    queue: ArrayQueue<u32>,
}
#[derive(Copy, Clone)]
struct QueueAction {}
#[derive(Copy, Clone)]
enum QueueRequest {
    PushAction(u32),
    #[allow(dead_code)]
    PopAction,
}
impl Action for QueueAction {
    type Request = QueueRequest;
    type Response = u32;
}

impl Runner<QueueAction> for QueueRunner {
    fn init() -> Self {
        Self {
            queue: ArrayQueue::new(1024),
        }
    }

    fn invoke(
        &self,
        action: <QueueAction as Action>::Request,
    ) -> <QueueAction as Action>::Response {
        match action {
            QueueRequest::PushAction(val) => {
                self.queue.push(val).unwrap();
                return val;
            }
            QueueRequest::PopAction => self.queue.pop().unwrap_or(0),
        }
    }
}

struct QueueModel {
    queue: RwLock<VecDeque<u32>>,
}
impl Runner<QueueAction> for QueueModel {
    fn init() -> Self {
        Self {
            queue: RwLock::new(VecDeque::new()),
        }
    }

    fn invoke(
        &self,
        action: <QueueAction as Action>::Request,
    ) -> <QueueAction as Action>::Response {
        match action {
            QueueRequest::PushAction(val) => {
                self.queue.write().unwrap().push_back(val);
                return val;
            }
            QueueRequest::PopAction => self.queue.write().unwrap().pop_front().unwrap_or(0),
        }
    }
}
impl Model<QueueAction, VecDeque<u32>> for QueueModel {
    fn get_state(&self) -> VecDeque<u32> {
        self.queue.read().unwrap().clone()
    }

    fn from_state(state: &VecDeque<u32>) -> Self {
        Self {
            queue: RwLock::new(state.clone()),
        }
    }
}

#[test]
fn crossbeam_queue_single_thread_test() {
    let mut checker: Checker<QueueAction, VecDeque<u32>, QueueModel, QueueRunner> = Checker::new();
    let mut push_history = Vec::new();
    for i in 0..1000 {
        push_history.push(QueueRequest::PushAction(i));
    }
    let pop_history = vec![QueueRequest::PopAction; 500];
    checker.add_thread(push_history);
    checker.add_thread(pop_history);
    checker.finish_prepare();

    assert!(checker.check(VecDeque::new(), 4));
}

#[test]
fn crossbeam_queue_multiple_thread_test() {
    let mut checker: Checker<QueueAction, VecDeque<u32>, QueueModel, QueueRunner> = Checker::new();
    let mut histories = Vec::new();
    for _ in 0..4 {
        let mut history = Vec::new();
        for i in 0..12 {
            history.push(QueueRequest::PushAction(i));
        }
        for _ in 0..8 {
            history.push(QueueRequest::PopAction);
        }
        histories.push(history);
    }

    for history in histories {
        checker.add_thread(history);
    }
    checker.finish_prepare();
    assert!(checker.check(VecDeque::new(), 4));
}
